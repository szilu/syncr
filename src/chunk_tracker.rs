//! Unified chunk tracking system for synchronization
//!
//! This module provides a centralized way to track chunks across multiple nodes
//! and manage chunk transfers during synchronization.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

/// Error type for chunk operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkTrackerError {
	/// Chunk not found in tracker
	ChunkNotFound(String),
	/// Invalid operation for current state
	InvalidState(String),
	/// Node not found
	NodeNotFound(u8),
}

impl fmt::Display for ChunkTrackerError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			ChunkTrackerError::ChunkNotFound(hash) => write!(f, "Chunk not found: {}", hash),
			ChunkTrackerError::InvalidState(msg) => write!(f, "Invalid state: {}", msg),
			ChunkTrackerError::NodeNotFound(node_id) => write!(f, "Node not found: {}", node_id),
		}
	}
}

impl Error for ChunkTrackerError {}

/// Status of a chunk transfer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferStatus {
	/// Transfer pending, not yet started
	Pending,
	/// Transfer in progress
	InProgress,
	/// Transfer completed successfully
	Completed,
	/// Transfer failed
	Failed,
}

/// Information about a chunk available from a node
#[derive(Clone, Debug)]
pub struct ChunkLocation {
	/// Node ID that has this chunk
	pub node_id: u8,
	/// Byte offset within a file (if applicable)
	pub offset: u64,
	/// Byte size of chunk
	pub size: u32,
}

/// Central chunk tracking system
#[derive(Debug)]
pub struct ChunkTracker {
	/// Local chunks available on primary node
	/// Maps hash (base64) -> list of locations
	local_chunks: BTreeMap<String, Vec<ChunkLocation>>,

	/// Remote chunks available on other nodes
	/// Maps hash (base64) -> list of locations
	remote_chunks: BTreeMap<String, Vec<ChunkLocation>>,

	/// Chunks that are missing and need to be transferred
	missing_chunks: BTreeSet<String>,

	/// Pending chunk transfers
	/// Maps hash (base64) -> transfer status
	pending_transfers: BTreeMap<String, TransferStatus>,
}

impl ChunkTracker {
	/// Create a new chunk tracker
	pub fn new() -> Self {
		ChunkTracker {
			local_chunks: BTreeMap::new(),
			remote_chunks: BTreeMap::new(),
			missing_chunks: BTreeSet::new(),
			pending_transfers: BTreeMap::new(),
		}
	}

	/// Register chunks available locally
	pub fn add_local_chunks(&mut self, chunks: Vec<(String, ChunkLocation)>) {
		for (hash, location) in chunks {
			self.local_chunks.entry(hash.clone()).or_default().push(location);
			self.missing_chunks.remove(&hash);
		}
	}

	/// Register chunks available on a remote node
	pub fn add_remote_chunks(&mut self, chunks: Vec<(String, ChunkLocation)>) {
		for (hash, location) in chunks {
			self.remote_chunks.entry(hash).or_default().push(location);
		}
	}

	/// Mark chunks as missing (need to be transferred)
	pub fn mark_missing(&mut self, hashes: Vec<String>) {
		for hash in hashes {
			if !self.local_chunks.contains_key(&hash) && !self.remote_chunks.contains_key(&hash) {
				self.missing_chunks.insert(hash);
			}
		}
	}

	/// Get all missing chunks
	pub fn get_missing_chunks(&self) -> Vec<String> {
		self.missing_chunks.iter().cloned().collect()
	}

	/// Find sources for a chunk (can come from local or remote)
	pub fn get_chunk_sources(&self, hash: &str) -> Vec<ChunkLocation> {
		let mut sources = Vec::new();

		if let Some(local) = self.local_chunks.get(hash) {
			sources.extend(local.iter().cloned());
		}

		if let Some(remote) = self.remote_chunks.get(hash) {
			sources.extend(remote.iter().cloned());
		}

		sources
	}

	/// Check if we have a chunk locally
	pub fn has_chunk_locally(&self, hash: &str) -> bool {
		self.local_chunks.contains_key(hash)
	}

	/// Check if we know where a chunk is (local or remote)
	pub fn is_chunk_available(&self, hash: &str) -> bool {
		self.local_chunks.contains_key(hash) || self.remote_chunks.contains_key(hash)
	}

