//! Shared filesystem operations for protocol implementations
//!
//! This module contains the core business logic for file operations that
//! is shared between both the internal (in-process) and V3 (JSON5/IPC) protocol
//! implementations. This avoids duplicating the same logic across two code paths.

use rollsum::Bup;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs as afs;
use tokio::io::AsyncReadExt;
use tokio::sync::Semaphore;
use tracing::{debug, error, warn};

use super::error::ProtocolError;
use super::streaming::{create_listing_channel, ListingStream, StreamingConfig};
use super::types::*;
use crate::chunking;
use crate::serve::DumpState;
use crate::util;

/// Core filesystem operations shared by all server implementations
pub struct FileSystemServer {
	pub base_path: PathBuf,
	pub state: DumpState,
}

impl FileSystemServer {
	/// Create a new FileSystemServer for the given base path
	pub fn new(base_path: PathBuf, state: DumpState) -> Self {
		Self { base_path, state }
	}

	/// List all files/directories recursively
	///
	/// This is called by the LIST command to enumerate the directory tree.
	/// Recursively walks the directory and returns FileSystemEntry objects.
	pub async fn list_directory(&mut self) -> Result<Vec<FileSystemEntry>, ProtocolError> {
		let mut entries = Vec::new();
		let base = self.base_path.clone();
		self.traverse_dir_for_listing_impl(&base, &mut entries).await?;
		Ok(entries)
	}

	/// List all files/directories recursively via streaming
	///
	/// This method returns a channel that yields entries as they're discovered.
	/// This enables:
	/// - Lower initial latency (first entry arrives sooner)
	/// - Lower memory usage (no buffering all entries)
	/// - Concurrency control via semaphore (Phase 2+)
	pub fn list_directory_streaming(&self) -> Result<ListingStream, ProtocolError> {
		// Create channel with default buffer size
		let config = StreamingConfig::default();
		let (sender, receiver) = create_listing_channel(&config);

		// Clone state for the task
		let state = self.state.clone();
		let base_path = self.base_path.clone();
		let exclude_patterns = self.state.exclude.clone();

		// Create semaphore to limit concurrent file operations
		// This prevents "too many open files" errors by limiting concurrent chunk computations
		let semaphore = Arc::new(Semaphore::new(config.max_concurrent_files));

		// Spawn task to traverse directory and send entries
		// IMPORTANT: This must run to completion before returning so all chunks are in DumpState
		// The sender will block when buffer is full, providing backpressure
		tokio::spawn(async move {
			debug!(
				"Starting directory traversal task with semaphore limit: {}",
				config.max_concurrent_files
			);
			if let Err(e) =
				traverse_and_stream(base_path, state, exclude_patterns, sender, semaphore).await
			{
				error!("Directory traversal failed: {}", e);
			}
			debug!("Directory traversal task completed");
		});

		Ok(receiver)
	}

