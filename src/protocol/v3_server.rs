//! Protocol V3 server implementation using JSON5 over stdin/stdout
//!
//! This server is used in serve mode to handle protocol commands from
//! a remote sync parent process via JSON5-formatted messages.

use async_trait::async_trait;
use std::path::Path;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tracing::debug;

use super::file_operations::FileSystemServer;
use super::traits::*;
use super::types::*;
use crate::serve::DumpState;
use crate::util;

/// V3 Protocol server using JSON5 over stdin/stdout
pub struct ProtocolV3Server {
	fs_server: FileSystemServer,
}

impl ProtocolV3Server {
	/// Create a new V3 protocol server for the given directory
	pub fn new(base_path: PathBuf, state: DumpState) -> Self {
		Self { fs_server: FileSystemServer::new(base_path, state) }
	}

	/// Run the server loop with async I/O (called from serve() entry point)
	///
	/// This reads JSON5 commands from stdin and processes them,
	/// sending responses to stdout using proper async I/O.
	pub async fn run(
		mut self,
		mut reader: tokio::io::BufReader<tokio::io::Stdin>,
		mut writer: tokio::io::Stdout,
	) -> Result<(), Box<dyn std::error::Error>> {
		let mut line = String::new();

		loop {
			line.clear();
			// Handle read errors gracefully
			if let Err(e) = reader.read_line(&mut line).await {
				let _ = send_error_response(&mut writer, &format!("Read error: {}", e)).await;
				break;
			}

			let n = line.len();
			if n == 0 {
				break; // EOF
			}

			let trimmed = line.trim();

			// Skip empty lines
			if trimmed.is_empty() {
				continue;
			}

			// Parse JSON5 command
			if let Ok(json_obj) = json5::from_str::<serde_json::Value>(trimmed) {
				if let Some(cmd) = json_obj.get("cmd").and_then(|v| v.as_str()) {
					match cmd {
						"VER" => {
							// Protocol version negotiation
							let response = serde_json::json!({
								"cmd": "VER",
								"ver": 3,
							});
							if let Err(e) = write_response(&mut writer, &response).await {
								let _ = send_error_response(
									&mut writer,
									&format!("Write error: {}", e),
								)
								.await;
								break;
							}
						}

						"CAP" => {
							// Request capabilities
							match self.handle_capabilities().await {
								Ok(caps) => {
									let response = serde_json::json!({
										"cmd": "CAP",
										"capabilities": caps
									});
									if let Err(e) = write_response(&mut writer, &response).await {
										let _ = send_error_response(
											&mut writer,
											&format!("Write error: {}", e),
										)
										.await;
										break;
									}
								}
								Err(e) => {
									let _ = send_error_response(
										&mut writer,
										&format!("Capabilities error: {}", e),
									)
									.await;
									continue;
								}
							}
						}

						"LIST" => {
							// Request directory listing with streaming
							// Entries arrive as they're discovered, not after full scan
							match self.handle_list_streaming().await {
								Ok(mut receiver) => {
									let mut had_error = false;

									// Process entries as they arrive from the streaming channel
									while let Some(entry_result) = receiver.recv().await {
										match entry_result {
											Ok(entry) => {
												// Output file/dir/symlink metadata
												match serialize_entry_json5(&entry) {
													Ok(json_str) => {
														if let Err(e) = writer
															.write_all(json_str.as_bytes())
															.await
														{
															let _ = send_error_response(
																&mut writer,
																&format!("Write error: {}", e),
															)
															.await;
															had_error = true;
															break;
														}
														if let Err(e) =
															writer.write_all(b"\n").await
														{
															let _ = send_error_response(
																&mut writer,
																&format!("Write error: {}", e),
															)
															.await;
															had_error = true;
															break;
														}

														// Output chunks for this entry
														for chunk in &entry.chunks {
															let chunk_json = serde_json::json!({
																"typ": "C",
																"off": chunk.offset,
																"len": chunk.size,
																"hsh": util::hash_to_base64(&chunk.hash),
															});
															match serde_json::to_string(&chunk_json)
															{
																Ok(chunk_str) => {
																	if let Err(e) = writer
																		.write_all(
																			chunk_str.as_bytes(),
																		)
																		.await
																	{
																		let _ = send_error_response(
																			&mut writer,
																			&format!("Write error: {}", e),
																		)
																		.await;
																		had_error = true;
																		break;
																	}
																	if let Err(e) = writer
																		.write_all(b"\n")
																		.await
																	{
																		let _ = send_error_response(
																			&mut writer,
																			&format!("Write error: {}", e),
																		)
																		.await;
																		had_error = true;
																		break;
																	}
																}
																Err(e) => {
																	let _ = send_error_response(
																		&mut writer,
																		&format!(
																			"Serialization error: {}",
																			e
																		),
																	)
																	.await;
																	had_error = true;
																	break;
																}
															}
														}
													}
													Err(e) => {
														let _ = send_error_response(
															&mut writer,
															&format!("Serialization error: {}", e),
														)
														.await;
														had_error = true;
														break;
													}
												}
											}
											Err(e) => {
												// Log error from listing, continue to next entry
												let _ = send_error_response(
													&mut writer,
													&format!("Listing error: {}", e),
												)
												.await;
												// Continue processing remaining entries
											}
										}
									}

									// Send END marker only if no fatal error
									if !had_error {
										if let Err(e) =
											writer.write_all(b"{\"cmd\":\"END\"}\n").await
										{
											let _ = send_error_response(
												&mut writer,
												&format!("Write error: {}", e),
											)
											.await;
											break;
										}
										if let Err(e) = writer.flush().await {
											let _ = send_error_response(
												&mut writer,
												&format!("Flush error: {}", e),
											)
											.await;
											break;
										}
									}
								}
								Err(e) => {
									let _ = send_error_response(
										&mut writer,
										&format!("List error: {}", e),
									)
									.await;
									continue;
								}
							}
						}

						"WRITE" => {
							// Enter metadata write mode
							if let Err(e) = self.handle_write_mode(&mut reader, &mut writer).await {
								let _ = send_error_response(
									&mut writer,
									&format!("Write mode error: {}", e),
								)
								.await;
								break;
							}
						}

						"READ" => {
							// Enter chunk read mode
							if let Err(e) = self.handle_read_mode(&mut reader, &mut writer).await {
								let _ = send_error_response(
									&mut writer,
									&format!("Read mode error: {}", e),
								)
								.await;
								break;
							}
						}

						"COMMIT" => {
							// Finalize all changes
							match self.handle_commit().await {
								Ok(result) => {
									let response = serde_json::json!({
										"cmd": "OK",
										"renamed": result.renamed_count,
										"failed": result.failed_count
									});
									if let Err(e) = write_response(&mut writer, &response).await {
										let _ = send_error_response(
											&mut writer,
											&format!("Write error: {}", e),
										)
										.await;
										break;
									}
								}
								Err(e) => {
									let _ = send_error_response(
										&mut writer,
										&format!("Commit error: {}", e),
									)
									.await;
									continue;
								}
							}
						}

						"QUIT" => {
							// Close connection
							let response = serde_json::json!({ "cmd": "OK" });
							let _ = write_response(&mut writer, &response).await;
							break;
						}

						_ => {
							eprintln!("Unknown command: {}", cmd);
							let response = serde_json::json!({
								"cmd": "ERR",
								"msg": "Unknown command"
							});
							if let Err(e) = write_response(&mut writer, &response).await {
								let _ = send_error_response(
									&mut writer,
									&format!("Write error: {}", e),
								)
								.await;
								break;
							}
						}
					}
				}
			}
		}

		Ok(())
	}