	/// Register a chunk transfer as pending
	pub fn start_transfer(&mut self, hash: String) -> Result<(), ChunkTrackerError> {
		if !self.is_chunk_available(&hash) {
			return Err(ChunkTrackerError::ChunkNotFound(hash));
		}
		self.pending_transfers.insert(hash, TransferStatus::InProgress);
		Ok(())
	}

	/// Mark a chunk transfer as completed
	pub fn mark_transferred(&mut self, hash: &str) -> Result<(), ChunkTrackerError> {
		if !self.pending_transfers.contains_key(hash) {
			return Err(ChunkTrackerError::ChunkNotFound(hash.to_string()));
		}
		self.pending_transfers.insert(hash.to_string(), TransferStatus::Completed);
		Ok(())
	}

	/// Get transfer status for a chunk
	pub fn get_transfer_status(&self, hash: &str) -> Option<TransferStatus> {
		self.pending_transfers.get(hash).copied()
	}

	/// Get statistics about tracked chunks
	pub fn stats(&self) -> ChunkStats {
		ChunkStats {
			total_local_chunks: self.local_chunks.len(),
			total_remote_chunks: self.remote_chunks.len(),
			missing_chunks: self.missing_chunks.len(),
			pending_transfers: self.pending_transfers.len(),
			completed_transfers: self
				.pending_transfers
				.values()
				.filter(|s| **s == TransferStatus::Completed)
				.count(),
		}
	}

	/// Clear all tracking information
	pub fn clear(&mut self) {
		self.local_chunks.clear();
		self.remote_chunks.clear();
		self.missing_chunks.clear();
		self.pending_transfers.clear();
	}

	/// Get all available chunks (local + remote)
	pub fn get_all_available_chunks(&self) -> Vec<String> {
		let mut chunks = Vec::new();
		for hash in self.local_chunks.keys() {
			chunks.push(hash.clone());
		}
		for hash in self.remote_chunks.keys() {
			if !chunks.contains(hash) {
				chunks.push(hash.clone());
			}
		}
		chunks.sort();
		chunks
	}

	/// Get deduplication statistics
	pub fn dedup_stats(&self) -> DedupStats {
		let total_available = self.get_all_available_chunks().len();
		let unique_in_local = self.local_chunks.len();
		let unique_in_remote = self.remote_chunks.len();

		DedupStats {
			total_unique_chunks: total_available,
			chunks_in_local: unique_in_local,
			chunks_in_remote: unique_in_remote,
			chunks_everywhere: self
				.local_chunks
				.keys()
				.filter(|h| self.remote_chunks.contains_key(*h))
				.count(),
		}
	}
}

impl Default for ChunkTracker {
	fn default() -> Self {
		Self::new()
	}
}

/// Statistics about tracked chunks
#[derive(Debug, Clone)]
pub struct ChunkStats {
	/// Total local chunks
	pub total_local_chunks: usize,
	/// Total remote chunks
	pub total_remote_chunks: usize,
	/// Missing chunks that need transfer
	pub missing_chunks: usize,
	/// Pending transfers in progress
	pub pending_transfers: usize,
	/// Completed transfers
	pub completed_transfers: usize,
}

/// Deduplication statistics
#[derive(Debug, Clone)]
pub struct DedupStats {
	/// Total unique chunks tracked
	pub total_unique_chunks: usize,
	/// Chunks available in local storage
	pub chunks_in_local: usize,
	/// Chunks available in remote storage
	pub chunks_in_remote: usize,
	/// Chunks available in both local and remote
	pub chunks_everywhere: usize,
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_chunk_tracker_creation() {
		let tracker = ChunkTracker::new();
		assert_eq!(tracker.get_missing_chunks().len(), 0);
		assert_eq!(tracker.get_all_available_chunks().len(), 0);
	}

	#[test]
	fn test_add_local_chunks() {
		let mut tracker = ChunkTracker::new();
		let chunks =
			vec![("hash1".to_string(), ChunkLocation { node_id: 0, offset: 0, size: 1024 })];

		tracker.add_local_chunks(chunks);

		assert!(tracker.has_chunk_locally("hash1"));
		assert!(tracker.is_chunk_available("hash1"));
		assert_eq!(tracker.get_all_available_chunks().len(), 1);
	}