	/// Helper for recursive directory traversal (helper that does the work)
	fn traverse_dir_for_listing_impl<'a>(
		&'a mut self,
		dir: &'a Path,
		entries: &'a mut Vec<FileSystemEntry>,
	) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ProtocolError>> + 'a>> {
		Box::pin(async move {
			let read_result = fs::read_dir(dir);
			let dir_entries = match read_result {
				Ok(e) => e,
				Err(e) => {
					warn!("Cannot read directory {}: {}", dir.display(), e);
					return Ok(());
				}
			};

			for entry_result in dir_entries {
				let entry = match entry_result {
					Ok(e) => e,
					Err(e) => {
						debug!("Error reading directory entry: {}", e);
						continue;
					}
				};

				let path = entry.path();

				// Skip excluded files
				if self.state.exclude[0].matches_path(&path) {
					continue;
				}

				let meta = match fs::symlink_metadata(&path) {
					Ok(m) => m,
					Err(e) => {
						warn!("Cannot access {}: {}", path.display(), e);
						continue;
					}
				};

				let relative_path =
					path.strip_prefix(&self.base_path).unwrap_or(&path).to_path_buf();

				if meta.is_file() {
					// Process file and collect its chunks
					let chunks = self.get_file_chunks(&path).await?;

					entries.push(FileSystemEntry {
						entry_type: FileSystemEntryType::File,
						path: relative_path,
						mode: meta.mode(),
						user_id: meta.uid(),
						group_id: meta.gid(),
						created_time: meta.ctime() as u32,
						modified_time: meta.mtime() as u32,
						size: meta.size(),
						target: None,
						needs_data_transfer: None,
						chunks,
					});
				} else if meta.is_symlink() {
					// Process symlink
					let target = fs::read_link(&path).ok();

					entries.push(FileSystemEntry {
						entry_type: FileSystemEntryType::SymLink,
						path: relative_path,
						mode: meta.mode(),
						user_id: meta.uid(),
						group_id: meta.gid(),
						created_time: meta.ctime() as u32,
						modified_time: meta.mtime() as u32,
						size: 0,
						target,
						chunks: Vec::new(),
						needs_data_transfer: None,
					});
				} else if meta.is_dir() {
					// Add directory entry
					entries.push(FileSystemEntry {
						entry_type: FileSystemEntryType::Directory,
						path: relative_path,
						mode: meta.mode(),
						user_id: meta.uid(),
						group_id: meta.gid(),
						created_time: meta.ctime() as u32,
						modified_time: meta.mtime() as u32,
						size: 0,
						target: None,
						needs_data_transfer: None,
						chunks: Vec::new(),
					});

					// Recursively process subdirectory
					self.traverse_dir_for_listing_impl(&path, entries).await?;
				}
			}

			Ok(())
		})
	}

	/// Calculate chunks for a file using rolling hash
	async fn get_file_chunks(&mut self, path: &Path) -> Result<Vec<ChunkInfo>, ProtocolError> {
		let mut chunks = Vec::new();

		let mut f = match afs::File::open(&path).await {
			Ok(file) => file,
			Err(e) => {
				warn!("Cannot open file {}: {}", path.display(), e);
				return Ok(Vec::new());
			}
		};

		let mut buf: Vec<u8> = vec![0; chunking::MAX_CHUNK_SIZE];
		let mut n = match f.read(&mut buf).await {
			Ok(bytes) => bytes,
			Err(e) => {
				warn!("Cannot read file {}: {}", path.display(), e);
				return Ok(Vec::new());
			}
		};

		let mut offset: u64 = 0;
		while n > 0 {
			let mut bup = Bup::new_with_chunk_bits(chunking::CHUNK_BITS);
			let mut endofs = chunking::MAX_CHUNK_SIZE;
			if endofs > n {
				endofs = n;
			}

			let count = if let Some((edge, _)) = bup.find_chunk_edge(&buf[..endofs]) {
				edge
			} else {
				endofs
			};

			let hash_binary = util::hash_binary(&buf[..count]);
			chunks.push(ChunkInfo { hash: hash_binary, offset, size: count as u32 });

			// Track chunk in state for later retrieval (thread-safe)
			let hash_b64 = util::hash_to_base64(&hash_binary);
			self.state.add_chunk(hash_b64, path.to_path_buf(), offset, count).await;

			// Shift remaining data to front
			buf.copy_within(count..n, 0);
			offset += count as u64;
			n -= count;

			// Read next chunk of file
			let read_result = f.read(&mut buf[n..]).await;
			match read_result {
				Ok(bytes) => n += bytes,
				Err(e) => {
					warn!("Error reading file {}: {}", path.display(), e);
					break;
				}
			}
		}

		Ok(chunks)
	}

	/// Read chunks from disk
	///
	/// Returns the requested chunks from the local filesystem.
	pub async fn read_chunks(&self, hashes: &[String]) -> Result<Vec<ChunkData>, ProtocolError> {
		debug!("[fs_server] read_chunks called with {} hashes", hashes.len());
		let mut chunks = Vec::new();
		let mut missing_hashes = Vec::new();

		for (idx, hash) in hashes.iter().enumerate() {
			debug!("[fs_server] Processing chunk {}/{}: {}", idx + 1, hashes.len(), hash);
			match self.state.read_chunk(&self.base_path, hash).await {
				Ok(Some(data)) => {
					let data_len = data.len();
					debug!("[fs_server] Successfully read chunk {}: {} bytes", hash, data_len);
					chunks.push(ChunkData { hash: hash.clone(), data });
				}
				Ok(None) => {
					// Chunk hash not in DumpState (not found during LIST)
					warn!(
						"[fs_server] Chunk {} requested but not in DumpState (LIST phase issue)",
						hash
					);
					missing_hashes.push(hash.clone());
				}
				Err(e) => {
					// Error reading chunk from disk
					error!(
						"[fs_server] Error reading chunk {}: {} (I/O or all file copies failed)",
						hash, e
					);
					missing_hashes.push(hash.clone());
				}
			}
		}

		debug!(
			"[fs_server] read_chunks summary: {}/{} chunks successfully read",
			chunks.len(),
			hashes.len()
		);

		if !missing_hashes.is_empty() {
			error!(
				"[fs_server] PROBLEM: Failed to read {} out of {} requested chunks",
				missing_hashes.len(),
				hashes.len()
			);
			for hash in &missing_hashes {
				error!("[fs_server]   - Missing: {}", hash);
			}
		} else {
			debug!("[fs_server] All {} chunks successfully read", hashes.len());
		}

		Ok(chunks)
	}

	/// Write metadata (create file/dir/symlink)
	///
	/// Creates a new file, directory, or symlink on disk based on the metadata.
	pub async fn write_metadata(&mut self, entry: &MetadataEntry) -> Result<(), ProtocolError> {
		use std::os::unix::fs::PermissionsExt;

		let full_path = self.base_path.join(&entry.path);

		match entry.entry_type {
			FileSystemEntryType::File => {
				debug!(
					"[fs_server] write_metadata for file: {} (chunks: {})",
					entry.path.display(),
					entry.chunks.len()
				);
				// Validate path for security
				if !self.validate_path_safe(&entry.path) {
					return Err(ProtocolError::Other(
						"Invalid file path (contains ..)".to_string(),
					));
				}

				// Create parent directories if needed
				if let Some(parent) = full_path.parent() {
					afs::create_dir_all(parent).await.ok();
				}

				// Create temporary file with .SyNcR-TmP suffix
				let filename = entry.path.file_name().ok_or("Path has no filename")?.to_os_string();
				let mut tmp_name = filename;
				tmp_name.push(".SyNcR-TmP");

				let tmp_path = self
					.base_path
					.join(entry.path.parent().unwrap_or(Path::new("")))
					.join(&tmp_name);

				// Create the file
				let _file = afs::File::create(&tmp_path)
					.await
					.map_err(|e| ProtocolError::Other(format!("Failed to create file: {}", e)))?;

				// Set permissions
				afs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(entry.mode))
					.await
					.ok();

				// Track for later rename during commit
				self.state.rename.lock().await.insert(tmp_path.clone(), full_path);

				// Track where each chunk should be written for this file
				{
					let mut chunk_writes = self.state.chunk_writes.lock().await;
					for chunk in &entry.chunks {
						let hash_b64 = util::hash_to_base64(&chunk.hash);
						chunk_writes.insert(hash_b64, (tmp_path.clone(), chunk.offset));
					}
				}
			}
			FileSystemEntryType::Directory => {
				// Validate path
				if !self.validate_path_safe(&entry.path) {
					return Err(ProtocolError::Other(
						"Invalid directory path (contains ..)".to_string(),
					));
				}

				// Create directory
				match afs::create_dir(&full_path).await {
					Ok(_) => {}
					Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
					Err(e) => {
						return Err(ProtocolError::Other(format!(
							"Failed to create directory: {}",
							e
						)))
					}
				}

				// Set permissions
				afs::set_permissions(&full_path, std::fs::Permissions::from_mode(entry.mode))
					.await
					.ok();
			}
			FileSystemEntryType::SymLink => {
				// Validate path
				if !self.validate_path_safe(&entry.path) {
					return Err(ProtocolError::Other(
						"Invalid symlink path (contains ..)".to_string(),
					));
				}

				// Get target path
				let target = entry.target.as_ref().ok_or("Symlink has no target")?;

				// Remove existing symlink if present
				afs::remove_file(&full_path).await.ok();

				// Create symlink
				afs::symlink(target, &full_path).await.map_err(|e| {
					ProtocolError::Other(format!("Failed to create symlink: {}", e))
				})?;
			}
		}

		Ok(())
	}

	/// Validate that a path is safe to use (no directory traversal)
	fn validate_path_safe(&self, path: &Path) -> bool {
		for component in path.components() {
			if component == std::path::Component::ParentDir {
				return false;
			}
		}
		true
	}

	/// Delete a file or directory
	pub async fn delete_file(&mut self, path: &Path) -> Result<(), ProtocolError> {
		let full_path = self.base_path.join(path);

		// Try to remove as file first
		if afs::remove_file(&full_path).await.is_ok() {
			return Ok(());
		}

		// Try to remove as directory
		if afs::remove_dir(&full_path).await.is_ok() {
			return Ok(());
		}

		// If both fail, return an error
		Err(ProtocolError::Other(format!(
			"Could not delete {}: file or directory not found",
			path.display()
		)))
	}

	/// Write chunk data to a file
	///
	/// Chunks can be written either to files we're creating (from MetadataEntry chunks)
	/// or to existing local files (for deduplication). This is called during chunk transfer.
	pub async fn write_chunk(&mut self, hash: &str, data: &[u8]) -> Result<(), ProtocolError> {
		// Verify hash before writing to detect corruption
		let computed_hash = util::hash_binary(data);
		let computed_b64 = util::hash_to_base64(&computed_hash);

		if computed_b64 != hash {
			return Err(ProtocolError::Other(format!(
				"Hash mismatch: expected {}, got {}",
				hash, computed_b64
			)));
		}

		// Look up where this chunk should be written
		{
			let chunk_writes = self.state.chunk_writes.lock().await;
			if let Some((tmp_path, offset)) = chunk_writes.get(hash) {
				debug!("[fs_server] Writing chunk {} to {:?} at offset {}", hash, tmp_path, offset);
				// Clone the data we need before releasing the lock
				let tmp_path = tmp_path.clone();
				let offset = *offset;
				drop(chunk_writes);

				// Open the temporary file for writing
				use tokio::io::AsyncWriteExt;
				let mut file =
					afs::OpenOptions::new().write(true).open(&tmp_path).await.map_err(|e| {
						ProtocolError::Other(format!("Failed to open temp file for writing: {}", e))
					})?;

				// Seek to the correct offset
				use tokio::io::AsyncSeekExt;
				file.seek(std::io::SeekFrom::Start(offset)).await.map_err(|e| {
					ProtocolError::Other(format!("Failed to seek in temp file: {}", e))
				})?;

				// Write the chunk data
				file.write_all(data).await.map_err(|e| {
					ProtocolError::Other(format!("Failed to write chunk data: {}", e))
				})?;

				debug!("[fs_server] Successfully wrote {} bytes for chunk {}", data.len(), hash);
				return Ok(());
			} else {
				debug!("[fs_server] Chunk {} not in chunk_writes map (not an error, might be deduplication)", hash);
			}
		}

		// Chunk not in write map - this is not an error, might be deduplication
		// Just ignore it for now
		Ok(())
	}
	/// Commit all pending changes
	///
	/// Renames temporary files to their final locations and returns the result.
	pub async fn commit(&mut self) -> Result<CommitResponse, ProtocolError> {
		let mut renamed_count = 0;
		let mut failed_count = 0;

		// Get all pending renames
		let renames: Vec<_> = self
			.state
			.rename
			.lock()
			.await
			.iter()
			.map(|(k, v)| (k.clone(), v.clone()))
			.collect();

		// Execute renames
		for (tmp_path, final_path) in renames {
			match afs::rename(&tmp_path, &final_path).await {
				Ok(_) => renamed_count += 1,
				Err(_) => failed_count += 1,
			}
		}

		// Clear the rename map after commit
		self.state.rename.lock().await.clear();

		Ok(CommitResponse {
			success: failed_count == 0,
			message: None,
			renamed_count: Some(renamed_count),
			failed_count: Some(failed_count),
		})
	}
}

