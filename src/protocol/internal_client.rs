//! Internal protocol client using in-process channels
//!
//! This client communicates with a server in the same process using
//! tokio::sync::mpsc channels. This provides zero-copy, zero-serialization
//! communication for local file system operations.

use async_trait::async_trait;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::error::ProtocolError;
use super::messages::{ProtocolCommand, ProtocolResponse};
use super::traits::*;
use super::types::*;

/// Internal protocol client using in-process channels
pub struct ProtocolInternalClient {
	cmd_tx: tokio::sync::mpsc::Sender<ProtocolCommand>,
	response_rx: Arc<Mutex<tokio::sync::mpsc::Receiver<ProtocolResponse>>>,
	chunks: BTreeSet<[u8; 32]>,
	missing: Arc<Mutex<BTreeSet<String>>>,
}

impl ProtocolInternalClient {
	/// Create a new internal protocol client with the given channels
	pub fn new(
		cmd_tx: tokio::sync::mpsc::Sender<ProtocolCommand>,
		response_rx: tokio::sync::mpsc::Receiver<ProtocolResponse>,
	) -> Self {
		Self {
			cmd_tx,
			response_rx: Arc::new(Mutex::new(response_rx)),
			chunks: BTreeSet::new(),
			missing: Arc::new(Mutex::new(BTreeSet::new())),
		}
	}

	async fn send_command(&self, cmd: ProtocolCommand) -> ProtocolResult<()> {
		self.cmd_tx
			.send(cmd)
			.await
			.map_err(|e| ProtocolError::Other(format!("Channel send failed: {}", e)))
	}

	async fn recv_response(&self) -> ProtocolResult<ProtocolResponse> {
		self.response_rx
			.lock()
			.await
			.recv()
			.await
			.ok_or_else(|| ProtocolError::Other("Channel closed".to_string()))
	}
}

#[async_trait]
impl ProtocolClient for ProtocolInternalClient {
	fn protocol_name(&self) -> &str {
		"internal"
	}

	async fn request_capabilities(&mut self) -> ProtocolResult<NodeCapabilities> {
		self.send_command(ProtocolCommand::Capabilities).await?;

		match self.recv_response().await? {
			ProtocolResponse::Capabilities(caps) => Ok(caps),
			ProtocolResponse::Error(msg) => Err(ProtocolError::Other(msg)),
			_ => {
				Err(ProtocolError::Other("Unexpected response to capabilities request".to_string()))
			}
		}
	}

	async fn close(&mut self) -> ProtocolResult<()> {
		self.send_command(ProtocolCommand::Quit).await?;
		Ok(())
	}

	async fn request_listing(&mut self) -> ProtocolResult<()> {
		self.send_command(ProtocolCommand::List).await?;
		Ok(())
	}

	async fn receive_entry(&mut self) -> ProtocolResult<Option<FileSystemEntry>> {
		match self.recv_response().await? {
			ProtocolResponse::Entry(entry) => {
				// Track chunks for deduplication
				for chunk in &entry.chunks {
					self.chunks.insert(chunk.hash);
				}
				Ok(Some(entry))
			}
			ProtocolResponse::EndOfList => Ok(None),
			ProtocolResponse::Error(msg) => Err(ProtocolError::Other(msg)),
			_ => Err(ProtocolError::Other("Unexpected response during listing".to_string())),
		}
	}

	async fn begin_metadata_transfer(&mut self) -> ProtocolResult<()> {
		self.send_command(ProtocolCommand::BeginWrite).await?;
		Ok(())
	}

	async fn send_metadata(&mut self, entry: &MetadataEntry) -> ProtocolResult<()> {
		use tracing::debug;
		debug!(
			"[internal_client] send_metadata for {}: chunks={}, needs_data_transfer={:?}",
			entry.path.display(),
			entry.chunks.len(),
			entry.needs_data_transfer
		);
		self.send_command(ProtocolCommand::WriteMetadata(entry.clone())).await?;

		// Track missing chunks
		if entry.needs_data_transfer == Some(true) {
			let mut missing = self.missing.lock().await;
			for chunk in &entry.chunks {
				if !self.chunks.contains(&chunk.hash) {
					let hash_b64 = crate::util::hash_to_base64(&chunk.hash);
					missing.insert(hash_b64);
				}
			}
		}

		// Receive response from server
		match self.recv_response().await? {
			ProtocolResponse::Ok | ProtocolResponse::WriteOk => Ok(()),
			ProtocolResponse::Error(msg) => Err(ProtocolError::Other(msg)),
			_ => Err(ProtocolError::Other("Unexpected response to send_metadata".to_string())),
		}
	}