	async fn handle_write_mode(
		&mut self,
		reader: &mut tokio::io::BufReader<tokio::io::Stdin>,
		writer: &mut tokio::io::Stdout,
	) -> Result<(), Box<dyn std::error::Error>> {
		// Handle WRITE mode - receive metadata and file creation commands
		// Process entries (files, dirs, symlinks) and their chunks

		// Track pending file and its accumulated chunks
		let mut pending_file: Option<MetadataEntry> = None;
		let mut current_file_chunks: Vec<crate::protocol::ChunkInfo> = Vec::new();

		let mut line = String::new();
		loop {
			line.clear();
			// Handle read errors gracefully
			if let Err(e) = reader.read_line(&mut line).await {
				let _ = send_error_response(writer, &format!("Read error: {}", e)).await;
				return Err(Box::new(e));
			}

			let n = line.len();
			if n == 0 {
				break; // EOF
			}
			let trimmed = line.trim();

			// Try parsing as JSON5 metadata entry
			if let Ok(json_obj) = json5::from_str::<serde_json::Value>(trimmed) {
				// Check for END command
				if let Some(cmd) = json_obj.get("cmd").and_then(|v| v.as_str()) {
					if cmd == "END" {
						// Process pending file before exiting
						if let Some(mut file_entry) = pending_file.take() {
							file_entry.chunks = current_file_chunks.clone();
							if let Err(e) = self.handle_write_metadata(&file_entry).await {
								let _ = send_error_response(
									writer,
									&format!("Write metadata error: {}", e),
								)
								.await;
							}
							current_file_chunks.clear();
						}
						break;
					}
				}

				// Check for metadata entries (type field)
				if let Some(typ) = json_obj.get("typ").and_then(|v| v.as_str()) {
					match typ {
						"F" => {
							// File metadata - process previous pending file first
							if let Some(mut file_entry) = pending_file.take() {
								debug!(
									"[v3_server] Processing pending file: {} with {} chunks",
									file_entry.path.display(),
									current_file_chunks.len()
								);
								file_entry.chunks = current_file_chunks.clone();
								if let Err(e) = self.handle_write_metadata(&file_entry).await {
									let _ = send_error_response(
										writer,
										&format!("Write metadata error: {}", e),
									)
									.await;
								}
								current_file_chunks.clear();
							}

							// Start processing new file
							if let Some(pth) = json_obj.get("pth").and_then(|v| v.as_str()) {
								debug!("[v3_server] Received file metadata: {}", pth);
								let file_path = std::path::PathBuf::from(pth);

								// Build MetadataEntry from JSON
								let entry = MetadataEntry {
									entry_type: FileSystemEntryType::File,
									path: file_path.clone(),
									mode: json_obj
										.get("mod")
										.and_then(|v| v.as_u64())
										.unwrap_or(0o644) as u32,
									user_id: json_obj
										.get("uid")
										.and_then(|v| v.as_u64())
										.unwrap_or(0) as u32,
									group_id: json_obj
										.get("gid")
										.and_then(|v| v.as_u64())
										.unwrap_or(0) as u32,
									created_time: json_obj
										.get("ct")
										.and_then(|v| v.as_u64())
										.unwrap_or(0) as u32,
									modified_time: json_obj
										.get("mt")
										.and_then(|v| v.as_u64())
										.unwrap_or(0) as u32,
									size: json_obj.get("sz").and_then(|v| v.as_u64()).unwrap_or(0),
									target: None,
									chunks: Vec::new(),
									needs_data_transfer: Some(false),
								};
								pending_file = Some(entry);
							}
						}
						"D" => {
							// Directory metadata - process pending file first
							if let Some(mut file_entry) = pending_file.take() {
								file_entry.chunks = current_file_chunks.clone();
								if let Err(e) = self.handle_write_metadata(&file_entry).await {
									let _ = send_error_response(
										writer,
										&format!("Write metadata error: {}", e),
									)
									.await;
								}
								current_file_chunks.clear();
							}

							if let Some(pth) = json_obj.get("pth").and_then(|v| v.as_str()) {
								let entry = MetadataEntry {
									entry_type: FileSystemEntryType::Directory,
									path: std::path::PathBuf::from(pth),
									mode: json_obj
										.get("mod")
										.and_then(|v| v.as_u64())
										.unwrap_or(0o755) as u32,
									user_id: json_obj
										.get("uid")
										.and_then(|v| v.as_u64())
										.unwrap_or(0) as u32,
									group_id: json_obj
										.get("gid")
										.and_then(|v| v.as_u64())
										.unwrap_or(0) as u32,
									created_time: json_obj
										.get("ct")
										.and_then(|v| v.as_u64())
										.unwrap_or(0) as u32,
									modified_time: json_obj
										.get("mt")
										.and_then(|v| v.as_u64())
										.unwrap_or(0) as u32,
									size: 0,
									target: None,
									chunks: Vec::new(),
									needs_data_transfer: Some(false),
								};
								if let Err(e) = self.handle_write_metadata(&entry).await {
									let _ = send_error_response(
										writer,
										&format!("Write metadata error: {}", e),
									)
									.await;
									continue; // Continue processing other metadata
								}
							}
						}
						"S" => {
							// Symlink metadata - process pending file first
							if let Some(mut file_entry) = pending_file.take() {
								file_entry.chunks = current_file_chunks.clone();
								if let Err(e) = self.handle_write_metadata(&file_entry).await {
									let _ = send_error_response(
										writer,
										&format!("Write metadata error: {}", e),
									)
									.await;
								}
								current_file_chunks.clear();
							}

							if let Some(pth) = json_obj.get("pth").and_then(|v| v.as_str()) {
								let target = json_obj
									.get("tgt")
									.and_then(|v| v.as_str())
									.map(std::path::PathBuf::from);
								let entry = MetadataEntry {
									entry_type: FileSystemEntryType::SymLink,
									path: std::path::PathBuf::from(pth),
									mode: json_obj
										.get("mod")
										.and_then(|v| v.as_u64())
										.unwrap_or(0o644) as u32,
									user_id: json_obj
										.get("uid")
										.and_then(|v| v.as_u64())
										.unwrap_or(0) as u32,
									group_id: json_obj
										.get("gid")
										.and_then(|v| v.as_u64())
										.unwrap_or(0) as u32,
									created_time: json_obj
										.get("ct")
										.and_then(|v| v.as_u64())
										.unwrap_or(0) as u32,
									modified_time: json_obj
										.get("mt")
										.and_then(|v| v.as_u64())
										.unwrap_or(0) as u32,
									size: 0,
									target,
									chunks: Vec::new(),
									needs_data_transfer: Some(false),
								};
								if let Err(e) = self.handle_write_metadata(&entry).await {
									let _ = send_error_response(
										writer,
										&format!("Write metadata error: {}", e),
									)
									.await;
									continue; // Continue processing other metadata
								}
							}
						}
						"C" => {
							// Chunk metadata - accumulate chunks for the pending file
							if let (Some(off), Some(len), Some(hsh)) = (
								json_obj.get("off").and_then(|v| v.as_u64()),
								json_obj.get("len").and_then(|v| v.as_u64()),
								json_obj.get("hsh").and_then(|v| v.as_str()),
							) {
								debug!(
									"[v3_server] Received chunk metadata: off={}, len={}, hash={}",
									off, len, hsh
								);
								if let Ok(hash) = crate::util::base64_to_hash(hsh) {
									current_file_chunks.push(crate::protocol::ChunkInfo {
										hash,
										offset: off,
										size: len as u32,
									});
									debug!(
										"[v3_server] Added chunk to pending (now {} chunks)",
										current_file_chunks.len()
									);
								} else {
									debug!("[v3_server] Failed to decode hash: {}", hsh);
								}
							} else {
								debug!("[v3_server] Chunk message incomplete: off={:?}, len={:?}, hsh={:?}",
									json_obj.get("off"), json_obj.get("len"), json_obj.get("hsh"));
							}
						}
						_ => {}
					}
				}
			}
		}

		// Send completion response
		let response = serde_json::json!({ "cmd": "OK" });
		write_response(writer, &response).await?;
		Ok(())
	}

