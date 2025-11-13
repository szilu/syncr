//! Protocol-agnostic types for sync communication
//!
//! These types are used by all protocol implementations to ensure
//! a consistent interface regardless of the underlying protocol version.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Re-export NodeCapabilities from metadata module for use in protocol
pub use crate::metadata::NodeCapabilities;

/// Type of file system entry
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileSystemEntryType {
	#[serde(rename = "F")]
	File,
	#[serde(rename = "D")]
	Directory,
	#[serde(rename = "S")]
	SymLink,
}

/// Information about a chunk in a file
#[derive(Debug, Clone)]
pub struct ChunkInfo {
	pub hash: [u8; 32], // Binary hash
	pub offset: u64,
	pub size: u32,
}

/// A file system entry (file, directory, or symlink) with metadata
///
/// This struct represents a file system entry with all its metadata and chunks.
/// The `needs_data_transfer` field indicates whether chunk data should be transferred
/// during sync operations (None = not applicable, Some(true) = needs transfer, Some(false) = doesn't need)
#[derive(Debug, Clone)]
pub struct FileSystemEntry {
	pub entry_type: FileSystemEntryType,
	pub path: PathBuf,
	pub mode: u32,
	pub user_id: u32,
	pub group_id: u32,
	pub created_time: u32,
	pub modified_time: u32,
	pub size: u64,
	pub target: Option<PathBuf>, // For symlinks
	pub chunks: Vec<ChunkInfo>,  // For files
	/// Whether chunk data needs to be transferred (merged from MetadataEntry)
	/// Default: None (not specified during normal traversal)
	pub needs_data_transfer: Option<bool>,
}

impl FileSystemEntry {
	/// Create a new FileSystemEntry without data transfer requirement
	#[allow(clippy::too_many_arguments)]
	pub fn new(
		entry_type: FileSystemEntryType,
		path: PathBuf,
		mode: u32,
		user_id: u32,
		group_id: u32,
		created_time: u32,
		modified_time: u32,
		size: u64,
		target: Option<PathBuf>,
		chunks: Vec<ChunkInfo>,
	) -> Self {
		Self {
			entry_type,
			path,
			mode,
			user_id,
			group_id,
			created_time,
			modified_time,
			size,
			target,
			chunks,
			needs_data_transfer: None,
		}
	}

	/// Create a new FileSystemEntry with explicit data transfer requirement
	#[allow(clippy::too_many_arguments)]
	pub fn with_data_transfer(
		entry_type: FileSystemEntryType,
		path: PathBuf,
		mode: u32,
		user_id: u32,
		group_id: u32,
		created_time: u32,
		modified_time: u32,
		size: u64,
		target: Option<PathBuf>,
		chunks: Vec<ChunkInfo>,
		needs_data_transfer: bool,
	) -> Self {
		Self {
			entry_type,
			path,
			mode,
			user_id,
			group_id,
			created_time,
			modified_time,
			size,
			target,
			chunks,
			needs_data_transfer: Some(needs_data_transfer),
		}
	}
}

/// Type alias for backward compatibility
/// MetadataEntry was merged into FileSystemEntry with needs_data_transfer as Option<bool>
pub type MetadataEntry = FileSystemEntry;

/// Received chunk with binary data
#[derive(Debug, Clone)]
pub struct ChunkData {
	pub hash: String, // Base64-encoded hash for identification
	pub data: Vec<u8>,
}

/// Response from commit operation
#[derive(Debug, Clone)]
pub struct CommitResponse {
	pub success: bool,
	pub message: Option<String>,
	pub renamed_count: Option<usize>,
	pub failed_count: Option<usize>,
}

// vim: ts=4
