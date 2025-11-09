//! Protocol-agnostic types for sync communication
//!
//! These types are used by all protocol implementations to ensure
//! a consistent interface regardless of the underlying protocol version.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Protocol version identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolVersion {
	V3,
}

impl std::fmt::Display for ProtocolVersion {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			ProtocolVersion::V3 => write!(f, "3"),
		}
	}
}

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
}

/// Metadata entry to send during sync
#[derive(Debug, Clone)]
pub struct MetadataEntry {
	pub entry_type: FileSystemEntryType,
	pub path: PathBuf,
	pub mode: u32,
	pub user_id: u32,
	pub group_id: u32,
	pub created_time: u32,
	pub modified_time: u32,
	pub size: u64,
	pub target: Option<PathBuf>,
	pub chunks: Vec<ChunkInfo>,
	pub needs_data_transfer: bool,
}

/// Received chunk with binary data
#[derive(Debug)]
pub struct ChunkData {
	pub hash: String, // Base64-encoded hash for identification
	pub data: Vec<u8>,
}

/// Response from commit operation
#[derive(Debug)]
pub struct CommitResponse {
	pub success: bool,
	pub message: Option<String>,
	pub renamed_count: Option<usize>,
	pub failed_count: Option<usize>,
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_protocol_version_display() {
		assert_eq!(ProtocolVersion::V3.to_string(), "3");
	}
}

// vim: ts=4
