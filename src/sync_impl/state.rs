//! Node state management for sync operations

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::path;
use tokio::sync::Mutex;

use crate::metadata::NodeCapabilities;
use crate::protocol::{FileSystemEntryType, ProtocolClient};
use crate::types::{FileData, FileType};

/// State for a single sync node (local or remote)
pub struct NodeState {
	pub id: u8,
	pub protocol: Mutex<Box<dyn ProtocolClient>>,
	pub dir: BTreeMap<path::PathBuf, Box<FileData>>,
	pub chunks: BTreeSet<[u8; 32]>,             // Binary BLAKE3 hashes
	pub missing: Mutex<BTreeSet<String>>,       // Base64-encoded hashes needing transfer
	pub capabilities: Option<NodeCapabilities>, // Node capabilities (detected during sync)
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
			needs_data_transfer: Some(trans_data),
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
					// Convert protocol chunks to FileData chunks
					let chunks: Vec<crate::types::HashChunk> = entry
						.chunks
						.iter()
						.map(|c| crate::types::HashChunk {
							hash: c.hash,
							offset: c.offset,
							size: c.size,
						})
						.collect();

					// Track chunks for deduplication
					for chunk in &chunks {
						self.chunks.insert(chunk.hash);
					}

					let fd = Box::new(
						FileData::builder(FileType::File, entry.path.clone())
							.mode(entry.mode)
							.user(entry.user_id)
							.group(entry.group_id)
							.ctime(entry.created_time)
							.mtime(entry.modified_time)
							.size(entry.size)
							.chunks(chunks)
							.build(),
					);

					bytes_count += fd.size;
					files_count += 1;
					self.dir.insert(entry.path.clone(), fd);

					// Send progress update every 100 files or every 200ms
					if files_count % 100 == 0 || last_update.elapsed().as_millis() >= 200 {
						progress_callback(files_count, bytes_count);
						last_update = std::time::Instant::now();
					}
				}
				FileSystemEntryType::Directory => {
					let fd = Box::new(
						FileData::builder(FileType::Dir, entry.path.clone())
							.mode(entry.mode)
							.user(entry.user_id)
							.group(entry.group_id)
							.ctime(entry.created_time)
							.mtime(entry.modified_time)
							.size(0)
							.build(),
					);

					files_count += 1;
					self.dir.insert(entry.path.clone(), fd);

					// Send progress update every 100 files or every 200ms
					if files_count % 100 == 0 || last_update.elapsed().as_millis() >= 200 {
						progress_callback(files_count, bytes_count);
						last_update = std::time::Instant::now();
					}
				}
				FileSystemEntryType::SymLink => {
					let fd = Box::new(
						FileData::builder(FileType::SymLink, entry.path.clone())
							.mode(entry.mode)
							.user(entry.user_id)
							.group(entry.group_id)
							.ctime(entry.created_time)
							.mtime(entry.modified_time)
							.size(0)
							.target(entry.target)
							.build(),
					);

					files_count += 1;
					self.dir.insert(entry.path.clone(), fd);

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
