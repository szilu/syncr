//! Streaming abstractions for the collection phase
//!
//! This module provides types and utilities for streaming directory listings
//! instead of buffering all entries in memory.
//!
//! The streaming collection architecture enables:
//! - Reduced initial latency (entries sent as discovered)
//! - Lower memory usage (no buffering all entries)
//! - Potential for parallelization (future phases)

use crate::protocol::error::ProtocolError;
use crate::protocol::types::FileSystemEntry;
use tokio::sync::mpsc;

/// Maximum number of entries buffered in the streaming channel
/// This provides natural backpressure: if the receiver can't keep up,
/// the sender will be blocked from reading more entries from disk
pub const DEFAULT_CHANNEL_BUFFER_SIZE: usize = 100;

/// Result type for streaming operations
pub type StreamResult<T> = Result<T, ProtocolError>;

/// A single event in the directory listing stream
///
/// This can be either a successfully discovered entry or an error encountered
/// during directory traversal.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum ListingEvent {
	/// A file system entry was discovered and processed
	Entry(FileSystemEntry),

	/// An error occurred during processing
	///
	/// The stream continues after errors (recoverable) or may terminate
	/// based on error type and client decision.
	Error {
		/// The error message
		message: String,
		/// Whether this error is fatal (stops the stream)
		fatal: bool,
	},
}

/// Statistics about the listing operation
///
/// These track progress through the directory listing and can be used
/// for progress reporting or debugging.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Default)]
pub struct ListingStats {
	/// Number of files discovered and processed
	pub files_processed: u64,
	/// Number of directories traversed
	pub directories_processed: u64,
	/// Number of symlinks discovered
	pub symlinks_processed: u64,
	/// Total bytes in all discovered files
	pub total_bytes_processed: u64,
	/// Number of recoverable errors encountered
	pub errors_encountered: u32,
	/// Number of files skipped due to errors
	pub files_skipped: u32,
}

impl ListingStats {
	/// Total entries processed
	#[allow(dead_code)]
	pub fn total_entries(&self) -> u64 {
		self.files_processed + self.directories_processed + self.symlinks_processed
	}
}

/// Configuration for streaming collection behavior
#[derive(Debug, Clone)]
pub struct StreamingConfig {
	/// Size of the channel buffer (affects memory vs latency tradeoff)
	pub channel_buffer_size: usize,
	/// Maximum number of files to process concurrently (Phase 2+)
	pub max_concurrent_files: usize,
	/// Maximum memory to use for buffering (bytes)
	/// None = unlimited (not implemented yet)
	#[allow(dead_code)]
	pub max_buffer_memory: Option<usize>,
}

impl Default for StreamingConfig {
	fn default() -> Self {
		Self {
			channel_buffer_size: DEFAULT_CHANNEL_BUFFER_SIZE,
			max_concurrent_files: 8,
			max_buffer_memory: None,
		}
	}
}

/// Type alias for the receiver side of the listing stream
pub type ListingStream = mpsc::Receiver<StreamResult<FileSystemEntry>>;

/// Type alias for the sender side of the listing stream
pub type ListingSender = mpsc::Sender<StreamResult<FileSystemEntry>>;

/// Create a new streaming channel with configured buffer size
pub fn create_listing_channel(config: &StreamingConfig) -> (ListingSender, ListingStream) {
	mpsc::channel(config.channel_buffer_size)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_listing_stats_total_entries() {
		let stats = ListingStats {
			files_processed: 10,
			directories_processed: 5,
			symlinks_processed: 2,
			total_bytes_processed: 1000,
			errors_encountered: 0,
			files_skipped: 0,
		};

		assert_eq!(stats.total_entries(), 17);
	}

	#[test]
	fn test_streaming_config_default() {
		let config = StreamingConfig::default();
		assert_eq!(config.channel_buffer_size, DEFAULT_CHANNEL_BUFFER_SIZE);
		assert_eq!(config.max_concurrent_files, 8);
		assert!(config.max_buffer_memory.is_none());
	}

	#[test]
	fn test_create_listing_channel() {
		let config = StreamingConfig::default();
		let (tx, _rx) = create_listing_channel(&config);
		// If this doesn't panic, the channel was created successfully
		drop(tx);
	}
}

// vim: ts=4
