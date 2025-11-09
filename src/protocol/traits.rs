//! Core protocol trait defining the sync communication interface
//!
//! All protocol implementations (V2, V3, future versions) must implement
//! this trait to provide the sync engine with protocol operations.

use async_trait::async_trait;
use std::path::Path;

use super::error::ProtocolError;
use super::types::*;

/// Result type for protocol operations
pub type ProtocolResult<T> = Result<T, ProtocolError>;

/// Core trait defining all protocol operations for sync communication
///
/// This trait abstracts all protocol-specific details and provides a unified
/// interface that the sync engine can use regardless of protocol version.
/// The sync logic depends only on this trait, never on specific protocol versions.
#[async_trait]
pub trait SyncProtocol: Send + Sync {
	// === Metadata ===

	/// Get protocol version identifier
	fn version(&self) -> ProtocolVersion;

	// === Lifecycle ===

	/// Gracefully close the protocol connection
	async fn close(&mut self) -> ProtocolResult<()>;

	// === Collection Phase (LIST) ===

	/// Request directory listing from remote node
	async fn request_listing(&mut self) -> ProtocolResult<()>;

	/// Receive and parse the next entry in directory listing
	/// Returns None when listing is complete
	async fn receive_entry(&mut self) -> ProtocolResult<Option<FileSystemEntry>>;

	// === Metadata Phase (WRITE) ===

	/// Enter WRITE mode for sending file metadata
	async fn begin_metadata_transfer(&mut self) -> ProtocolResult<()>;

	/// Send file/directory/symlink metadata to remote
	async fn send_metadata(&mut self, entry: &MetadataEntry) -> ProtocolResult<()>;

	/// Send file deletion command
	async fn send_delete(&mut self, path: &Path) -> ProtocolResult<()>;

	/// Exit WRITE mode
	async fn end_metadata_transfer(&mut self) -> ProtocolResult<()>;

	// === Chunk Phase (READ) ===

	/// Enter READ mode for chunk transfer
	async fn begin_chunk_transfer(&mut self) -> ProtocolResult<()>;

	/// Request specific chunks from remote
	async fn request_chunks(&mut self, chunk_hashes: &[String]) -> ProtocolResult<()>;

	/// Receive next chunk from remote
	/// Returns None when all requested chunks received
	async fn receive_chunk(&mut self) -> ProtocolResult<Option<ChunkData>>;

	/// Send chunk data to remote
	async fn send_chunk(&mut self, hash: &str, data: &[u8]) -> ProtocolResult<()>;

	/// Exit READ mode
	async fn end_chunk_transfer(&mut self) -> ProtocolResult<()>;

	// === Commit Phase ===

	/// Send COMMIT command to finalize all changes
	async fn commit(&mut self) -> ProtocolResult<CommitResponse>;

	// === Utility Methods ===

	/// Check if a chunk is available locally
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_protocol_version_values() {
		let v3 = ProtocolVersion::V3;
		assert_eq!(v3, ProtocolVersion::V3);
	}
}

// vim: ts=4
