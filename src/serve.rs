use std::collections::BTreeMap;
use std::error::Error;
use std::sync::Arc;
use std::{env, fs, io, path};
use tokio::fs as afs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;

use crate::cache::ChildCache;
use crate::logging::*;
use crate::types::FileChunk;
use crate::util;

//////////
// List //
//////////
#[derive(Clone)]
pub struct DumpState {
	pub exclude: Vec<glob::Pattern>,
	/// Thread-safe chunk tracking for concurrent access during parallel file processing
	pub chunks: Arc<Mutex<BTreeMap<String, Vec<FileChunk>>>>,
	#[allow(dead_code)]
	pub missing: Arc<Mutex<BTreeMap<String, Vec<FileChunk>>>>,
	pub rename: Arc<Mutex<BTreeMap<path::PathBuf, path::PathBuf>>>,
	/// Maps chunk hash (base64) to (temp_file_path, offset_in_file) for chunks being written
	pub chunk_writes: Arc<Mutex<BTreeMap<String, (path::PathBuf, u64)>>>,
	#[allow(dead_code)]
	pub cache: Option<ChildCache>,
}

impl DumpState {
	/// Add a chunk to the shared state (thread-safe)
	///
	/// This method acquires a lock on the chunks map and adds or updates the chunk entry.
	/// It's now async to support the Mutex-based synchronization.
	pub async fn add_chunk(&self, hash: String, path: path::PathBuf, offset: u64, size: usize) {
		let mut chunks_guard = self.chunks.lock().await;
		let v = chunks_guard.entry(hash).or_default();
		if !v.iter().any(|p| p.path == path) {
			v.push(FileChunk { path, offset, size });
		}
	}

	pub async fn read_chunk(
		&self,
		dir: &path::Path,
		hash: &str,
	) -> Result<Option<Vec<u8>>, Box<dyn Error>> {
		// Acquire lock to access chunks
		let chunks_guard = self.chunks.lock().await;
		let fc_vec_opt = chunks_guard.get(hash);

		match fc_vec_opt {
			Some(fc_vec) => {
				// Try all available file copies (resilient to file deletions/moves)
				for (attempt, fc) in fc_vec.iter().enumerate() {
					let path = dir.join(&fc.path);

					match afs::File::open(&path).await {
						Ok(mut f) => {
							// File opened successfully, try to read it
							let mut buf: Vec<u8> = vec![0; fc.size];
							match f.seek(io::SeekFrom::Start(fc.offset)).await {
								Ok(_) => {
									match f.read_exact(&mut buf).await {
										Ok(_) => {
											// Successfully read, verify hash
											let computed_hash = util::hash(&buf);
											if computed_hash != hash {
												debug!(
													"[read_chunk] Hash mismatch for chunk {} from file {}: expected {}, got {}",
													hash, fc.path.display(), hash, computed_hash
												);
												// Continue to next copy
												continue;
											}

											// Hash verified, return the data
											if attempt > 0 {
												debug!(
													"[read_chunk] Successfully read chunk {} from alternate copy (attempt {})",
													hash, attempt + 1
												);
											}
											return Ok(Some(buf));
										}
										Err(e) => {
											debug!(
												"[read_chunk] Failed to read chunk {} from {}: {} (trying next copy)",
												hash, fc.path.display(), e
											);
											// Continue to next copy
											continue;
										}
									}
								}
								Err(e) => {
									debug!(
										"[read_chunk] Failed to seek in chunk {} at {}: {} (trying next copy)",
										hash, fc.path.display(), e
									);
									// Continue to next copy
									continue;
								}
							}
						}
						Err(e) => {
							debug!(
								"[read_chunk] Cannot open file for chunk {} at {}: {} (trying next copy)",
								hash, fc.path.display(), e
							);
							// Continue to next copy
							continue;
						}
					}
				}

				// All file copies failed
				warn!(
					"[read_chunk] Failed to read chunk {} from all {} available copies",
					hash,
					fc_vec.len()
				);
				Err(format!("Failed to read chunk {} from all {} file copies", hash, fc_vec.len())
					.into())
			}
			None => Ok(None),
		}
	}
}

