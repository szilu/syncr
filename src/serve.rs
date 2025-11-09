use base64::{engine::general_purpose, Engine as _};
use rollsum::Bup;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::error::Error;
use std::os::unix::{fs::MetadataExt, prelude::PermissionsExt};
use std::{env, fs, io, path, pin::Pin};
use tokio::fs as afs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
//use std::{thread, time};

use crate::cache::ChildCache;
use crate::config;
use crate::logging::*;
#[allow(unused_imports)]
use crate::metadata_utils;
use crate::protocol_utils;
use crate::types::{FileChunk, HashChunk};
use crate::util;

// V3 Protocol message structures
#[derive(Serialize, Deserialize, Debug, Clone)]
struct VersionCommand {
	cmd: String,
	ver: i32,
	#[serde(skip_serializing_if = "Option::is_none")]
	log: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct VersionResponse {
	cmd: String,
	ver: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ErrorResponse {
	cmd: String,
	msg: String,
}

// V3 Protocol data structures for file/directory listing and transfer
#[derive(Serialize, Deserialize, Debug, Clone)]
struct FileEntity {
	#[serde(rename = "typ")]
	entity_type: String, // "F"
	#[serde(rename = "pth")]
	path: String,
	#[serde(rename = "mod", skip_serializing_if = "Option::is_none")]
	mode: Option<u32>,
	#[serde(rename = "uid", skip_serializing_if = "Option::is_none")]
	user_id: Option<u32>,
	#[serde(rename = "gid", skip_serializing_if = "Option::is_none")]
	group_id: Option<u32>,
	#[serde(rename = "ct", skip_serializing_if = "Option::is_none")]
	created_time: Option<u64>,
	#[serde(rename = "mt", skip_serializing_if = "Option::is_none")]
	modified_time: Option<u64>,
	#[serde(rename = "sz", skip_serializing_if = "Option::is_none")]
	size: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct DirectoryEntity {
	#[serde(rename = "typ")]
	entity_type: String, // "D"
	#[serde(rename = "pth")]
	path: String,
	#[serde(rename = "mod", skip_serializing_if = "Option::is_none")]
	mode: Option<u32>,
	#[serde(rename = "uid", skip_serializing_if = "Option::is_none")]
	user_id: Option<u32>,
	#[serde(rename = "gid", skip_serializing_if = "Option::is_none")]
	group_id: Option<u32>,
	#[serde(rename = "ct", skip_serializing_if = "Option::is_none")]
	created_time: Option<u64>,
	#[serde(rename = "mt", skip_serializing_if = "Option::is_none")]
	modified_time: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SymlinkEntity {
	#[serde(rename = "typ")]
	entity_type: String, // "S"
	#[serde(rename = "pth")]
	path: String,
	#[serde(rename = "mod", skip_serializing_if = "Option::is_none")]
	mode: Option<u32>,
	#[serde(rename = "uid", skip_serializing_if = "Option::is_none")]
	user_id: Option<u32>,
	#[serde(rename = "gid", skip_serializing_if = "Option::is_none")]
	group_id: Option<u32>,
	#[serde(rename = "ct", skip_serializing_if = "Option::is_none")]
	created_time: Option<u64>,
	#[serde(rename = "mt", skip_serializing_if = "Option::is_none")]
	modified_time: Option<u64>,
	#[serde(rename = "tgt", skip_serializing_if = "Option::is_none")]
	target: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ChunkEntity {
	#[serde(rename = "typ")]
	entity_type: String, // "C"
	#[serde(rename = "off")]
	offset: u64,
	#[serde(rename = "len")]
	length: u32,
	#[serde(rename = "hsh")]
	hash: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ChunkHeader {
	cmd: String,
	#[serde(rename = "hsh")]
	hash: String,
	#[serde(rename = "len")]
	length: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct EndCommand {
	cmd: String,
}

/// Protocol version for handshake and compatibility checking (V3 only)
const PROTOCOL_VERSION_V3: i32 = 3;

// Type alias for complex async function return type
type BoxedAsyncResult<'a> =
	Pin<Box<dyn std::future::Future<Output = Result<(), Box<dyn Error>>> + 'a>>;

/// Macro for resilient protocol output - ignores broken pipe errors
/// This prevents child processes from panicking when parent closes the pipe
macro_rules! protocol_println {
	($($arg:tt)*) => {{
		use std::io::Write;
		let result = writeln!(std::io::stdout(), $($arg)*);
		match result {
			Ok(_) => {
				// Flush to ensure message is sent immediately
				let _ = std::io::stdout().flush();
			}
			Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
				// Parent closed pipe or not reading yet - silently ignore
				// Child will be stopped via QUIT command
			}
			Err(e) => {
				// Other errors - continue but log to stderr
				eprintln!("Protocol write error: {}", e);
			}
		}
	}};
}

///////////
// Utils //
///////////

/// Helper function for resilient stdout writes
fn write_stdout(data: &[u8]) -> io::Result<()> {
	use std::io::Write;
	let mut stdout = io::stdout();
	match stdout.write_all(data) {
		Ok(()) => {
			stdout.flush()?;
			Ok(())
		}
		Err(e) if e.kind() == io::ErrorKind::BrokenPipe => {
			// Parent closed pipe or not reading yet - pretend success
			Ok(())
		}
		Err(e) => Err(e),
	}
}

/// Validate that a path is safe to use (prevents directory traversal/injection attacks)
/// Only allows relative paths without parent directory references
fn validate_path(path: &path::Path) -> Result<(), Box<dyn Error>> {
	// Reject absolute paths
	if path.is_absolute() {
		return Err("Absolute paths are not allowed".into());
	}

	// Reject paths with parent directory components (..)
	for component in path.components() {
		if component == std::path::Component::ParentDir {
			return Err("Parent directory traversal (..) is not allowed".into());
		}
	}

	// Reject current directory references (.) - they're redundant and could be suspicious
	if path.as_os_str().is_empty() {
		return Err("Empty paths are not allowed".into());
	}

	// Additional check: reject paths that are exactly "."
	if path == path::Path::new(".") {
		return Err("Current directory reference (.) is not allowed".into());
	}

	Ok(())
}

//////////
// List //
//////////
pub struct DumpState {
	pub exclude: Vec<glob::Pattern>,
	pub chunks: BTreeMap<String, Vec<FileChunk>>,
	#[allow(dead_code)]
	pub missing: RefCell<BTreeMap<String, Vec<FileChunk>>>,
	pub rename: RefCell<BTreeMap<path::PathBuf, path::PathBuf>>,
	pub cache: Option<ChildCache>,
}

impl DumpState {
	fn add_chunk(&mut self, hash: String, path: path::PathBuf, offset: u64, size: usize) {
		let v = self.chunks.entry(hash).or_default();
		if !v.iter().any(|p| p.path == path) {
			v.push(FileChunk { path, offset, size });
		}
	}

	async fn read_chunk(
		&self,
		dir: &path::Path,
		hash: &str,
	) -> Result<Option<Vec<u8>>, Box<dyn Error>> {
		let fc_vec_opt = self.chunks.get(hash);

		match fc_vec_opt {
			Some(fc_vec) => {
				let fc = &fc_vec[0];
				let path = dir.join(&fc.path);
				let mut f = afs::File::open(&path).await?;
				let mut buf: Vec<u8> = vec![0; fc.size];
				f.seek(io::SeekFrom::Start(fc.offset)).await?;
				f.read_exact(&mut buf).await?;

				// Verify hash to detect corruption
				let computed_hash = util::hash(&buf);
				if computed_hash != hash {
					return Err(format!(
						"Hash mismatch for chunk {}: expected {}, got {}",
						hash, hash, computed_hash
					)
					.into());
				}

				Ok(Some(buf))
			}
			None => Ok(None),
		}
	}

	async fn write_chunk(
		&self,
		path: &path::Path,
		chunk: &HashChunk,
		buf: &[u8],
	) -> Result<(), Box<dyn Error>> {
		// Verify hash before writing to detect corruption during transfer
		let computed_hash = util::hash_binary(buf);
		if computed_hash != chunk.hash {
			let expected_b64 = crate::util::hash_to_base64(&chunk.hash);
			let computed_b64 = crate::util::hash_to_base64(&computed_hash);
			return Err(format!(
				"Hash mismatch for chunk: expected {}, got {}",
				expected_b64, computed_b64
			)
			.into());
		}

		let mut f = afs::OpenOptions::new()
			.write(true)
			.create(true)
			.truncate(false)
			.open(&path)
			.await?;
		f.seek(io::SeekFrom::Start(chunk.offset)).await?;
		f.write_all(buf).await?;
		Ok(())
	}
}

fn traverse_dir<'a>(state: &'a mut DumpState, dir: path::PathBuf) -> BoxedAsyncResult<'a> {
	Box::pin(async move {
		let entries = match fs::read_dir(&dir) {
			Ok(e) => e,
			Err(e) => {
				warn!("Cannot read directory {}: {}", dir.display(), e);
				return Ok(());
			}
		};

		for entry_result in entries {
			let entry = match entry_result {
				Ok(e) => e,
				Err(e) => {
					debug!("Error reading directory entry: {}", e);
					continue;
				}
			};
			let path = entry.path();
			if state.exclude[0].matches_path(&path) {
				continue;
			}

			let meta = match fs::symlink_metadata(&path) {
				Ok(m) => m,
				Err(e) => {
					warn!("Cannot access {}: {}", path.display(), e);
					continue;
				}
			};

			if meta.is_file() {
				protocol_println!(
					"F:{}:{}:{}:{}:{}:{}:{}",
					&path.to_string_lossy(),
					meta.mode(),
					meta.uid(),
					meta.gid(),
					meta.ctime(),
					meta.mtime(),
					meta.size()
				);

				// Check cache first
				let rel_path = path.to_string_lossy().to_string();
				let mtime = meta.mtime() as u32;

				if let Some(ref cache) = state.cache {
					match cache.get_chunks(&rel_path, mtime) {
						Ok(Some(cached_chunks)) => {
							// Cache hit - use cached chunks without reading file
							info!("Cache hit: {}", rel_path);
							for chunk in &cached_chunks {
								let hash_b64 = util::hash_to_base64(&chunk.hash);
								protocol_println!(
									"C:{}:{}:{}",
									chunk.offset,
									chunk.size,
									&hash_b64
								);
								state.add_chunk(
									hash_b64,
									path.clone(),
									chunk.offset,
									chunk.size as usize,
								);
							}
							continue;
						}
						Ok(None) => {
							// Cache miss - need to hash file
							info!("Cache miss: {}", rel_path);
						}
						Err(e) => {
							warn!("Cache lookup error for {}: {}", rel_path, e);
						}
					}
				}

				// Cache miss or no cache - hash the file
				let mut chunks_for_cache = Vec::new();

				let mut f = match afs::File::open(&path).await {
					Ok(file) => file,
					Err(e) => {
						warn!("Cannot open file {}: {}", path.display(), e);
						continue;
					}
				};

				let mut buf: Vec<u8> = vec![0; config::MAX_CHUNK_SIZE];

				let mut n = match f.read(&mut buf).await {
					Ok(bytes) => bytes,
					Err(e) => {
						warn!("Cannot read file {}: {}", path.display(), e);
						continue;
					}
				};

				let mut offset: u64 = 0;
				//let mut bup = Bup::new_with_chunk_bits(config::CHUNK_BITS);
				while n > 0 {
					let mut bup = Bup::new_with_chunk_bits(config::CHUNK_BITS);
					let mut endofs = config::MAX_CHUNK_SIZE;
					if endofs > n {
						endofs = n
					}
					if let Some((count, _hash)) = bup.find_chunk_edge(&buf[..endofs]) {
						let h = util::hash(&buf[..count]);
						let hash_binary = util::hash_binary(&buf[..count]);
						protocol_println!("C:{}:{}:{}", offset, count, &h);
						//state.chunks.insert(h, path.clone());
						state.add_chunk(h, path.clone(), offset, count);

						// Store chunk for caching
						chunks_for_cache.push(HashChunk {
							hash: hash_binary,
							offset,
							size: count as u32,
						});

						// Shift remaining unprocessed bytes to the front of the buffer
						// Safe alternative to unsafe ptr::copy
						buf.copy_within(count..n, 0);
						offset += count as u64;
						n -= count;
					} else {
						let count = endofs;
						let h = util::hash(&buf[..count]);
						let hash_binary = util::hash_binary(&buf[..count]);
						protocol_println!("C:{}:{}:{}", offset, count, &h);
						//state.chunks.insert(h, path.clone());
						state.add_chunk(h, path.clone(), offset, count);

						// Store chunk for caching
						chunks_for_cache.push(HashChunk {
							hash: hash_binary,
							offset,
							size: count as u32,
						});

						offset += count as u64;
						n -= count;
					}

					let read_result = f.read(&mut buf[n..]).await;
					match read_result {
						Ok(bytes) => n += bytes,
						Err(e) => {
							warn!(
								"Error reading file {} at offset {}: {}",
								path.display(),
								offset,
								e
							);
							break;
						}
					}
				}

				// Store chunks in cache
				if let Some(ref cache) = state.cache {
					let entry = crate::cache::CacheEntry {
						mtime,
						uid: meta.uid(),
						gid: meta.gid(),
						ctime: meta.ctime() as u32,
						size: meta.size(),
						mode: meta.mode(),
						chunks: chunks_for_cache,
					};
					match cache.set(&rel_path, entry) {
						Ok(_) => info!("Cached chunks for: {}", rel_path),
						Err(e) => warn!("Failed to cache chunks for {}: {}", rel_path, e),
					}
				}
			}
			if meta.file_type().is_symlink() {
				let target = match fs::read_link(&path) {
					Ok(target_path) => target_path.to_string_lossy().to_string(),
					Err(_) => String::new(),
				};
				protocol_println!(
					"L:{}:{}:{}:{}:{}:{}:{}",
					&path.to_string_lossy(),
					meta.mode(),
					meta.uid(),
					meta.gid(),
					meta.ctime(),
					meta.mtime(),
					target
				);
			}
			if meta.is_dir() {
				protocol_println!(
					"D:{}:{}:{}:{}:{}:{}",
					&path.to_string_lossy(),
					meta.mode(),
					meta.uid(),
					meta.gid(),
					meta.ctime(),
					meta.mtime()
				);
				// Note: The above now includes UID/GID/ctime - old 3-field format removed
				traverse_dir(state, path).await?
			}
		}
		Ok(())
	})
}

//////////////
// V3 Handlers //
//////////////

/// Traverse directory and output as v3 JSON5 format
fn traverse_dir_v3<'a>(state: &'a mut DumpState, dir: path::PathBuf) -> BoxedAsyncResult<'a> {
	Box::pin(async move {
		let entries = match fs::read_dir(&dir) {
			Ok(e) => e,
			Err(e) => {
				warn!("Cannot read directory {}: {}", dir.display(), e);
				return Ok(());
			}
		};

		for entry_result in entries {
			let entry = match entry_result {
				Ok(e) => e,
				Err(e) => {
					debug!("Error reading directory entry: {}", e);
					continue;
				}
			};
			let path = entry.path();
			if state.exclude[0].matches_path(&path) {
				continue;
			}

			let meta = match fs::symlink_metadata(&path) {
				Ok(m) => m,
				Err(e) => {
					warn!("Cannot access {}: {}", path.display(), e);
					continue;
				}
			};

			if meta.is_file() {
				// Output file entity as JSON5
				let file_entity = FileEntity {
					entity_type: "F".to_string(),
					path: path.to_string_lossy().to_string(),
					mode: Some(meta.mode()),
					user_id: Some(meta.uid()),
					group_id: Some(meta.gid()),
					created_time: Some(meta.ctime() as u64),
					modified_time: Some(meta.mtime() as u64),
					size: Some(meta.size()),
				};
				if let Ok(json_str) = serde_json::to_string(&file_entity) {
					protocol_println!("{}", json_str);
				}

				// Check cache first
				let rel_path = path.to_string_lossy().to_string();
				let mtime = meta.mtime() as u32;

				if let Some(ref cache) = state.cache {
					match cache.get_chunks(&rel_path, mtime) {
						Ok(Some(cached_chunks)) => {
							// Cache hit - use cached chunks without reading file
							info!("Cache hit: {}", rel_path);
							for chunk in &cached_chunks {
								let hash_b64 = util::hash_to_base64(&chunk.hash);
								let chunk_entity = ChunkEntity {
									entity_type: "C".to_string(),
									offset: chunk.offset,
									length: chunk.size,
									hash: hash_b64.clone(),
								};
								if let Ok(json_str) = serde_json::to_string(&chunk_entity) {
									protocol_println!("{}", json_str);
								}
								state.add_chunk(
									hash_b64,
									path.clone(),
									chunk.offset,
									chunk.size as usize,
								);
							}
							continue;
						}
						Ok(None) => {
							// Cache miss - need to hash file
							info!("Cache miss: {}", rel_path);
						}
						Err(e) => {
							warn!("Cache lookup error for {}: {}", rel_path, e);
						}
					}
				}

				// Cache miss or no cache - hash the file
				let mut chunks_for_cache = Vec::new();

				let mut f = match afs::File::open(&path).await {
					Ok(file) => file,
					Err(e) => {
						warn!("Cannot open file {}: {}", path.display(), e);
						continue;
					}
				};

				let mut buf: Vec<u8> = vec![0; config::MAX_CHUNK_SIZE];

				let mut n = match f.read(&mut buf).await {
					Ok(bytes) => bytes,
					Err(e) => {
						warn!("Cannot read file {}: {}", path.display(), e);
						continue;
					}
				};

				let mut offset: u64 = 0;
				while n > 0 {
					let mut bup = Bup::new_with_chunk_bits(config::CHUNK_BITS);
					let mut endofs = config::MAX_CHUNK_SIZE;
					if endofs > n {
						endofs = n
					}
					if let Some((count, _hash)) = bup.find_chunk_edge(&buf[..endofs]) {
						let h = util::hash(&buf[..count]);
						let hash_binary = util::hash_binary(&buf[..count]);

						// Output chunk entity as JSON5
						let chunk_entity = ChunkEntity {
							entity_type: "C".to_string(),
							offset,
							length: count as u32,
							hash: h.clone(),
						};
						if let Ok(json_str) = serde_json::to_string(&chunk_entity) {
							protocol_println!("{}", json_str);
						}

						state.add_chunk(h, path.clone(), offset, count);
						chunks_for_cache.push(HashChunk {
							hash: hash_binary,
							offset,
							size: count as u32,
						});

						buf.copy_within(count..n, 0);
						offset += count as u64;
						n -= count;
					} else {
						let count = endofs;
						let h = util::hash(&buf[..count]);
						let hash_binary = util::hash_binary(&buf[..count]);

						// Output chunk entity as JSON5
						let chunk_entity = ChunkEntity {
							entity_type: "C".to_string(),
							offset,
							length: count as u32,
							hash: h.clone(),
						};
						if let Ok(json_str) = serde_json::to_string(&chunk_entity) {
							protocol_println!("{}", json_str);
						}

						state.add_chunk(h, path.clone(), offset, count);
						chunks_for_cache.push(HashChunk {
							hash: hash_binary,
							offset,
							size: count as u32,
						});

						offset += count as u64;
						n -= count;
					}

					let read_result = f.read(&mut buf[n..]).await;
					match read_result {
						Ok(bytes) => n += bytes,
						Err(e) => {
							warn!("Error reading file {}: {}", path.display(), e);
							break;
						}
					}
				}
			} else if meta.is_symlink() {
				// Output symlink entity as JSON5
				let target = match fs::read_link(&path) {
					Ok(t) => Some(t.to_string_lossy().to_string()),
					Err(e) => {
						warn!("Cannot read symlink {}: {}", path.display(), e);
						None
					}
				};

				let symlink_entity = SymlinkEntity {
					entity_type: "S".to_string(),
					path: path.to_string_lossy().to_string(),
					mode: Some(meta.mode()),
					user_id: Some(meta.uid()),
					group_id: Some(meta.gid()),
					created_time: Some(meta.ctime() as u64),
					modified_time: Some(meta.mtime() as u64),
					target,
				};
				if let Ok(json_str) = serde_json::to_string(&symlink_entity) {
					protocol_println!("{}", json_str);
				}
			} else if meta.is_dir() {
				// Output directory entity as JSON5
				let dir_entity = DirectoryEntity {
					entity_type: "D".to_string(),
					path: path.to_string_lossy().to_string(),
					mode: Some(meta.mode()),
					user_id: Some(meta.uid()),
					group_id: Some(meta.gid()),
					created_time: Some(meta.ctime() as u64),
					modified_time: Some(meta.mtime() as u64),
				};
				if let Ok(json_str) = serde_json::to_string(&dir_entity) {
					protocol_println!("{}", json_str);
				}

				// Recursively process subdirectory
				Box::pin(traverse_dir_v3(state, path)).await?;
			}
		}
		Ok(())
	})
}

/// Serve LIST command for v3 protocol (outputs JSON5 format)
async fn serve_list_v3(dir: path::PathBuf) -> Result<DumpState, Box<dyn Error>> {
	// Initialize global cache
	let cache = {
		let cache_dir = path::PathBuf::from(env::var("HOME").unwrap_or_else(|_| ".".to_string()))
			.join(".syncr/cache");
		let _ = fs::create_dir_all(&cache_dir);

		let cache_db_path = cache_dir.join("cache.db");

		match ChildCache::open(&cache_db_path) {
			Ok(c) => {
				info!("Cache opened: {}", cache_db_path.display());
				Some(c)
			}
			Err(e) => {
				info!("Cache unavailable (continuing without): {}", e);
				if let Err(remove_err) = fs::remove_file(&cache_db_path) {
					debug!("Could not remove old cache: {}", remove_err);
				}
				match ChildCache::open(&cache_db_path) {
					Ok(c) => {
						info!("Cache recovered by recreating database");
						Some(c)
					}
					Err(_) => None,
				}
			}
		}
	};

	let mut state = DumpState {
		exclude: vec![glob::Pattern::new("**/*.SyNcR-TmP")?],
		chunks: BTreeMap::new(),
		missing: RefCell::new(BTreeMap::new()),
		rename: RefCell::new(BTreeMap::new()),
		cache,
	};
	traverse_dir_v3(&mut state, dir).await?;

	// Send END marker (V3 protocol)
	let end_cmd = EndCommand { cmd: "END".to_string() };
	if let Ok(json_str) = serde_json::to_string(&end_cmd) {
		protocol_println!("{}", json_str);
	}

	// Send "." terminator (expected by parent's do_collect() for protocol compatibility)
	protocol_println!(".");

	Ok(state)
}

/// Serve READ command for v3 protocol (binary chunk transfer)
async fn serve_read_v3(dir: path::PathBuf, dump_state: &DumpState) -> Result<(), Box<dyn Error>> {
	let mut chunks: Vec<String> = Vec::new();
	let mut buf = String::new();

	// Read hash list from parent (ends with "END")
	loop {
		buf.clear();
		io::stdin()
			.read_line(&mut buf)
			.map_err(|e| format!("Failed to read chunk request: {}", e))?;

		let trimmed = buf.trim();

		// Try parsing as JSON5 to extract hash
		if let Ok(json_obj) = json5::from_str::<serde_json::Value>(trimmed) {
			if let Some(cmd) = json_obj.get("cmd").and_then(|v| v.as_str()) {
				if cmd == "END" {
					break;
				}
			}
			if let Some(hsh) = json_obj.get("hsh").and_then(|v| v.as_str()) {
				chunks.push(hsh.to_string());
			}
		}
	}

	// Send each chunk as binary data
	for hash in &chunks {
		if let Ok(Some(chunk_data)) = dump_state.read_chunk(&dir, hash).await {
			// Send CHK header with hash and length
			let header = ChunkHeader {
				cmd: "CHK".to_string(),
				hash: hash.clone(),
				length: chunk_data.len() as u32,
			};
			if let Ok(json_str) = serde_json::to_string(&header) {
				protocol_println!("{}", json_str);
			}

			// Send binary data
			write_stdout(&chunk_data)?;
			write_stdout(b"\n")?;
		} else {
			// Chunk not found - send error
			let error =
				ErrorResponse { cmd: "ERR".to_string(), msg: format!("Chunk not found: {}", hash) };
			if let Ok(json_str) = serde_json::to_string(&error) {
				protocol_println!("{}", json_str);
			}
		}
	}

	// Send END marker to signal completion of READ
	let end_cmd = EndCommand { cmd: "END".to_string() };
	if let Ok(json_str) = serde_json::to_string(&end_cmd) {
		protocol_println!("{}", json_str);
	}

	Ok(())
}

/// Serve WRITE command for v3 protocol (receives metadata and binary chunks)
async fn serve_write_v3(_dir: path::PathBuf, dump_state: &DumpState) -> Result<(), Box<dyn Error>> {
	use std::io::Read;

	let mut buf = String::new();
	let mut _file: Option<afs::File> = None;
	let mut filepath = path::PathBuf::from("");
	// Track chunk metadata: (hash, offset, length, target_file_path)
	let mut expected_chunks: Vec<(String, u64, u32, path::PathBuf)> = Vec::new();
	let mut current_chunk_data: Vec<u8> = Vec::new();
	let mut _current_chunk_hash: String = String::new();

	let mut stdin = io::stdin();

	loop {
		buf.clear();
		stdin.read_line(&mut buf)?;

		let trimmed = buf.trim();

		// Try parsing as JSON5
		if let Ok(json_obj) = json5::from_str::<serde_json::Value>(trimmed) {
			if let Some(cmd) = json_obj.get("cmd").and_then(|v| v.as_str()) {
				match cmd {
					"END" => break, // End of WRITE command
					"CHK" => {
						// Chunk header with binary data coming
						if let (Some(hsh), Some(len)) = (
							json_obj.get("hsh").and_then(|v| v.as_str()),
							json_obj.get("len").and_then(|v| v.as_u64()),
						) {
							// Read binary chunk data from stdin
							current_chunk_data.clear();
							current_chunk_data.resize(len as usize, 0);
							stdin
								.read_exact(&mut current_chunk_data)
								.map_err(|e| format!("Failed to read chunk data: {}", e))?;

							// Consume the newline after binary data
							let mut newline_buf = [0u8; 1];
							stdin.read_exact(&mut newline_buf).ok();

							_current_chunk_hash = hsh.to_string();

							// Write chunk to target files based on expected_chunks metadata
							let hash_binary = crate::util::base64_to_hash(hsh)
								.map_err(|e| format!("Invalid hash: {}", e))?;

							// Find all chunks with this hash and write to their target files
							for (chunk_hash, chunk_offset, chunk_len, target_file) in
								&expected_chunks
							{
								if chunk_hash == hsh {
									let hc = HashChunk {
										hash: hash_binary,
										offset: *chunk_offset,
										size: *chunk_len,
									};
									if let Err(e) = dump_state
										.write_chunk(target_file, &hc, &current_chunk_data)
										.await
									{
										protocol_println!("{{\"cmd\":\"ERR\",\"msg\":\"Failed to write chunk to {}: {}\"}}", target_file.display(), e);
									}
								}
							}
						}
					}
					_ => {}
				}
			}

			// Check for file/directory entity (type field)
			if let Some(typ) = json_obj.get("typ").and_then(|v| v.as_str()) {
				match typ {
					"C" => {
						// Chunk metadata entity - record it for later when chunk data arrives
						if let (Some(hsh), Some(off), Some(len)) = (
							json_obj.get("hsh").and_then(|v| v.as_str()),
							json_obj.get("off").and_then(|v| v.as_u64()),
							json_obj.get("len").and_then(|v| v.as_u64()),
						) {
							// Store chunk with the current filepath (the file this chunk belongs to)
							expected_chunks.push((
								hsh.to_string(),
								off,
								len as u32,
								filepath.clone(),
							));
						}
					}
					"F" => {
						// File entity
						if let Some(pth) = json_obj.get("pth").and_then(|v| v.as_str()) {
							filepath = path::PathBuf::from(pth);
							validate_path(&filepath)?;

							// Create temp file
							let filename =
								filepath.file_name().ok_or("Path has no filename")?.to_os_string();
							let mut tmp_filename = filename;
							tmp_filename.push(".SyNcR-TmP");
							filepath.set_file_name(tmp_filename);

							_file = Some(afs::File::create(&filepath).await?);

							// Set permissions if provided
							if let Some(mod_val) = json_obj.get("mod").and_then(|v| v.as_u64()) {
								afs::set_permissions(
									&filepath,
									std::fs::Permissions::from_mode(mod_val as u32),
								)
								.await
								.ok();
							}

							// Track for rename during COMMIT
							dump_state
								.rename
								.borrow_mut()
								.insert(filepath.clone(), path::PathBuf::from(pth));
						}
					}
					"D" => {
						// Directory entity
						if let Some(pth) = json_obj.get("pth").and_then(|v| v.as_str()) {
							let dir_path = path::PathBuf::from(pth);
							validate_path(&dir_path)?;

							match afs::create_dir(&dir_path).await {
								Ok(_) => {}
								Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
								Err(e) => return Err(e.into()),
							}

							if let Some(mod_val) = json_obj.get("mod").and_then(|v| v.as_u64()) {
								afs::set_permissions(
									&dir_path,
									std::fs::Permissions::from_mode(mod_val as u32),
								)
								.await
								.ok();
							}
						}
					}
					"S" => {
						// Symlink entity
						if let Some(pth) = json_obj.get("pth").and_then(|v| v.as_str()) {
							let symlink_path = path::PathBuf::from(pth);
							validate_path(&symlink_path)?;

							if let Some(target) = json_obj.get("tgt").and_then(|v| v.as_str()) {
								if afs::metadata(&symlink_path).await.is_ok() {
									afs::remove_file(&symlink_path).await.ok();
								}
								afs::symlink(target, &symlink_path).await.ok();
							}
						}
					}
					_ => {}
				}
			}
		}
	}

	// Send completion response
	protocol_println!("{{\"cmd\":\"OK\"}}");

	Ok(())
}

/// Serve COMMIT command for v3 protocol (rename temp files to final locations)
async fn serve_commit_v3(
	_dir: path::PathBuf,
	dump_state: &DumpState,
) -> Result<(), Box<dyn Error>> {
	let renames_data: Vec<_> =
		dump_state.rename.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
	let mut renamed_count = 0;
	let mut failed_count = 0;

	for (tmp_path, final_path) in renames_data.iter() {
		match afs::rename(tmp_path, final_path).await {
			Ok(_) => renamed_count += 1,
			Err(e) => {
				error!("Failed to rename {:?} to {:?}: {}", tmp_path, final_path, e);
				failed_count += 1;
			}
		}
	}

	// Send COMMIT response
	protocol_println!(
		"{{\"cmd\":\"OK\",\"renamed\":{},\"failed\":{}}}",
		renamed_count,
		failed_count
	);

	Ok(())
}

/// Serve CANCEL command for v3 protocol (clean up temporary files and abort operation)
async fn serve_cancel_v3(
	_dir: path::PathBuf,
	dump_state: &DumpState,
) -> Result<(), Box<dyn Error>> {
	let tmp_paths: Vec<_> = dump_state.rename.borrow().keys().cloned().collect();
	let mut cleaned_count = 0;
	let mut failed_count = 0;

	// Iterate through all temp files that were created but not yet committed
	for tmp_path in tmp_paths.iter() {
		// Try to remove the temp file
		match afs::remove_file(tmp_path).await {
			Ok(_) => {
				debug!("Cancelled temp file: {:?}", tmp_path);
				cleaned_count += 1;
			}
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
				// File doesn't exist - might have been deleted already
				debug!("Temp file already missing: {:?}", tmp_path);
				cleaned_count += 1;
			}
			Err(e) => {
				error!("Failed to remove temp file {:?}: {}", tmp_path, e);
				failed_count += 1;
			}
		}
	}

	// Clear the rename map to prevent any renames from occurring later
	dump_state.rename.borrow_mut().clear();

	// Send CANCEL response with cleanup statistics
	protocol_println!(
		"{{\"cmd\":\"OK\",\"cleaned\":{},\"failed\":{}}}",
		cleaned_count,
		failed_count
	);

	Ok(())
}

pub fn serve_list(dir: path::PathBuf) -> Result<DumpState, Box<dyn Error>> {
	// Initialize global cache
	let cache = {
		let cache_dir = path::PathBuf::from(env::var("HOME").unwrap_or_else(|_| ".".to_string()))
			.join(".syncr/cache");
		let _ = fs::create_dir_all(&cache_dir);

		let cache_db_path = cache_dir.join("cache.db");

		match ChildCache::open(&cache_db_path) {
			Ok(c) => {
				info!("Cache opened: {}", cache_db_path.display());
				Some(c)
			}
			Err(e) => {
				info!("Cache unavailable (continuing without): {}", e);
				// Try to remove and recreate corrupted cache
				if let Err(remove_err) = fs::remove_file(&cache_db_path) {
					debug!("Could not remove old cache: {}", remove_err);
				}
				match ChildCache::open(&cache_db_path) {
					Ok(c) => {
						info!("Cache recovered by recreating database");
						Some(c)
					}
					Err(_) => None,
				}
			}
		}
	};

	let mut state = DumpState {
		exclude: vec![glob::Pattern::new("**/*.SyNcR-TmP")?],
		chunks: BTreeMap::new(),
		missing: RefCell::new(BTreeMap::new()),
		rename: RefCell::new(BTreeMap::new()),
		cache,
	};
	tokio::task::block_in_place(|| {
		tokio::runtime::Handle::current().block_on(traverse_dir(&mut state, dir))
	})?;

	protocol_println!(".");
	Ok(state)
}

#[allow(dead_code)]
async fn serve_read(dir: path::PathBuf, dump_state: &DumpState) -> Result<(), Box<dyn Error>> {
	let mut chunks: Vec<String> = Vec::new();
	let mut buf = String::new();
	loop {
		buf.clear();
		io::stdin()
			.read_line(&mut buf)
			.map_err(|e| format!("Failed to read chunk hashes: {}", e))?;
		if buf.trim() == "." {
			break;
		}
		chunks.push(String::from(buf.trim()));
	}

	for chunk in &chunks {
		let &fc_vec_opt = &dump_state.chunks.get(chunk);

		if let Some(fc_vec) = &fc_vec_opt {
			let fc = &fc_vec[0];
			let path = dir.join(&fc.path);
			let mut f = afs::File::open(&path).await?;
			let mut buf: Vec<u8> = vec![0; fc.size];
			f.seek(io::SeekFrom::Start(fc.offset)).await?;
			f.read_exact(&mut buf).await?;
			let encoded = general_purpose::URL_SAFE.encode(buf);
			protocol_println!("C:{}", chunk);
			for line in encoded.into_bytes().chunks(config::BASE64_LINE_LENGTH) {
				write_stdout(line)?;
				write_stdout(b"\n")?;
			}
			protocol_println!(".");
		}
	}
	protocol_println!(".");
	Ok(())
}

#[allow(dead_code)]
async fn serve_write(dir: path::PathBuf, dump_state: &DumpState) -> Result<(), Box<dyn Error>> {
	let mut buf = String::new();

	let mut file: Option<afs::File> = None;
	let mut filepath = path::PathBuf::from("");
	loop {
		buf.clear();
		io::stdin().read_line(&mut buf)?;

		let fields = protocol_utils::parse_protocol_line(&buf, 1)?;
		let cmd = fields[0];

		match cmd {
			"DEL" => {
				// Delete command: remove file from node
				let fields = protocol_utils::parse_protocol_line(&buf, 2)?;
				let path = path::PathBuf::from(fields[1]);

				// Validate path to prevent directory traversal attacks
				validate_path(&path)?;

				debug!("DELETE: {:?}", &path);

				// Remove the file
				if afs::metadata(&path).await.is_ok() {
					if afs::metadata(&path).await?.is_file() {
						afs::remove_file(&path).await?;
					} else if afs::metadata(&path).await?.is_dir() {
						afs::remove_dir_all(&path).await?;
					}
				}
			}
			"L" => {
				// Symlink command: create a symlink
				let fd = metadata_utils::parse_symlink_metadata(&buf)?;
				let path = fd.path.clone();
				validate_path(&path)?;

				// Remove existing file if it exists
				if afs::metadata(&path).await.is_ok() {
					afs::remove_file(&path).await.ok();
				}

				// Create the symlink with the target
				if let Some(target) = &fd.target {
					if let Err(e) = afs::symlink(target, &path).await {
						error!("Failed to create symlink {:?} -> {:?}: {}", path, target, e);
					}
				} else {
					warn!("Symlink {:?} has no target", path);
				}
			}
			"FM" | "FD" => {
				let fd = metadata_utils::parse_file_metadata(&buf)?;
				let path = fd.path.clone();

				// Validate path to prevent directory traversal attacks
				validate_path(&path)?;
				if cmd == "FD" {
					filepath = path.clone();
					let filename = path.file_name().ok_or("Path has no filename")?.to_os_string();
					let mut tmp_filename = filename;
					tmp_filename.push(".SyNcR-TmP");
					filepath.set_file_name(tmp_filename);
					//eprintln!("CREATE {:?}", &filepath);
					file = Some(afs::File::create(&filepath).await?);
					afs::set_permissions(&filepath, std::fs::Permissions::from_mode(fd.mode))
						.await?;
					dump_state.rename.borrow_mut().insert(filepath.clone(), path.clone());
				}
			}
			"D" => {
				let fd = metadata_utils::parse_dir_metadata(&buf)?;
				let path = fd.path.clone();
				filepath = path.clone();
				let filename = path.file_name().ok_or("Path has no filename")?;
				// FIXME: Should create temp dir, but then all paths must be altered!
				//filename.push(".SyNcR-TmP");
				filepath.set_file_name(filename);
				debug!("MKDIR {:?}", &filepath);
				// Try to create directory, but ignore AlreadyExists error
				match afs::create_dir(&filepath).await {
					Ok(_) => {}
					Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
						debug!("Directory already exists: {:?}", &filepath);
					}
					Err(e) => return Err(e.into()),
				}
				afs::set_permissions(&filepath, std::fs::Permissions::from_mode(fd.mode)).await?;
				dump_state.rename.borrow_mut().insert(filepath.clone(), path.clone());
			}
			"LC" | "RC" => {
				let fields = protocol_utils::parse_protocol_line(&buf, 4)?;
				if file.is_none() {
					return Err("Protocol error: chunk command without file".into());
				}
				let hash = crate::util::base64_to_hash(fields[3])
					.map_err(|e| format!("Invalid hash: {}", e))?;
				let hc = HashChunk {
					hash,
					offset: fields[1]
						.parse()
						.map_err(|e| format!("Invalid offset '{}': {}", fields[1], e))?,
					size: fields[2]
						.parse::<u32>()
						.map_err(|e| format!("Invalid size '{}': {}", fields[2], e))?,
				};
				if cmd == "LC" {
					// Local chunk, copy it locally
					let buf = dump_state
						.read_chunk(&dir, fields[3])
						.await?
						.ok_or_else(|| format!("Chunk not found: {}", fields[3]))?;
					if let Err(e) = dump_state.write_chunk(&filepath, &hc, &buf).await {
						protocol_println!("ERROR {}", e);
					}
				} else {
					// Remote chunk, add to wait list
					let mut missing = dump_state.missing.borrow_mut();
					let v = missing.entry(String::from(fields[3])).or_default();
					v.push(FileChunk {
						path: filepath.clone(),
						offset: fields[1]
							.parse()
							.map_err(|e| format!("Invalid offset '{}': {}", fields[1], e))?,
						size: fields[2]
							.parse()
							.map_err(|e| format!("Invalid size '{}': {}", fields[2], e))?,
					});
				}
			}
			"C" => {
				let fields = protocol_utils::parse_protocol_line(&buf, 2)?;
				let mut buf = String::new();
				let hash_b64 = fields[1];
				let hash = crate::util::base64_to_hash(hash_b64)
					.map_err(|e| format!("Invalid hash: {}", e))?;
				let mut chunk: Vec<u8> = Vec::new();
				loop {
					buf.clear();
					io::stdin().read_line(&mut buf)?;
					if buf.trim() == "." {
						break;
					}
					//eprintln!("DECODE: [{:?}]", &buf.trim());
					chunk.append(&mut general_purpose::URL_SAFE.decode(buf.trim())?);
				}
				//eprintln!("DECODED CHUNK: {:?}", chunk);
				let fc_vec_opt = {
					let missing = dump_state.missing.borrow();
					missing.get(hash_b64).cloned()
				};
				if let Some(fc_vec) = fc_vec_opt {
					for fc in fc_vec {
						let hc = HashChunk { hash, offset: fc.offset, size: fc.size as u32 };
						//let filepath = tmp_filename(&fc.path);
						if let Err(e) = dump_state.write_chunk(&fc.path, &hc, &chunk).await {
							error!("ERROR WRITING {}", e);
						}
					}
					dump_state.missing.borrow_mut().remove(hash_b64);
				}
			}
			"." => {
				if file.is_some() {
					file = None;
				} else {
					break;
				}
			}
			_ => return Err(format!("Unknown command: {}", cmd).into()),
		}
	}
	protocol_println!("OK");
	Ok(())
}