	async fn handle_read_mode(
		&mut self,
		reader: &mut tokio::io::BufReader<tokio::io::Stdin>,
		writer: &mut tokio::io::Stdout,
	) -> Result<(), Box<dyn std::error::Error>> {
		// Handle READ mode - send requested chunks
		tracing::debug!("[v3_server] Entering READ mode");
		let mut requested_hashes: Vec<String> = Vec::new();

		// Read hash requests until END
		let mut line = String::new();
		let mut line_count = 0u32;
		loop {
			line.clear();
			line_count += 1;
			// Handle read errors gracefully
			if let Err(e) = reader.read_line(&mut line).await {
				tracing::error!("[v3_server] Read error on line {}: {}", line_count, e);
				let _ = send_error_response(writer, &format!("Read error: {}", e)).await;
				return Err(Box::new(e));
			}

			let n = line.len();
			if n == 0 {
				tracing::debug!("[v3_server] Reached EOF");
				break; // EOF
			}
			let trimmed = line.trim();

			// Try parsing as JSON5
			match json5::from_str::<serde_json::Value>(trimmed) {
				Ok(json_obj) => {
					if let Some(cmd) = json_obj.get("cmd").and_then(|v| v.as_str()) {
						if cmd == "END" {
							tracing::debug!(
								"[v3_server] Received END marker, {} hashes requested",
								requested_hashes.len()
							);
							break;
						}
					}
					if let Some(hsh) = json_obj.get("hsh").and_then(|v| v.as_str()) {
						tracing::debug!("[v3_server] Request chunk {}", hsh);
						requested_hashes.push(hsh.to_string());
					}
				}
				Err(e) => {
					tracing::warn!(
						"[v3_server] Failed to parse JSON on line {}: {}",
						line_count,
						e
					);
				}
			}
		}

		tracing::info!("[v3_server] Client requested {} chunks", requested_hashes.len());

		// Send each requested chunk
		match self.handle_read_chunks(&requested_hashes).await {
			Ok(chunks) => {
				tracing::debug!("[v3_server] handle_read_chunks returned {} chunks", chunks.len());
				for (idx, chunk) in chunks.iter().enumerate() {
					let header = serde_json::json!({
						"cmd": "CHK",
						"hsh": &chunk.hash,
						"len": chunk.data.len()
					});
					tracing::debug!(
						"[v3_server] Sending chunk {}/{}: {} ({} bytes)",
						idx + 1,
						chunks.len(),
						chunk.hash,
						chunk.data.len()
					);
					match serde_json::to_string(&header) {
						Ok(header_str) => {
							if let Err(e) = writer.write_all(header_str.as_bytes()).await {
								tracing::error!(
									"[v3_server] Failed to write header for chunk {}: {}",
									chunk.hash,
									e
								);
								let _ = send_error_response(writer, &format!("Write error: {}", e))
									.await;
								return Err(Box::new(e));
							}
							if let Err(e) = writer.write_all(b"\n").await {
								tracing::error!("[v3_server] Failed to write newline after header for chunk {}: {}", chunk.hash, e);
								let _ = send_error_response(writer, &format!("Write error: {}", e))
									.await;
								return Err(Box::new(e));
							}

							// Send binary data
							if let Err(e) = writer.write_all(&chunk.data).await {
								tracing::error!(
									"[v3_server] Failed to write data for chunk {}: {}",
									chunk.hash,
									e
								);
								let _ = send_error_response(writer, &format!("Write error: {}", e))
									.await;
								return Err(Box::new(e));
							}
							if let Err(e) = writer.write_all(b"\n").await {
								tracing::error!("[v3_server] Failed to write newline after data for chunk {}: {}", chunk.hash, e);
								let _ = send_error_response(writer, &format!("Write error: {}", e))
									.await;
								return Err(Box::new(e));
							}
							tracing::debug!(
								"[v3_server] Successfully sent chunk {} ({} bytes)",
								chunk.hash,
								chunk.data.len()
							);
						}
						Err(e) => {
							let _ =
								send_error_response(writer, &format!("Serialization error: {}", e))
									.await;
							return Err(Box::new(e));
						}
					}
				}
			}
			Err(e) => {
				let _ = send_error_response(writer, &format!("Read chunks error: {}", e)).await;
				return Err(Box::new(e));
			}
		}

		// Send END marker
		if let Err(e) = writer.write_all(b"{\"cmd\":\"END\"}\n").await {
			let _ = send_error_response(writer, &format!("Write error: {}", e)).await;
			return Err(Box::new(e));
		}
		if let Err(e) = writer.flush().await {
			let _ = send_error_response(writer, &format!("Flush error: {}", e)).await;
			return Err(Box::new(e));
		}
		Ok(())
	}
}

