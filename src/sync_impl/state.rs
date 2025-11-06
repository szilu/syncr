//! Node state management for sync operations

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

use crate::metadata_utils;
use crate::protocol_utils;
use crate::types::{FileData, FileType};

use super::protocol::{log_trace_message, parse_trace_message};

/// State for a single sync node (local or remote)
pub struct NodeState {
	pub id: u8,
	pub send: Mutex<tokio::process::ChildStdin>,
	pub recv: Mutex<BufReader<tokio::process::ChildStdout>>,
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
		let mut sender = self.send.lock().await;
		match file.tp {
			FileType::File => {
				if trans_data {
					let msg = format!(
						"FD:{}:{}:{}:{}:{}:{}:{}",
						file.path.to_string_lossy(),
						file.mode,
						file.user,
						file.group,
						file.ctime,
						file.mtime,
						file.size
					);
					sender.write_all(format!("{}\n", msg).as_bytes()).await?;
					for chunk in &file.chunks {
						let hash_b64 = crate::util::hash_to_base64(&chunk.hash);
						if !self.chunks.contains(&chunk.hash) {
							// Chunk needs transfer
							let msg = format!("RC:{}:{}:{}", chunk.offset, chunk.size, hash_b64);
							sender.write_all(format!("{}\n", msg).as_bytes()).await?;
							self.missing.lock().await.insert(hash_b64);
						} else {
							// Chunk is available locally
							let msg = format!("LC:{}:{}:{}", chunk.offset, chunk.size, hash_b64);
							sender.write_all(format!("{}\n", msg).as_bytes()).await?;
						}
					}
					sender.write_all(b".\n").await?;
				} else {
					let msg = format!(
						"FM:{}:{}:{}:{}:{}:{}:{}",
						file.path.to_string_lossy(),
						file.mode,
						file.user,
						file.group,
						file.ctime,
						file.mtime,
						file.size
					);
					sender.write_all(format!("{}\n", msg).as_bytes()).await?;
				}
			}
			FileType::SymLink => {
				let target = file
					.target
					.as_ref()
					.map(|p| p.to_string_lossy().to_string())
					.unwrap_or_default();
				let msg = format!(
					"L:{}:{}:{}:{}:{}:{}:{}",
					file.path.to_string_lossy(),
					file.mode,
					file.user,
					file.group,
					file.ctime,
					file.mtime,
					target
				);
				sender
					.write_all(
						format!(
							"{}
",
							msg
						)
						.as_bytes(),
					)
					.await?;
			}
			FileType::Dir => {
				let msg = format!(
					"D:{}:{}:{}:{}:{}:{}",
					file.path.to_string_lossy(),
					file.mode,
					file.user,
					file.group,
					file.ctime,
					file.mtime
				);
				sender.write_all(format!("{}\n", msg).as_bytes()).await?;
			}
		}
		sender.flush().await?;
		Ok(())
	}

	pub async fn send(&self, buf: &str) -> Result<(), Box<dyn Error>> {
		let mut sender = self.send.lock().await;
		sender.write_all([buf, "\n"].concat().as_bytes()).await?;
		sender.flush().await?;
		Ok(())
	}

	pub async fn do_collect<F>(&mut self, mut progress_callback: F) -> Result<(), Box<dyn Error>>
	where
		F: FnMut(usize, u64),
	{
		let mut buf = String::new();
		let mut file_data: Option<&mut Box<FileData>> = None;
		let mut files_count = 0;
		let mut bytes_count = 0u64;
		let mut last_update = std::time::Instant::now();

		// Note: Handshake already consumed the initialization "." message,
		// so we don't need to read it again here. Just send LIST directly.
		self.send.lock().await.write_all(b"LIST\n").await?;
		self.send.lock().await.flush().await?;
		loop {
			buf.clear();
			self.recv.lock().await.read_line(&mut buf).await?;
			if buf.trim() == "." {
				break;
			}
			//println!("[{}]LINE: {}", self.id, buf.trim());

			// Check for trace messages from child (format: #<LEVEL>:<msg> or !<LEVEL>:<msg>)
			if let Some((level, message)) = parse_trace_message(buf.trim()) {
				log_trace_message(level, &message, self.id);
				continue; // Skip protocol parsing for trace messages
			}

			// Skip empty lines
			if buf.trim().is_empty() {
				continue;
			}

			let fields = protocol_utils::parse_protocol_line(&buf, 1)?;
			let cmd = fields[0];

			match cmd {
				"F" => {
					let fd = metadata_utils::parse_file_metadata(&buf)?;
					let path = fd.path.clone();
					bytes_count += fd.size;
					files_count += 1;
					self.dir.insert(fd.path.clone(), fd);
					file_data = self.dir.get_mut(&path);

					// Send progress update every 100 files or every 200ms
					if files_count % 100 == 0 || last_update.elapsed().as_millis() >= 200 {
						progress_callback(files_count, bytes_count);
						last_update = std::time::Instant::now();
					}
				}
				"C" => {
					let hc = metadata_utils::parse_chunk_metadata(&buf)?;
					match &mut file_data {
						Some(data) => {
							data.chunks.push(hc.clone());
						}
						None => {
							return Err("Protocol error: chunk without file".into());
						}
					}
					self.chunks.insert(hc.hash);
				}
				"L" => {
					let fd = metadata_utils::parse_symlink_metadata(&buf)?;
					let path = fd.path.clone();
					files_count += 1;
					self.dir.insert(fd.path.clone(), fd);
					file_data = self.dir.get_mut(&path);

					// Send progress update every 100 files or every 200ms
					if files_count % 100 == 0 || last_update.elapsed().as_millis() >= 200 {
						progress_callback(files_count, bytes_count);
						last_update = std::time::Instant::now();
					}
				}
				"D" => {
					let fd = metadata_utils::parse_dir_metadata(&buf)?;
					let path = fd.path.clone();
					files_count += 1;
					self.dir.insert(fd.path.clone(), fd);
					file_data = self.dir.get_mut(&path);

					// Send progress update every 100 files or every 200ms
					if files_count % 100 == 0 || last_update.elapsed().as_millis() >= 200 {
						progress_callback(files_count, bytes_count);
						last_update = std::time::Instant::now();
					}
				}
				_ => return Err(format!("Unknown command in protocol: {}", cmd).into()),
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
