//! Protocol V3 implementation (JSON5 format with binary chunks)
//!
//! This implementation handles the newer SyncR protocol with JSON5-formatted
//! commands and binary chunk transfer for better performance.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

use crate::util;

use super::error::ProtocolError;
use super::traits::{ProtocolClient, ProtocolResult};
use super::types::*;

// V3 Protocol message structures
#[derive(Serialize, Deserialize, Debug, Clone)]
struct FileEntity {
	#[serde(rename = "typ")]
	entity_type: String,
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
	entity_type: String,
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
	entity_type: String,
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
	entity_type: String,
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

pub struct ProtocolV3 {
	send: Arc<Mutex<tokio::process::ChildStdin>>,
	recv: Arc<Mutex<BufReader<tokio::process::ChildStdout>>>,
	chunks: BTreeSet<[u8; 32]>,
	missing: Arc<Mutex<BTreeSet<String>>>,
	buffered_entry_line: Option<String>, // Buffer a line read ahead during chunk processing
}

impl ProtocolV3 {
	/// Create a new V3 protocol instance from owned streams
	pub fn new(
		send: tokio::process::ChildStdin,
		recv: BufReader<tokio::process::ChildStdout>,
	) -> Self {
		Self::with_shared_streams(Arc::new(Mutex::new(send)), Arc::new(Mutex::new(recv)))
	}

	/// Create a new V3 protocol instance from shared (Arc<Mutex<>>) streams
	pub fn with_shared_streams(
		send: Arc<Mutex<tokio::process::ChildStdin>>,
		recv: Arc<Mutex<BufReader<tokio::process::ChildStdout>>>,
	) -> Self {
		Self {
			send,
			recv,
			chunks: BTreeSet::new(),
			missing: Arc::new(Mutex::new(BTreeSet::new())),
			buffered_entry_line: None,
		}
	}
}

#[async_trait]
impl ProtocolClient for ProtocolV3 {
	fn protocol_name(&self) -> &str {
		"v3"
	}

	async fn request_capabilities(&mut self) -> ProtocolResult<NodeCapabilities> {
		let mut sender = self.send.lock().await;
		sender.write_all(b"{\"cmd\":\"CAP\"}\n").await?;
		sender.flush().await?;

		// Wait for the response
		let mut buf = String::new();
		let mut receiver = self.recv.lock().await;

		loop {
			buf.clear();
			receiver.read_line(&mut buf).await?;

			let trimmed = buf.trim();

			// Skip empty lines
			if trimmed.is_empty() {
				continue;
			}

			// Try parsing as JSON5
			if let Ok(response) = json5::from_str::<serde_json::Value>(trimmed) {
				// Extract capabilities from response
				if let Some(caps) = response.get("capabilities") {
					let capabilities: NodeCapabilities = serde_json::from_value(caps.clone())?;
					return Ok(capabilities);
				}
			}

			// If we get here with non-empty trimmed, it wasn't what we expected
			// Try next line
		}
	}

	async fn close(&mut self) -> ProtocolResult<()> {
		let mut sender = self.send.lock().await;
		sender.write_all(b"{\"cmd\":\"QUIT\"}\n").await?;
		sender.flush().await?;
		Ok(())
	}

	async fn request_listing(&mut self) -> ProtocolResult<()> {
		let mut sender = self.send.lock().await;
		sender.write_all(b"{\"cmd\":\"LIST\"}\n").await?;
		sender.flush().await?;
		Ok(())
	}