/// Helper function to write a JSON response with proper async I/O
async fn write_response(
	writer: &mut tokio::io::Stdout,
	response: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
	let json_str = serde_json::to_string(response)?;
	writer.write_all(json_str.as_bytes()).await?;
	writer.write_all(b"\n").await?;
	writer.flush().await?;
	Ok(())
}

/// Send an error response to the client - this MUST ALWAYS be called before exiting with an error
async fn send_error_response(
	writer: &mut tokio::io::Stdout,
	message: &str,
) -> Result<(), Box<dyn std::error::Error>> {
	let response = serde_json::json!({
		"cmd": "ERR",
		"msg": message
	});
	let json_str = serde_json::to_string(&response)?;
	writer.write_all(json_str.as_bytes()).await?;
	writer.write_all(b"\n").await?;
	writer.flush().await?;
	Ok(())
}

#[async_trait(?Send)]
impl ProtocolServer for ProtocolV3Server {
	fn base_path(&self) -> &Path {
		&self.fs_server.base_path
	}

	async fn handle_capabilities(&mut self) -> ProtocolResult<NodeCapabilities> {
		use crate::metadata::NodeCapabilities;
		Ok(NodeCapabilities::detect(Some(&self.fs_server.base_path)))
	}

	async fn handle_list(&mut self) -> ProtocolResult<Vec<FileSystemEntry>> {
		self.fs_server.list_directory().await
	}