/// Traverses directory tree and streams entries through a channel
///
/// This function is spawned as a separate task by list_directory_streaming.
/// It walks the directory, computes chunks for each file, and sends entries
/// to the caller as they're discovered. When the channel is closed (receiver
/// dropped), the task gracefully terminates.
async fn traverse_and_stream(
	base_path: PathBuf,
	state: DumpState,
	exclude_patterns: Vec<glob::Pattern>,
	sender: super::streaming::ListingSender,
	semaphore: Arc<Semaphore>,
) -> Result<(), ProtocolError> {
	let mut stack = vec![base_path.clone()];

	while let Some(dir) = stack.pop() {
		let read_result = fs::read_dir(&dir);
		let dir_entries = match read_result {
			Ok(e) => e,
			Err(e) => {
				warn!("Cannot read directory {}: {}", dir.display(), e);
				continue;
			}
		};

		for entry_result in dir_entries {
			let entry = match entry_result {
				Ok(e) => e,
				Err(e) => {
					debug!("Error reading directory entry: {}", e);
					continue;
				}
			};

			let path = entry.path();

			// Skip excluded files
			if exclude_patterns.iter().any(|p| p.matches_path(&path)) {
				continue;
			}

			let meta = match fs::symlink_metadata(&path) {
				Ok(m) => m,
				Err(e) => {
					warn!("Cannot access {}: {}", path.display(), e);
					continue;
				}
			};

			let relative_path = path.strip_prefix(&base_path).unwrap_or(&path).to_path_buf();

			// Prepare entry based on type
			let entry_result = if meta.is_file() {
				// Acquire semaphore permit BEFORE computing chunks
				// This prevents "too many open files" errors by limiting concurrent file operations
				let file_path = path.clone();
				let state_clone = state.clone();

				// Acquire permit - this will block if we've hit the concurrency limit
				// This provides natural backpressure: if we're maxed out on concurrent files,
				// directory traversal pauses until a file is done processing
				match semaphore.acquire().await {
					Ok(permit) => {
						// Compute chunks while holding the permit
						match compute_file_chunks(&file_path, &state_clone).await {
							Ok(chunks) => {
								// Permit will be released when it goes out of scope
								drop(permit);

								// Send entry with chunks already computed
								Ok(FileSystemEntry {
									entry_type: FileSystemEntryType::File,
									path: relative_path,
									mode: meta.mode(),
									user_id: meta.uid(),
									group_id: meta.gid(),
									created_time: meta.ctime() as u32,
									modified_time: meta.mtime() as u32,
									size: meta.size(),
									target: None,
									needs_data_transfer: None,
									chunks,
								})
							}
							Err(e) => {
								drop(permit);
								warn!(
									"Failed to compute chunks for {}: {}",
									file_path.display(),
									e
								);
								// Send entry with empty chunks on error
								Ok(FileSystemEntry {
									entry_type: FileSystemEntryType::File,
									path: relative_path,
									mode: meta.mode(),
									user_id: meta.uid(),
									group_id: meta.gid(),
									created_time: meta.ctime() as u32,
									modified_time: meta.mtime() as u32,
									size: meta.size(),
									target: None,
									needs_data_transfer: None,
									chunks: Vec::new(),
								})
							}
						}
					}
					Err(e) => {
						error!("Failed to acquire semaphore permit: {}", e);
						// Send entry with empty chunks if we can't get permit
						Ok(FileSystemEntry {
							entry_type: FileSystemEntryType::File,
							path: relative_path,
							mode: meta.mode(),
							user_id: meta.uid(),
							group_id: meta.gid(),
							created_time: meta.ctime() as u32,
							modified_time: meta.mtime() as u32,
							size: meta.size(),
							target: None,
							needs_data_transfer: None,
							chunks: Vec::new(),
						})
					}
				}
			} else if meta.is_symlink() {
				// Process symlink
				let target = fs::read_link(&path).ok();

				Ok(FileSystemEntry {
					entry_type: FileSystemEntryType::SymLink,
					path: relative_path,
					mode: meta.mode(),
					user_id: meta.uid(),
					group_id: meta.gid(),
					created_time: meta.ctime() as u32,
					modified_time: meta.mtime() as u32,
					size: 0,
					target,
					chunks: Vec::new(),
					needs_data_transfer: None,
				})
			} else if meta.is_dir() {
				// Add directory entry
				let fse = FileSystemEntry {
					entry_type: FileSystemEntryType::Directory,
					path: relative_path,
					mode: meta.mode(),
					user_id: meta.uid(),
					group_id: meta.gid(),
					created_time: meta.ctime() as u32,
					modified_time: meta.mtime() as u32,
					size: 0,
					target: None,
					needs_data_transfer: None,
					chunks: Vec::new(),
				};

				// Push to stack for recursive processing
				stack.push(path);

				Ok(fse)
			} else {
				continue;
			};

			// Send entry through channel
			// If receiver is dropped, exit gracefully
			if sender.send(entry_result).await.is_err() {
				debug!("Receiver dropped, terminating directory stream");
				return Ok(());
			}
		}
	}

	Ok(())
}