	#[test]
	fn test_add_remote_chunks() {
		let mut tracker = ChunkTracker::new();
		let chunks =
			vec![("hash1".to_string(), ChunkLocation { node_id: 1, offset: 0, size: 1024 })];

		tracker.add_remote_chunks(chunks);

		assert!(!tracker.has_chunk_locally("hash1"));
		assert!(tracker.is_chunk_available("hash1"));
		assert_eq!(tracker.get_chunk_sources("hash1").len(), 1);
	}

	#[test]
	fn test_mark_missing() {
		let mut tracker = ChunkTracker::new();
		tracker.mark_missing(vec!["missing1".to_string(), "missing2".to_string()]);

		assert_eq!(tracker.get_missing_chunks().len(), 2);
		assert!(!tracker.is_chunk_available("missing1"));
	}

	#[test]
	fn test_missing_removed_when_added() {
		let mut tracker = ChunkTracker::new();
		tracker.mark_missing(vec!["hash1".to_string()]);

		assert!(tracker.get_missing_chunks().contains(&"hash1".to_string()));

		// Add it as local chunk
		tracker.add_local_chunks(vec![(
			"hash1".to_string(),
			ChunkLocation { node_id: 0, offset: 0, size: 1024 },
		)]);

		// Should no longer be in missing
		assert!(!tracker.get_missing_chunks().contains(&"hash1".to_string()));
		assert!(tracker.has_chunk_locally("hash1"));
	}

	#[test]
	fn test_transfer_tracking() {
		let mut tracker = ChunkTracker::new();

		// Add a local chunk
		tracker.add_local_chunks(vec![(
			"hash1".to_string(),
			ChunkLocation { node_id: 0, offset: 0, size: 1024 },
		)]);

		// Start transfer
		assert!(tracker.start_transfer("hash1".to_string()).is_ok());
		assert_eq!(tracker.get_transfer_status("hash1"), Some(TransferStatus::InProgress));

		// Mark as transferred
		assert!(tracker.mark_transferred("hash1").is_ok());
		assert_eq!(tracker.get_transfer_status("hash1"), Some(TransferStatus::Completed));
	}

	#[test]
	fn test_transfer_nonexistent_chunk() {
		let mut tracker = ChunkTracker::new();

		// Try to transfer nonexistent chunk
		let result = tracker.start_transfer("nonexistent".to_string());
		assert!(result.is_err());
	}

	#[test]
	fn test_stats() {
		let mut tracker = ChunkTracker::new();

		tracker.add_local_chunks(vec![(
			"hash1".to_string(),
			ChunkLocation { node_id: 0, offset: 0, size: 1024 },
		)]);

		tracker.add_remote_chunks(vec![(
			"hash2".to_string(),
			ChunkLocation { node_id: 1, offset: 0, size: 2048 },
		)]);

		tracker.mark_missing(vec!["hash3".to_string()]);

		let stats = tracker.stats();
		assert_eq!(stats.total_local_chunks, 1);
		assert_eq!(stats.total_remote_chunks, 1);
		assert_eq!(stats.missing_chunks, 1);
	}

	#[test]
	fn test_get_chunk_sources() {
		let mut tracker = ChunkTracker::new();

		tracker.add_local_chunks(vec![(
			"hash1".to_string(),
			ChunkLocation { node_id: 0, offset: 0, size: 1024 },
		)]);

		tracker.add_remote_chunks(vec![(
			"hash1".to_string(),
			ChunkLocation { node_id: 1, offset: 0, size: 1024 },
		)]);

		let sources = tracker.get_chunk_sources("hash1");
		assert_eq!(sources.len(), 2);
	}

	#[test]
	fn test_dedup_stats() {
		let mut tracker = ChunkTracker::new();

		// Add same chunk locally and remotely
		tracker.add_local_chunks(vec![(
			"hash1".to_string(),
			ChunkLocation { node_id: 0, offset: 0, size: 1024 },
		)]);

		tracker.add_remote_chunks(vec![(
			"hash1".to_string(),
			ChunkLocation { node_id: 1, offset: 0, size: 1024 },
		)]);

		// Add unique local chunk
		tracker.add_local_chunks(vec![(
			"hash2".to_string(),
			ChunkLocation { node_id: 0, offset: 1024, size: 1024 },
		)]);

		let dedup = tracker.dedup_stats();
		assert_eq!(dedup.total_unique_chunks, 2);
		assert_eq!(dedup.chunks_in_local, 2);
		assert_eq!(dedup.chunks_in_remote, 1);
		assert_eq!(dedup.chunks_everywhere, 1);
	}
}
