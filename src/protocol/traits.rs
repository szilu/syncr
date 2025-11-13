//! Core protocol traits defining the sync communication interface
//!
//! All protocol implementations (V2, V3, future versions) must implement
//! these traits to provide the sync engine with protocol operations.
//!
//! # Architecture
//!
//! We use two complementary traits:
//! - `ProtocolClient`: Used by sync orchestrator to communicate with a remote/local node
//! - `ProtocolServer`: Implemented by nodes to handle incoming protocol requests
//!
//! Both are implemented by protocol-specific types (e.g., V3Client, V3Server).

use async_trait::async_trait;
use std::path::Path;

use super::error::ProtocolError;
use super::types::*;

/// Result type for protocol operations
pub type ProtocolResult<T> = Result<T, ProtocolError>;

/// Client-side protocol interface used by sync orchestrator
///
/// This trait abstracts all protocol-specific details on the client side
/// and provides a unified interface that the sync engine can use regardless
/// of protocol version or transport (local channels vs remote JSON5).
///
/// Implementations: ProtocolV3Client (JSON5/pipes), ProtocolInternalClient (channels)
#[async_trait]
pub trait ProtocolClient: Send + Sync {
	// === Metadata ===

	/// Get protocol implementation identifier
	fn protocol_name(&self) -> &str;

	/// Request node capabilities (supported metadata operations)
	async fn request_capabilities(&mut self) -> ProtocolResult<super::types::NodeCapabilities>;

	// === Lifecycle ===

	/// Gracefully close the protocol connection
	async fn close(&mut self) -> ProtocolResult<()>;

	// === Collection Phase (LIST) ===

	/// Request directory listing from node
	async fn request_listing(&mut self) -> ProtocolResult<()>;

	/// Receive and parse the next entry in directory listing
	/// Returns None when listing is complete
	async fn receive_entry(&mut self) -> ProtocolResult<Option<FileSystemEntry>>;

	// === Metadata Phase (WRITE) ===

	/// Enter WRITE mode for sending file metadata
	async fn begin_metadata_transfer(&mut self) -> ProtocolResult<()>;

	/// Send file/directory/symlink metadata to node
	async fn send_metadata(&mut self, entry: &MetadataEntry) -> ProtocolResult<()>;

	/// Send file deletion command
	async fn send_delete(&mut self, path: &Path) -> ProtocolResult<()>;

	/// Exit WRITE mode
	async fn end_metadata_transfer(&mut self) -> ProtocolResult<()>;

	// === Chunk Phase (READ) ===

	/// Enter READ mode for chunk transfer
	async fn begin_chunk_transfer(&mut self) -> ProtocolResult<()>;

	/// Request specific chunks from node
	async fn request_chunks(&mut self, chunk_hashes: &[String]) -> ProtocolResult<()>;

	/// Receive next chunk from node
	/// Returns None when all requested chunks received
	async fn receive_chunk(&mut self) -> ProtocolResult<Option<ChunkData>>;

	/// Send chunk data to node
	async fn send_chunk(&mut self, hash: &str, data: &[u8]) -> ProtocolResult<()>;

	/// Exit READ mode
	async fn end_chunk_transfer(&mut self) -> ProtocolResult<()>;

	// === Commit Phase ===

	/// Send COMMIT command to finalize all changes
	async fn commit(&mut self) -> ProtocolResult<CommitResponse>;

	// === Utility Methods ===

	/// Check if a chunk is available locally (on the client/orchestrator side)
	fn has_chunk(&self, hash: &[u8; 32]) -> bool;

	/// Mark chunks as missing (need transfer)
	fn mark_chunk_missing(&self, hash: String);

	/// Get count of missing chunks
	fn missing_chunk_count(&self) -> usize;

	/// Get list of missing chunk hashes (base64-encoded)
	async fn get_missing_chunks(&self) -> Vec<String>;

	/// Clear the missing chunks set
	fn clear_missing_chunks(&self);
}

/// Server-side protocol handler
///
/// This trait defines the business logic operations that nodes must implement.
/// The actual I/O (channels vs JSON5) is handled by protocol-specific implementations.
///
/// Implementations: ProtocolV3Server (JSON5/pipes), ProtocolInternalServer (channels)
///
/// Note: ProtocolServer doesn't require Send or Sync because servers are
/// always run in a single task and don't need to be shared across threads.
/// The DumpState struct uses RefCell which isn't Send/Sync, so we can't require them.
/// We use #[async_trait(?Send)] to allow non-Send futures.
#[async_trait(?Send)]
pub trait ProtocolServer {
	/// Get base directory being served
	#[allow(dead_code)]
	fn base_path(&self) -> &Path;

	/// Handle capabilities request
	async fn handle_capabilities(&mut self) -> ProtocolResult<NodeCapabilities>;

	/// Handle listing request - returns all entries
	async fn handle_list(&mut self) -> ProtocolResult<Vec<FileSystemEntry>>;

	/// Handle metadata write - processes incoming metadata entry
	async fn handle_write_metadata(&mut self, entry: &MetadataEntry) -> ProtocolResult<()>;

	/// Handle file deletion
	async fn handle_delete(&mut self, path: &Path) -> ProtocolResult<()>;

	/// Handle chunk read request - returns requested chunks
	async fn handle_read_chunks(&mut self, hashes: &[String]) -> ProtocolResult<Vec<ChunkData>>;

	/// Handle incoming chunk data
	async fn handle_write_chunk(&mut self, hash: &str, data: &[u8]) -> ProtocolResult<()>;

	/// Handle commit - rename temp files to final locations
	async fn handle_commit(&mut self) -> ProtocolResult<CommitResponse>;

	/// Check if a chunk is available
	#[allow(dead_code)]
	fn has_chunk(&self, hash: &[u8; 32]) -> bool;
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_protocol_client_trait_exists() {
		// This test verifies the trait is properly defined
		// Actual tests of implementations are in their respective modules
		let _: &dyn ProtocolClient;
	}

	#[test]
	fn test_protocol_server_trait_exists() {
		// This test verifies the trait is properly defined
		let _: &dyn ProtocolServer;
	}
}

// vim: ts=4