// Clean up orphaned temporary files from interrupted syncs
fn cleanup_temp_files(dir: &path::Path) -> Result<(), Box<dyn Error>> {
	info!("Cleaning up orphaned temporary files...");
	let mut count = 0;

	// Walk directory tree looking for .SyNcR-TmP files (synchronous version)
	fn scan_dir(dir: &path::Path, count: &mut u32) -> Result<(), Box<dyn Error>> {
		let entries = match fs::read_dir(dir) {
			Ok(e) => e,
			Err(e) => {
				warn!("Cannot read directory {} during cleanup: {}", dir.display(), e);
				return Ok(());
			}
		};

		for entry_result in entries {
			let entry = match entry_result {
				Ok(e) => e,
				Err(e) => {
					debug!("Error reading directory entry during cleanup: {}", e);
					continue;
				}
			};

			let path = entry.path();
			let metadata = match fs::symlink_metadata(&path) {
				Ok(m) => m,
				Err(e) => {
					warn!("Cannot access {} during cleanup: {}", path.display(), e);
					continue;
				}
			};

			if let Some(name) = path.file_name() {
				if let Some(name_str) = name.to_str() {
					if name_str.ends_with(".SyNcR-TmP") {
						debug!("Removing orphaned temp file: {:?}", path);
						let remove_result = if metadata.is_file() {
							fs::remove_file(&path)
						} else if metadata.is_dir() {
							fs::remove_dir_all(&path)
						} else {
							Ok(())
						};

						// Ignore "not found" errors - file may have been deleted already
						match remove_result {
							Ok(_) => *count += 1,
							Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
								debug!("Temp file already removed: {:?}", path);
							}
							Err(e) => {
								warn!("Failed to remove temp file {:?}: {}", path, e);
							}
						}
					}
				}
			}

			// Recursively scan subdirectories
			if metadata.is_dir() {
				scan_dir(&path, count)?;
			}
		}
		Ok(())
	}

	scan_dir(dir, &mut count)?;
	info!("Cleaned up {} temporary files", count);
	Ok(())
}

pub async fn serve(dir: &str) -> Result<(), Box<dyn Error>> {
	// Change directory first, before any protocol communication
	env::set_current_dir(dir)?;

	// ─── NEGOTIATION PHASE ───
	// Server announces what versions it supports
	println!("{}", crate::protocol::negotiation::server_capabilities_message());
	tokio::io::stdout().flush().await?;

	// Set up async I/O
	let stdin = tokio::io::stdin();
	let mut stdout = tokio::io::stdout();
	let mut reader = tokio::io::BufReader::new(stdin);
	let mut line = String::new();

	// Wait for client's version decision (USE:Z)
	let _selected_version = loop {
		line.clear();
		let n = tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line).await?;
		if n == 0 {
			return Err("Client closed connection before sending version decision".into());
		}

		let trimmed = line.trim();
		if trimmed.starts_with("USE:") {
			// Parse version selection: "USE:3"
			match parse_use_message(trimmed) {
				Ok(version) => {
					// Validate server supports this version
					if crate::protocol::negotiation::is_version_supported(version) {
						// Send ready acknowledgment
						stdout.write_all(b"READY\n").await?;
						stdout.flush().await?;
						break version;
					} else {
						eprintln!("!E:Client requested unsupported version: {}", version);
						return Err(format!("Unsupported version: {}", version).into());
					}
				}
				Err(e) => {
					eprintln!("!E:Failed to parse USE message: {}", e);
					return Err(e);
				}
			}
		} else if !trimmed.is_empty() && !trimmed.starts_with('#') && !trimmed.starts_with('!') {
			eprintln!("!E:Expected USE: message during negotiation, got: {}", trimmed);
			return Err("Expected USE: message".into());
		}
	};

	// Clean up orphaned temp files from any interrupted previous syncs
	// Note: We don't initialize protocol propagation here because serve is a child process
	// without a parent to send logs to. Logs should go to stderr if needed.
	cleanup_temp_files(path::Path::new("."))?;

	// Create V3 protocol server and run it
	// The server will handle:
	// - All protocol commands (LIST, READ, WRITE, COMMIT, etc.)
	let base_path = path::PathBuf::from(".");
	let dump_state = DumpState {
		exclude: vec![glob::Pattern::new("**/*.SyNcR-TmP")?],
		chunks: Arc::new(Mutex::new(BTreeMap::new())),
		missing: Arc::new(Mutex::new(BTreeMap::new())),
		rename: Arc::new(Mutex::new(BTreeMap::new())),
		chunk_writes: Arc::new(Mutex::new(BTreeMap::new())),
		cache: None,
	};

	// Create and run V3 server (this handles all protocol operations)
	let server = crate::protocol::v3_server::ProtocolV3Server::new(base_path, dump_state);

	// Run server using pure async I/O
	// v3_server.run() will handle the entire protocol loop including:
	// - LIST, READ, WRITE, COMMIT commands
	// - QUIT (graceful shutdown)
	server.run(reader, stdout).await?;

	Ok(())
}

/// Parse USE message: "USE:3" -> 3
fn parse_use_message(msg: &str) -> Result<u32, Box<dyn Error>> {
	if !msg.starts_with("USE:") {
		return Err("Expected USE: prefix".into());
	}
	let version_str = &msg[4..]; // Skip "USE:"
	Ok(version_str.trim().parse::<u32>()?)
}

// vim: ts=4