#[allow(dead_code)]
async fn serve_commit(
	_fixme_dir: path::PathBuf,
	dump_state: &DumpState,
) -> Result<(), Box<dyn Error>> {
	{
		let missing = dump_state.missing.borrow();
		if !missing.is_empty() {
			let missing_hashes: Vec<&String> = missing.keys().collect();
			error!("Cannot commit - {} missing chunks", missing.len());
			for hash in &missing_hashes {
				error!("  Missing chunk: {}", hash);
			}
			protocol_println!("ERROR:Cannot commit with {} missing chunks", missing.len());
			return Err(format!("Cannot commit: {} chunks still missing", missing.len()).into());
		}
	}

	let renames_to_do: Vec<(path::PathBuf, path::PathBuf)> = {
		let rename = dump_state.rename.borrow();
		rename.iter().map(|(src, dst)| (src.clone(), dst.clone())).collect()
	};

	for (src, dst) in renames_to_do {
		//eprintln!("RENAME: {:?} -> {:?}", src, dst);
		afs::rename(&src, &dst).await?;
		//fs::rename(&src, &dst)?;
	}
	protocol_println!("OK");
	Ok(())
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

pub fn serve(dir: &str) -> Result<(), Box<dyn Error>> {
	env::set_current_dir(dir)?;

	// Clean up orphaned temp files from any interrupted previous syncs
	cleanup_temp_files(path::Path::new("."))?;

	// Signal that we're ready to receive commands (do NOT send VERSION here,
	// wait for the client to initiate the handshake with VERSION)
	use std::io::Write;
	writeln!(std::io::stdout(), ".")?;
	std::io::stdout().flush()?;

	// Perform version negotiation (V3 only)
	negotiate_version()?;
	debug!("Negotiated protocol version: V3");

	let mut dump_state: Option<DumpState> = None;

	loop {
		let mut cmdline = String::new();
		match io::stdin().read_line(&mut cmdline) {
			Ok(0) => break, // EOF reached
			Ok(_) => {}
			Err(e) => {
				error!("Failed to read command: {}", e);
				break;
			}
		}

		// Handle v3 JSON5 protocol
		match handle_v3_command(&cmdline, &mut dump_state) {
			Ok(()) => {}
			Err(e) if e.to_string() == "QUIT" => break, // Graceful shutdown on QUIT
			Err(e) => return Err(e),
		}
	}
	Ok(())
}

/// Negotiate protocol version with parent
/// Only accepts V3 (JSON5) protocol
fn negotiate_version() -> Result<i32, Box<dyn Error>> {
	let mut line = String::new();
	io::stdin().read_line(&mut line)?;

	let trimmed = line.trim();

	// Try parsing as v3 (JSON5)
	if let Ok(cmd) = json5::from_str::<VersionCommand>(trimmed) {
		if cmd.cmd == "VER" && cmd.ver == PROTOCOL_VERSION_V3 {
			// Respond with v3 format
			let response = VersionResponse { cmd: "VER".to_string(), ver: PROTOCOL_VERSION_V3 };
			let response_json = serde_json::to_string(&response)?;
			protocol_println!("{}", response_json);
			return Ok(3);
		}
	}

	// Unknown format - send v3 error
	let error = ErrorResponse {
		cmd: "ERR".to_string(),
		msg: "Expected {cmd:\"VER\",ver:3} in JSON5 format".to_string(),
	};
	let error_json = serde_json::to_string(&error)?;
	protocol_println!("{}", error_json);

	Err("Unrecognized handshake format".into())
}

/// Handle v3 (JSON5) protocol commands
fn handle_v3_command(
	cmdline: &str,
	dump_state: &mut Option<DumpState>,
) -> Result<(), Box<dyn Error>> {
	let trimmed = cmdline.trim();

	// Skip empty lines
	if trimmed.is_empty() {
		return Ok(());
	}

	// Try parsing as JSON5
	match json5::from_str::<serde_json::Value>(trimmed) {
		Ok(json_obj) => {
			if let Some(cmd) = json_obj.get("cmd").and_then(|v| v.as_str()) {
				match cmd {
					"LIST" => {
						// Serve v3 LIST command (JSON5 format)
						let new_state = tokio::task::block_in_place(|| {
							tokio::runtime::Handle::current()
								.block_on(serve_list_v3(path::PathBuf::from(".")))
						})?;
						dump_state.replace(new_state);
					}
					"READ" => {
						// Serve v3 READ command (binary chunk transfer)
						if let Some(state) = dump_state {
							tokio::task::block_in_place(|| {
								tokio::runtime::Handle::current()
									.block_on(serve_read_v3(path::PathBuf::from("."), state))
							})?;
						} else {
							let error = ErrorResponse {
								cmd: "ERR".to_string(),
								msg: "Use LIST command first".to_string(),
							};
							if let Ok(json_str) = serde_json::to_string(&error) {
								protocol_println!("{}", json_str);
							}
						}
					}
					"WRITE" => {
						// Serve v3 WRITE command (receive metadata and binary chunks)
						if let Some(state) = dump_state {
							tokio::task::block_in_place(|| {
								tokio::runtime::Handle::current()
									.block_on(serve_write_v3(path::PathBuf::from("."), state))
							})?;
						} else {
							let error = ErrorResponse {
								cmd: "ERR".to_string(),
								msg: "Use LIST command first".to_string(),
							};
							if let Ok(json_str) = serde_json::to_string(&error) {
								protocol_println!("{}", json_str);
							}
						}
					}
					"COMMIT" => {
						// Serve v3 COMMIT command (rename temp files to final locations)
						if let Some(state) = dump_state {
							tokio::task::block_in_place(|| {
								tokio::runtime::Handle::current()
									.block_on(serve_commit_v3(path::PathBuf::from("."), state))
							})?;
						} else {
							let error = ErrorResponse {
								cmd: "ERR".to_string(),
								msg: "Use LIST and WRITE commands first".to_string(),
							};
							if let Ok(json_str) = serde_json::to_string(&error) {
								protocol_println!("{}", json_str);
							}
						}
					}
					"CAN" => {
						// Serve v3 CANCEL command (clean up temp files and abort operation)
						if let Some(state) = dump_state {
							tokio::task::block_in_place(|| {
								tokio::runtime::Handle::current()
									.block_on(serve_cancel_v3(path::PathBuf::from("."), state))
							})?;
						} else {
							// CANCEL without active WRITE is not an error - just send OK response
							protocol_println!("{{\"cmd\":\"OK\",\"cleaned\":0,\"failed\":0}}");
						}
					}
					"QUIT" => {
						protocol_println!("{{\"cmd\":\"OK\"}}");
						return Err("QUIT".into());
					}
					_ => {
						protocol_println!(
							"{{\"cmd\":\"ERR\",\"msg\":\"Unknown command: {}\"}}",
							cmd
						);
					}
				}
			}
		}
		Err(e) => {
			// Log JSON5 parse error for debugging, but don't send error response
			// to avoid confusing the parent with unexpected protocol output
			debug!("JSON5 parse error: {} (input: {})", e, trimmed);
		}
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_validate_path_allows_relative_paths() {
		assert!(validate_path(path::Path::new("file.txt")).is_ok());
		assert!(validate_path(path::Path::new("dir/file.txt")).is_ok());
		assert!(validate_path(path::Path::new("a/b/c/d.txt")).is_ok());
	}

	#[test]
	fn test_validate_path_rejects_absolute_paths() {
		assert!(validate_path(path::Path::new("/etc/passwd")).is_err());
		assert!(validate_path(path::Path::new("/home/user/file.txt")).is_err());
		assert!(validate_path(path::Path::new("/")).is_err());
	}

	#[test]
	fn test_validate_path_rejects_parent_traversal() {
		assert!(validate_path(path::Path::new("../etc/passwd")).is_err());
		assert!(validate_path(path::Path::new("file/../../../etc/passwd")).is_err());
		assert!(validate_path(path::Path::new("..")).is_err());
		assert!(validate_path(path::Path::new("dir/..")).is_err());
	}

	#[test]
	fn test_validate_path_rejects_current_dir() {
		assert!(validate_path(path::Path::new(".")).is_err());
	}

	#[test]
	fn test_validate_path_rejects_empty_paths() {
		assert!(validate_path(path::Path::new("")).is_err());
	}
}

// vim: ts=4