	async fn receive_entry(&mut self) -> ProtocolResult<Option<FileSystemEntry>> {
		let mut buf = String::new();
		let mut receiver = self.recv.lock().await;

		loop {
			// Use buffered line if available, otherwise read a new line
			let trimmed = if let Some(buffered) = self.buffered_entry_line.take() {
				buf = buffered;
				buf.trim()
			} else {
				buf.clear();
				receiver.read_line(&mut buf).await?;
				buf.trim()
			};

			// Check for end of listing
			if trimmed == "{\"cmd\":\"END\"}" {
				return Ok(None);
			}

			// Skip empty lines
			if trimmed.is_empty() {
				continue;
			}

			// Try parsing as JSON5
			if let Ok(entity) = json5::from_str::<serde_json::Value>(trimmed) {
				if let Some(typ) = entity.get("typ").and_then(|v| v.as_str()) {
					match typ {
						"F" => {
							let path_str =
								entity.get("pth").and_then(|v| v.as_str()).ok_or("Missing pth")?;
							let mode =
								entity.get("mod").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
							let user =
								entity.get("uid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
							let group =
								entity.get("gid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
							let ctime =
								entity.get("ct").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
							let mtime =
								entity.get("mt").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
							let size = entity.get("sz").and_then(|v| v.as_u64()).unwrap_or(0);

							let mut entry = FileSystemEntry {
								entry_type: FileSystemEntryType::File,
								path: PathBuf::from(path_str),
								mode,
								user_id: user,
								group_id: group,
								created_time: ctime,
								modified_time: mtime,
								size,
								target: None,
								chunks: Vec::new(),
								needs_data_transfer: None,
							};

							// Read subsequent chunks for this file
							loop {
								buf.clear();
								receiver.read_line(&mut buf).await?;
								let chunk_trimmed = buf.trim();

								// Check for end of listing
								if chunk_trimmed == "{\"cmd\":\"END\"}" {
									// End of listing - return this entry and note that we hit END
									// We'll return None on the next call
									self.buffered_entry_line = Some(chunk_trimmed.to_string());
									return Ok(Some(entry));
								}

								// Skip empty lines
								if chunk_trimmed.is_empty() {
									continue;
								}

								if let Ok(chunk_entity) =
									json5::from_str::<serde_json::Value>(chunk_trimmed)
								{
									if let Some(chunk_typ) =
										chunk_entity.get("typ").and_then(|v| v.as_str())
									{
										match chunk_typ {
											"C" => {
												// Chunk for this file
												let offset = chunk_entity
													.get("off")
													.and_then(|v| v.as_u64())
													.ok_or("Missing off")?;
												let length = chunk_entity
													.get("len")
													.and_then(|v| v.as_u64())
													.ok_or("Missing len")?;
												let hash_b64 = chunk_entity
													.get("hsh")
													.and_then(|v| v.as_str())
													.ok_or("Missing hsh")?;

												let hash_binary = util::base64_to_hash(hash_b64)?;
												self.chunks.insert(hash_binary);
												entry.chunks.push(ChunkInfo {
													hash: hash_binary,
													offset,
													size: length as u32,
												});
											}
											"F" | "D" | "S" => {
												// Next entry found - buffer it and return current entry
												self.buffered_entry_line =
													Some(chunk_trimmed.to_string());
												return Ok(Some(entry));
											}
											_ => {
												// Unknown chunk type - skip
												continue;
											}
										}
									}
								}
							}
						}
						"D" => {
							let path_str =
								entity.get("pth").and_then(|v| v.as_str()).ok_or("Missing pth")?;
							let mode =
								entity.get("mod").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
							let user =
								entity.get("uid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
							let group =
								entity.get("gid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
							let ctime =
								entity.get("ct").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
							let mtime =
								entity.get("mt").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

							return Ok(Some(FileSystemEntry {
								entry_type: FileSystemEntryType::Directory,
								path: PathBuf::from(path_str),
								mode,
								user_id: user,
								group_id: group,
								created_time: ctime,
								modified_time: mtime,
								size: 0,
								target: None,
								chunks: Vec::new(),
								needs_data_transfer: None,
							}));
						}
						"S" => {
							let path_str =
								entity.get("pth").and_then(|v| v.as_str()).ok_or("Missing pth")?;
							let mode =
								entity.get("mod").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
							let user =
								entity.get("uid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
							let group =
								entity.get("gid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
							let ctime =
								entity.get("ct").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
							let mtime =
								entity.get("mt").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
							let target = entity.get("tgt").and_then(|v| v.as_str());

							return Ok(Some(FileSystemEntry {
								entry_type: FileSystemEntryType::SymLink,
								path: PathBuf::from(path_str),
								mode,
								user_id: user,
								group_id: group,
								created_time: ctime,
								modified_time: mtime,
								size: 0,
								target: target.map(PathBuf::from),
								chunks: Vec::new(),
								needs_data_transfer: None,
							}));
						}
						"C" => {
							// Chunk without a preceding file entry - track it locally
							// This shouldn't normally happen
							let hash_b64 =
								entity.get("hsh").and_then(|v| v.as_str()).ok_or("Missing hsh")?;
							let hash_binary = util::base64_to_hash(hash_b64)?;
							self.chunks.insert(hash_binary);
							continue;
						}
						_ => continue,
					}
				}
			}
		}
	}

	async fn begin_metadata_transfer(&mut self) -> ProtocolResult<()> {
		let mut sender = self.send.lock().await;
		sender.write_all(b"{\"cmd\":\"WRITE\"}\n").await?;
		sender.flush().await?;
		Ok(())
	}

	async fn send_metadata(&mut self, entry: &MetadataEntry) -> ProtocolResult<()> {
		let mut sender = self.send.lock().await;

		match entry.entry_type {
			FileSystemEntryType::File => {
				let file_entity = FileEntity {
					entity_type: "F".to_string(),
					path: entry.path.to_string_lossy().to_string(),
					mode: Some(entry.mode),
					user_id: Some(entry.user_id),
					group_id: Some(entry.group_id),
					created_time: Some(entry.created_time as u64),
					modified_time: Some(entry.modified_time as u64),
					size: Some(entry.size),
				};

				let json = serde_json::to_string(&file_entity)?;
				sender.write_all(format!("{}\n", json).as_bytes()).await?;

				// Always send chunk messages - the server needs them to set up chunk_writes
				// for every file, regardless of whether data transfer is needed
				for chunk in &entry.chunks {
					let hash_b64 = util::hash_to_base64(&chunk.hash);
					if !self.chunks.contains(&chunk.hash) {
						self.missing.lock().await.insert(hash_b64.clone());
					}

					let chunk_entity = ChunkEntity {
						entity_type: "C".to_string(),
						offset: chunk.offset,
						length: chunk.size,
						hash: hash_b64,
					};
					let json = serde_json::to_string(&chunk_entity)?;
					sender.write_all(format!("{}\n", json).as_bytes()).await?;
				}
			}
			FileSystemEntryType::Directory => {
				let dir_entity = DirectoryEntity {
					entity_type: "D".to_string(),
					path: entry.path.to_string_lossy().to_string(),
					mode: Some(entry.mode),
					user_id: Some(entry.user_id),
					group_id: Some(entry.group_id),
					created_time: Some(entry.created_time as u64),
					modified_time: Some(entry.modified_time as u64),
				};

				let json = serde_json::to_string(&dir_entity)?;
				sender.write_all(format!("{}\n", json).as_bytes()).await?;
			}
			FileSystemEntryType::SymLink => {
				let symlink_entity = SymlinkEntity {
					entity_type: "S".to_string(),
					path: entry.path.to_string_lossy().to_string(),
					mode: Some(entry.mode),
					user_id: Some(entry.user_id),
					group_id: Some(entry.group_id),
					created_time: Some(entry.created_time as u64),
					modified_time: Some(entry.modified_time as u64),
					target: entry.target.as_ref().map(|p| p.to_string_lossy().to_string()),
				};

				let json = serde_json::to_string(&symlink_entity)?;
				sender.write_all(format!("{}\n", json).as_bytes()).await?;
			}
		}

		sender.flush().await?;
		Ok(())
	}

	async fn send_delete(&mut self, path: &Path) -> ProtocolResult<()> {
		let mut sender = self.send.lock().await;
		let cmd = serde_json::json!({
			"cmd": "DEL",
			"pth": path.to_string_lossy(),
		});
		let json = serde_json::to_string(&cmd)?;
		sender.write_all(format!("{}\n", json).as_bytes()).await?;
		sender.flush().await?;
		Ok(())
	}

	async fn end_metadata_transfer(&mut self) -> ProtocolResult<()> {
		let mut sender = self.send.lock().await;
		sender.write_all(b"{\"cmd\":\"END\"}\n").await?;
		sender.flush().await?;
		Ok(())
	}

	async fn begin_chunk_transfer(&mut self) -> ProtocolResult<()> {
		let mut sender = self.send.lock().await;
		sender.write_all(b"{\"cmd\":\"READ\"}\n").await?;
		sender.flush().await?;
		Ok(())
	}

	async fn request_chunks(&mut self, chunk_hashes: &[String]) -> ProtocolResult<()> {
		tracing::debug!("[v3_client] request_chunks called with {} hashes", chunk_hashes.len());
		let mut sender = self.send.lock().await;
		for (idx, hash) in chunk_hashes.iter().enumerate() {
			let cmd = serde_json::json!({ "hsh": hash });
			let json = serde_json::to_string(&cmd).map_err(|e| {
				tracing::error!("[v3_client] Failed to serialize chunk request {}: {}", hash, e);
				ProtocolError::Other(format!("Failed to serialize chunk {}: {}", hash, e))
			})?;
			tracing::debug!(
				"[v3_client] Sending chunk request {}/{}: {}",
				idx + 1,
				chunk_hashes.len(),
				hash
			);
			sender.write_all(format!("{}\n", json).as_bytes()).await.map_err(|e| {
				tracing::error!("[v3_client] Failed to write chunk request {}: {}", hash, e);
				ProtocolError::Io(e)
			})?;
		}
		tracing::debug!(
			"[v3_client] Sending END marker after {} chunk requests",
			chunk_hashes.len()
		);
		sender.write_all(b"{\"cmd\":\"END\"}\n").await.map_err(|e| {
			tracing::error!("[v3_client] Failed to write END marker: {}", e);
			ProtocolError::Io(e)
		})?;
		sender.flush().await.map_err(|e| {
			tracing::error!("[v3_client] Failed to flush after chunk requests: {}", e);
			ProtocolError::Io(e)
		})?;
		tracing::debug!("[v3_client] Successfully sent {} chunk requests", chunk_hashes.len());
		Ok(())
	}

	async fn receive_chunk(&mut self) -> ProtocolResult<Option<ChunkData>> {
		let mut buf = String::new();
		let mut receiver = self.recv.lock().await;
		let mut line_count = 0u32;

		loop {
			buf.clear();
			line_count += 1;
			receiver.read_line(&mut buf).await.map_err(|e| {
				tracing::error!("[v3_client] Failed to read line (attempt {}): {}", line_count, e);
				ProtocolError::Io(e)
			})?;

			let trimmed = buf.trim();

			if trimmed.is_empty() {
				tracing::debug!("[v3_client] Skipping empty line {}", line_count);
				continue;
			}

			tracing::debug!(
				"[v3_client] Received line {}: {}",
				line_count,
				if trimmed.len() > 100 { &trimmed[..100] } else { trimmed }
			);

			// Check for end marker
			if trimmed == "{\"cmd\":\"END\"}" {
				tracing::debug!("[v3_client] Received END marker, no more chunks");
				return Ok(None);
			}

			// Try parsing as CHK header
			match json5::from_str::<serde_json::Value>(trimmed) {
				Ok(json_obj) => {
					if let Some("CHK") = json_obj.get("cmd").and_then(|v| v.as_str()) {
						let hash_str = json_obj.get("hsh").and_then(|v| v.as_str());
						let len_val = json_obj.get("len").and_then(|v| v.as_u64());

						if let (Some(hash_str), Some(len_val)) = (hash_str, len_val) {
							let chunk_len = len_val as usize;
							tracing::debug!(
								"[v3_client] Received CHK header for chunk {}, size: {} bytes",
								hash_str,
								chunk_len
							);
							let mut chunk_data = vec![0u8; chunk_len];

							// Read binary chunk data
							let mut bytes_read = 0;
							while bytes_read < chunk_len {
								match receiver.read(&mut chunk_data[bytes_read..]).await {
									Ok(n) => {
										if n == 0 {
											return Err(ProtocolError::ProtocolViolation(
												format!("Unexpected EOF while reading chunk {} (got {}/{} bytes)",
													hash_str, bytes_read, chunk_len)
											));
										}
										bytes_read += n;
									}
									Err(e) => {
										return Err(ProtocolError::Io(e));
									}
								}
							}
							tracing::debug!(
								"[v3_client] Successfully read {} bytes for chunk {}",
								chunk_len,
								hash_str
							);

							// Read the trailing newline
							let mut trailing = [0u8; 1];
							match receiver.read(&mut trailing).await {
								Ok(n) => {
									if n == 0 || trailing[0] != b'\n' {
										return Err(ProtocolError::ProtocolViolation(format!(
											"Expected newline after chunk {} data, got: {:?}",
											hash_str, trailing
										)));
									}
								}
								Err(e) => {
									return Err(ProtocolError::Io(e));
								}
							}

							tracing::debug!(
								"[v3_client] Successfully received complete chunk {} ({} bytes)",
								hash_str,
								chunk_len
							);
							return Ok(Some(ChunkData {
								hash: hash_str.to_string(),
								data: chunk_data,
							}));
						} else {
							tracing::warn!(
								"[v3_client] CHK header missing hsh or len: hsh={:?}, len={:?}",
								hash_str,
								len_val
							);
						}
					} else {
						tracing::debug!(
							"[v3_client] Non-CHK JSON received, skipping: cmd={:?}",
							json_obj.get("cmd")
						);
					}
				}
				Err(e) => {
					tracing::warn!(
						"[v3_client] Failed to parse JSON on line {}: {} (content: {})",
						line_count,
						e,
						if trimmed.len() > 100 { &trimmed[..100] } else { trimmed }
					);
				}
			}
		}
	}

	async fn send_chunk(&mut self, hash: &str, data: &[u8]) -> ProtocolResult<()> {
		let mut sender = self.send.lock().await;

		// Send chunk header
		let header = ChunkHeader {
			cmd: "CHK".to_string(),
			hash: hash.to_string(),
			length: data.len() as u32,
		};
		let json = serde_json::to_string(&header)?;
		sender.write_all(format!("{}\n", json).as_bytes()).await?;

		// Send binary data
		sender.write_all(data).await?;
		sender.write_all(b"\n").await?;
		sender.flush().await?;
		Ok(())
	}

	async fn end_chunk_transfer(&mut self) -> ProtocolResult<()> {
		// No-op: READ session already closed when server sent END marker
		// The server exits serve_read_v3() after sending END and returns to main loop
		Ok(())
	}

	async fn commit(&mut self) -> ProtocolResult<CommitResponse> {
		// Send commit command and drop the lock before reading response
		{
			let mut sender = self.send.lock().await;
			sender.write_all(b"{\"cmd\":\"COMMIT\"}\n").await?;
			sender.flush().await?;
		} // Drop sender lock here to avoid deadlock

		// Read response
		let mut buf = String::new();
		let mut receiver = self.recv.lock().await;
		receiver.read_line(&mut buf).await?;

		let trimmed = buf.trim();
		if let Ok(json_obj) = json5::from_str::<serde_json::Value>(trimmed) {
			if let Some(cmd) = json_obj.get("cmd").and_then(|v| v.as_str()) {
				if cmd == "OK" {
					return Ok(CommitResponse {
						success: true,
						message: None,
						renamed_count: json_obj
							.get("renamed")
							.and_then(|v| v.as_u64())
							.map(|v| v as usize),
						failed_count: json_obj
							.get("failed")
							.and_then(|v| v.as_u64())
							.map(|v| v as usize),
					});
				} else if cmd == "ERR" {
					let msg = json_obj.get("msg").and_then(|v| v.as_str()).map(|s| s.to_string());
					return Ok(CommitResponse {
						success: false,
						message: msg,
						renamed_count: None,
						failed_count: None,
					});
				}
			}
		}

		Err("Unexpected commit response".into())
	}

	fn has_chunk(&self, hash: &[u8; 32]) -> bool {
		self.chunks.contains(hash)
	}

	fn mark_chunk_missing(&self, hash: String) {
		let rt = tokio::runtime::Handle::try_current();
		if rt.is_ok() {
			let missing = self.missing.clone();
			tokio::spawn(async move {
				missing.lock().await.insert(hash);
			});
		}
	}

	fn missing_chunk_count(&self) -> usize {
		0 // Limitation of sync method - would need to be async
	}

	async fn get_missing_chunks(&self) -> Vec<String> {
		self.missing.lock().await.iter().cloned().collect()
	}

	fn clear_missing_chunks(&self) {
		let rt = tokio::runtime::Handle::try_current();
		if rt.is_ok() {
			let missing = self.missing.clone();
			tokio::spawn(async move {
				missing.lock().await.clear();
			});
		}
	}
}

// vim: ts=4