	async fn handle_list_streaming(
		&mut self,
	) -> ProtocolResult<tokio::sync::mpsc::Receiver<ProtocolResult<FileSystemEntry>>> {
		// Override default to use streaming implementation instead of blocking list_directory()
		self.fs_server.list_directory_streaming()
	}

	async fn handle_write_metadata(&mut self, entry: &MetadataEntry) -> ProtocolResult<()> {
		self.fs_server.write_metadata(entry).await
	}

	async fn handle_delete(&mut self, path: &Path) -> ProtocolResult<()> {
		self.fs_server.delete_file(path).await
	}

	async fn handle_read_chunks(&mut self, hashes: &[String]) -> ProtocolResult<Vec<ChunkData>> {
		tracing::debug!(
			"[v3_server] handle_read_chunks called with {} requested hashes",
			hashes.len()
		);
		let result = self.fs_server.read_chunks(hashes).await;
		match &result {
			Ok(chunks) => {
				tracing::debug!(
					"[v3_server] handle_read_chunks returning {} chunks (requested: {})",
					chunks.len(),
					hashes.len()
				);
				if chunks.len() < hashes.len() {
					tracing::warn!(
						"[v3_server] MISMATCH: Server could only provide {}/{} chunks",
						chunks.len(),
						hashes.len()
					);
				}
			}
			Err(e) => {
				tracing::error!("[v3_server] handle_read_chunks failed: {}", e);
			}
		}
		result
	}

