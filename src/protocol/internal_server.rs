//! Internal protocol server using in-process channels
//!
//! This server runs in the same process as the client and handles
//! protocol commands via tokio::sync::mpsc channels. It delegates to
//! FileSystemServer for actual file operations.

use async_trait::async_trait;
use std::path::PathBuf;
use tokio::sync::mpsc;

use super::file_operations::FileSystemServer;
use super::messages::{ProtocolCommand, ProtocolResponse};
use super::traits::*;
use super::types::*;
use crate::serve::DumpState;

/// Internal protocol server using in-process channels
pub struct ProtocolInternalServer {
	fs_server: FileSystemServer,
	cmd_rx: mpsc::Receiver<ProtocolCommand>,
	response_tx: mpsc::Sender<ProtocolResponse>,
}

impl ProtocolInternalServer {
	/// Create a new internal protocol server
	pub fn new(
		base_path: PathBuf,
		state: DumpState,
		cmd_rx: mpsc::Receiver<ProtocolCommand>,
		response_tx: mpsc::Sender<ProtocolResponse>,
	) -> Self {
		Self { fs_server: FileSystemServer::new(base_path, state), cmd_rx, response_tx }
	}

	/// Run the server loop, processing commands until Quit
	pub async fn run(mut self) -> ProtocolResult<()> {
		while let Some(cmd) = self.cmd_rx.recv().await {
			match cmd {
				ProtocolCommand::Capabilities => match self.handle_capabilities().await {
					Ok(caps) => {
						let _ = self.response_tx.send(ProtocolResponse::Capabilities(caps)).await;
					}
					Err(e) => {
						let _ = self.response_tx.send(ProtocolResponse::Error(e.to_string())).await;
					}
				},

				ProtocolCommand::List => match self.handle_list().await {
					Ok(entries) => {
						for entry in entries {
							let _ = self.response_tx.send(ProtocolResponse::Entry(entry)).await;
						}
						let _ = self.response_tx.send(ProtocolResponse::EndOfList).await;
					}
					Err(e) => {
						let _ = self.response_tx.send(ProtocolResponse::Error(e.to_string())).await;
					}
				},

				ProtocolCommand::BeginWrite => {
					// No response needed, just mode transition
				}

				ProtocolCommand::WriteMetadata(entry) => {
					match self.handle_write_metadata(&entry).await {
						Ok(_) => {
							let _ = self.response_tx.send(ProtocolResponse::Ok).await;
						}
						Err(e) => {
							let _ =
								self.response_tx.send(ProtocolResponse::Error(e.to_string())).await;
						}
					}
				}

				ProtocolCommand::Delete(path) => match self.handle_delete(&path).await {
					Ok(_) => {
						let _ = self.response_tx.send(ProtocolResponse::Ok).await;
					}
					Err(e) => {
						let _ = self.response_tx.send(ProtocolResponse::Error(e.to_string())).await;
					}
				},

				ProtocolCommand::EndWrite => {
					let _ = self.response_tx.send(ProtocolResponse::WriteOk).await;
				}

				ProtocolCommand::BeginRead => {
					// No response needed, just mode transition
				}

				ProtocolCommand::RequestChunks(hashes) => {
					match self.handle_read_chunks(&hashes).await {
						Ok(chunks) => {
							for chunk in chunks {
								let _ = self.response_tx.send(ProtocolResponse::Chunk(chunk)).await;
							}
							let _ = self.response_tx.send(ProtocolResponse::EndOfChunks).await;
						}
						Err(e) => {
							let _ =
								self.response_tx.send(ProtocolResponse::Error(e.to_string())).await;
						}
					}
				}

				ProtocolCommand::SendChunk { hash, data } => {
					match self.handle_write_chunk(&hash, &data).await {
						Ok(_) => {
							let _ = self.response_tx.send(ProtocolResponse::Ok).await;
						}
						Err(e) => {
							let _ =
								self.response_tx.send(ProtocolResponse::Error(e.to_string())).await;
						}
					}
				}

				ProtocolCommand::EndRead => {
					// No response needed
				}

				ProtocolCommand::Commit => match self.handle_commit().await {
					Ok(result) => {
						let _ = self.response_tx.send(ProtocolResponse::CommitResult(result)).await;
					}
					Err(e) => {
						let _ = self.response_tx.send(ProtocolResponse::Error(e.to_string())).await;
					}
				},

				ProtocolCommand::Quit => {
					let _ = self.response_tx.send(ProtocolResponse::Ok).await;
					break;
				}
			}
		}

		Ok(())
	}
}

#[async_trait(?Send)]
impl ProtocolServer for ProtocolInternalServer {
	fn base_path(&self) -> &std::path::Path {
		&self.fs_server.base_path
	}

	async fn handle_capabilities(&mut self) -> ProtocolResult<NodeCapabilities> {
		use crate::metadata::NodeCapabilities;
		Ok(NodeCapabilities::detect(Some(&self.fs_server.base_path)))
	}

	async fn handle_list(&mut self) -> ProtocolResult<Vec<FileSystemEntry>> {
		self.fs_server.list_directory().await
	}

	async fn handle_write_metadata(&mut self, entry: &MetadataEntry) -> ProtocolResult<()> {
		use tracing::debug;
		debug!(
			"[internal] handle_write_metadata for {}: chunks={}",
			entry.path.display(),
			entry.chunks.len()
		);
		self.fs_server.write_metadata(entry).await
	}

	async fn handle_delete(&mut self, path: &std::path::Path) -> ProtocolResult<()> {
		self.fs_server.delete_file(path).await
	}

	async fn handle_read_chunks(&mut self, hashes: &[String]) -> ProtocolResult<Vec<ChunkData>> {
		self.fs_server.read_chunks(hashes).await
	}

	async fn handle_write_chunk(&mut self, hash: &str, data: &[u8]) -> ProtocolResult<()> {
		self.fs_server.write_chunk(hash, data).await
	}

	async fn handle_commit(&mut self) -> ProtocolResult<CommitResponse> {
		self.fs_server.commit().await
	}

	fn has_chunk(&self, hash: &[u8; 32]) -> bool {
		self.fs_server.has_chunk(hash)
	}
}

// vim: ts=4