/// Compute chunks for a file using rolling hash
///
/// This is extracted from get_file_chunks() to be reusable by both
/// the blocking and streaming paths.
async fn compute_file_chunks(
	path: &Path,
	state: &DumpState,
) -> Result<Vec<ChunkInfo>, ProtocolError> {
	let mut chunks = Vec::new();

	let mut f = match afs::File::open(&path).await {
		Ok(file) => file,
		Err(e) => {
			warn!("Cannot open file {}: {}", path.display(), e);
			return Ok(Vec::new());
		}
	};

	debug!("[compute_file_chunks] Processing: {}", path.display());

	let mut buf: Vec<u8> = vec![0; chunking::MAX_CHUNK_SIZE];
	let mut n = match f.read(&mut buf).await {
		Ok(bytes) => bytes,
		Err(e) => {
			warn!("Cannot read file {}: {}", path.display(), e);
			return Ok(Vec::new());
		}
	};

	let mut offset: u64 = 0;
	while n > 0 {
		let mut bup = Bup::new_with_chunk_bits(chunking::CHUNK_BITS);
		let mut endofs = chunking::MAX_CHUNK_SIZE;
		if endofs > n {
			endofs = n;
		}

		let count =
			if let Some((edge, _)) = bup.find_chunk_edge(&buf[..endofs]) { edge } else { endofs };

		let hash_binary = util::hash_binary(&buf[..count]);
		chunks.push(ChunkInfo { hash: hash_binary, offset, size: count as u32 });

		// Track chunk in state for later retrieval (thread-safe)
		let hash_b64 = util::hash_to_base64(&hash_binary);
		state.add_chunk(hash_b64.clone(), path.to_path_buf(), offset, count).await;
		debug!(
			"[compute_file_chunks] Added chunk: {} ({}B @ offset {})",
			&hash_b64[..8],
			count,
			offset
		);

		// Shift remaining data to front
		buf.copy_within(count..n, 0);
		offset += count as u64;
		n -= count;

		// Read next chunk of file
		let read_result = f.read(&mut buf[n..]).await;
		match read_result {
			Ok(bytes) => n += bytes,
			Err(e) => {
				warn!("Error reading file {}: {}", path.display(), e);
				break;
			}
		}
	}

	debug!("[compute_file_chunks] Completed {}: {} chunks", path.display(), chunks.len());
	Ok(chunks)
}

// vim: ts=4