	async fn handle_write_chunk(&mut self, hash: &str, data: &[u8]) -> ProtocolResult<()> {
		self.fs_server.write_chunk(hash, data).await
	}

	async fn handle_commit(&mut self) -> ProtocolResult<CommitResponse> {
		self.fs_server.commit().await
	}
}

/// Serialize a FileSystemEntry to JSON5 format (for V3 protocol output)
fn serialize_entry_json5(entry: &FileSystemEntry) -> Result<String, serde_json::Error> {
	match entry.entry_type {
		FileSystemEntryType::File => {
			let obj = serde_json::json!({
				"typ": "F",
				"pth": entry.path.to_string_lossy().to_string(),
				"mod": entry.mode,
				"uid": entry.user_id,
				"gid": entry.group_id,
				"ct": entry.created_time as u64,
				"mt": entry.modified_time as u64,
				"sz": entry.size,
			});
			serde_json::to_string(&obj)
		}
		FileSystemEntryType::Directory => {
			let obj = serde_json::json!({
				"typ": "D",
				"pth": entry.path.to_string_lossy().to_string(),
				"mod": entry.mode,
				"uid": entry.user_id,
				"gid": entry.group_id,
				"ct": entry.created_time as u64,
				"mt": entry.modified_time as u64,
			});
			serde_json::to_string(&obj)
		}
		FileSystemEntryType::SymLink => {
			let obj = serde_json::json!({
				"typ": "S",
				"pth": entry.path.to_string_lossy().to_string(),
				"mod": entry.mode,
				"uid": entry.user_id,
				"gid": entry.group_id,
				"ct": entry.created_time as u64,
				"mt": entry.modified_time as u64,
				"tgt": entry.target.as_ref().map(|t| t.to_string_lossy().to_string()),
			});
			serde_json::to_string(&obj)
		}
	}
}

// vim: ts=4
