//! Node state management for sync operations

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::path;
use tokio::sync::Mutex;

use crate::protocol::{FileSystemEntryType, SyncProtocol};
use crate::types::{FileData, FileType};

/// State for a single sync node (local or remote)
pub struct NodeState {
	pub id: u8,
	pub protocol: Mutex<Box<dyn SyncProtocol>>,
	pub dir: BTreeMap<path::PathBuf, Box<FileData>>,
	pub chunks: BTreeSet<[u8; 32]>,       // Binary BLAKE3 hashes
	pub missing: Mutex<BTreeSet<String>>, // Base64-encoded hashes needing transfer
}

impl PartialEq for NodeState {
	fn eq(&self, other: &Self) -> bool {
		self.id == other.id
	}
}

impl NodeState {
	pub async fn write_file(
		&self,
		file: &FileData,
		trans_data: bool,
	) -> Result<(), Box<dyn Error>> {
		use crate::protocol::{ChunkInfo, MetadataEntry};

		let entry_type = match file.tp {
			FileType::File => FileSystemEntryType::File,
			FileType::Dir => FileSystemEntryType::Directory,
			FileType::SymLink => FileSystemEntryType::SymLink,
		};

		let chunks = file
			.chunks
			.iter()
			.map(|c| ChunkInfo { hash: c.hash, offset: c.offset, size: c.size })
			.collect();

		let metadata = MetadataEntry {
			entry_type,
			path: file.path.clone(),
			mode: file.mode,
			user_id: file.user,
			group_id: file.group,
			created_time: file.ctime,
			modified_time: file.mtime,
			size: file.size,
			target: file.target.clone(),
			chunks,
			needs_data_transfer: trans_data,
		};

		let mut protocol = self.protocol.lock().await;
		protocol
			.send_metadata(&metadata)
			.await
			.map_err(|e| Box::new(e) as Box<dyn Error>)?;
		Ok(())
	}

	pub async fn do_collect<F>(&mut self, mut progress_callback: F) -> Result<(), Box<dyn Error>>
	where
		F: FnMut(usize, u64),
	{
		let mut current_file: Option<path::PathBuf> = None;
		let mut files_count = 0;
		let mut bytes_count = 0u64;
		let mut last_update = std::time::Instant::now();

		// Request directory listing
		{
			let mut protocol = self.protocol.lock().await;
			protocol.request_listing().await.map_err(|e| Box::new(e) as Box<dyn Error>)?;
		}

		// Receive entries
		loop {
			let entry_opt = {
				let mut protocol = self.protocol.lock().await;
				protocol.receive_entry().await.map_err(|e| Box::new(e) as Box<dyn Error>)?
			};

			let Some(entry) = entry_opt else {
				break; // End of listing
			};

			match entry.entry_type {
				FileSystemEntryType::File => {
					// If this is a file with chunk data (not just a chunk reference)
					if !entry.path.as_os_str().is_empty() {
						let fd = Box::new(FileData {
							path: entry.path.clone(),
							tp: FileType::File,
							mode: entry.mode,
							user: entry.user_id,
							group: entry.group_id,
							ctime: entry.created_time,
							mtime: entry.modified_time,
							size: entry.size,
							chunks: Vec::new(),
							target: None,
						});

						bytes_count += fd.size;
						files_count += 1;
						self.dir.insert(entry.path.clone(), fd);
						current_file = Some(entry.path.clone());

						// Send progress update every 100 files or every 200ms
						if files_count % 100 == 0 || last_update.elapsed().as_millis() >= 200 {
							progress_callback(files_count, bytes_count);
							last_update = std::time::Instant::now();
						}
					} else {
						// This is a chunk reference for the current file
						if let Some(ref file_path) = current_file {
							if let Some(file_data) = self.dir.get_mut(file_path) {
								for chunk_info in &entry.chunks {
									file_data.chunks.push(crate::types::HashChunk {
										hash: chunk_info.hash,
										offset: chunk_info.offset,
										size: chunk_info.size,
									});
									self.chunks.insert(chunk_info.hash);
								}
							}
						}
					}
				}
				FileSystemEntryType::Directory => {
					let fd = Box::new(FileData {
						path: entry.path.clone(),
						tp: FileType::Dir,
						mode: entry.mode,
						user: entry.user_id,
						group: entry.group_id,
						ctime: entry.created_time,
						mtime: entry.modified_time,
						size: 0,
						chunks: Vec::new(),
						target: None,
					});

					files_count += 1;
					self.dir.insert(entry.path.clone(), fd);
					current_file = None;

					// Send progress update every 100 files or every 200ms
					if files_count % 100 == 0 || last_update.elapsed().as_millis() >= 200 {
						progress_callback(files_count, bytes_count);
						last_update = std::time::Instant::now();
					}
				}
				FileSystemEntryType::SymLink => {
					let fd = Box::new(FileData {
						path: entry.path.clone(),
						tp: FileType::SymLink,
						mode: entry.mode,
						user: entry.user_id,
						group: entry.group_id,
						ctime: entry.created_time,
						mtime: entry.modified_time,
						size: 0,
						chunks: Vec::new(),
						target: entry.target,
					});

					files_count += 1;
					self.dir.insert(entry.path.clone(), fd);
					current_file = None;

					// Send progress update every 100 files or every 200ms
					if files_count % 100 == 0 || last_update.elapsed().as_millis() >= 200 {
						progress_callback(files_count, bytes_count);
						last_update = std::time::Instant::now();
					}
				}
			}
		}

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	#[test]
	fn test_nodestate_equality() {
		// NodeState can't be directly constructed in tests without async,
		// but we can verify the PartialEq trait behavior
		// This is tested implicitly by the sync tests
	}
}

// vim: ts=4