	async fn send_delete(&mut self, path: &Path) -> ProtocolResult<()> {
		self.send_command(ProtocolCommand::Delete(path.to_path_buf())).await?;

		// Receive response from server
		match self.recv_response().await? {
			ProtocolResponse::Ok | ProtocolResponse::WriteOk => Ok(()),
			ProtocolResponse::Error(msg) => Err(ProtocolError::Other(msg)),
			_ => Err(ProtocolError::Other("Unexpected response to send_delete".to_string())),
		}
	}

	async fn end_metadata_transfer(&mut self) -> ProtocolResult<()> {
		self.send_command(ProtocolCommand::EndWrite).await?;

		match self.recv_response().await? {
			ProtocolResponse::WriteOk | ProtocolResponse::Ok => Ok(()),
			ProtocolResponse::Error(msg) => Err(ProtocolError::Other(msg)),
			_ => Err(ProtocolError::Other("Unexpected response to end write".to_string())),
		}
	}

	async fn begin_chunk_transfer(&mut self) -> ProtocolResult<()> {
		self.send_command(ProtocolCommand::BeginRead).await?;
		Ok(())
	}

	async fn request_chunks(&mut self, chunk_hashes: &[String]) -> ProtocolResult<()> {
		self.send_command(ProtocolCommand::RequestChunks(chunk_hashes.to_vec())).await?;
		Ok(())
	}

	async fn receive_chunk(&mut self) -> ProtocolResult<Option<ChunkData>> {
		match self.recv_response().await? {
			ProtocolResponse::Chunk(chunk) => Ok(Some(chunk)),
			ProtocolResponse::EndOfChunks => Ok(None),
			ProtocolResponse::Error(msg) => Err(ProtocolError::Other(msg)),
			_ => Err(ProtocolError::Other("Unexpected response during chunk transfer".to_string())),
		}
	}

	async fn send_chunk(&mut self, hash: &str, data: &[u8]) -> ProtocolResult<()> {
		self.send_command(ProtocolCommand::SendChunk {
			hash: hash.to_string(),
			data: data.to_vec(),
		})
		.await?;

		// Receive response from server
		match self.recv_response().await? {
			ProtocolResponse::Ok | ProtocolResponse::WriteOk => Ok(()),
			ProtocolResponse::Error(msg) => Err(ProtocolError::Other(msg)),
			_ => Err(ProtocolError::Other("Unexpected response to send_chunk".to_string())),
		}
	}

	async fn end_chunk_transfer(&mut self) -> ProtocolResult<()> {
		self.send_command(ProtocolCommand::EndRead).await?;
		Ok(())
	}

	async fn commit(&mut self) -> ProtocolResult<CommitResponse> {
		self.send_command(ProtocolCommand::Commit).await?;

		match self.recv_response().await? {
			ProtocolResponse::CommitResult(result) => Ok(result),
			ProtocolResponse::Error(msg) => Err(ProtocolError::Other(msg)),
			_ => Err(ProtocolError::Other("Unexpected response to commit".to_string())),
		}
	}

	fn has_chunk(&self, hash: &[u8; 32]) -> bool {
		self.chunks.contains(hash)
	}

	fn mark_chunk_missing(&self, hash: String) {
		let missing = self.missing.clone();
		tokio::spawn(async move {
			missing.lock().await.insert(hash);
		});
	}

	fn missing_chunk_count(&self) -> usize {
		0 // Would need to be async to get actual count
	}

	async fn get_missing_chunks(&self) -> Vec<String> {
		self.missing.lock().await.iter().cloned().collect()
	}

	fn clear_missing_chunks(&self) {
		let missing = self.missing.clone();
		tokio::spawn(async move {
			missing.lock().await.clear();
		});
	}
}

// vim: ts=4
