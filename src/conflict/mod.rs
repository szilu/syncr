//! Conflict detection and resolution

use crate::types::FileData;
use std::path::PathBuf;

pub mod resolver;
pub mod rules;

pub use resolver::ConflictResolver;

// Re-export the error type

/// Represents a sync conflict between nodes
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Conflict {
	/// Unique conflict identifier
	pub id: u64,

	/// File path where conflict occurred
	pub path: PathBuf,

	/// Type of conflict
	pub conflict_type: ConflictType,

	/// Competing versions from different nodes
	pub versions: Vec<FileVersion>,
}

/// Types of conflicts that can occur
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ConflictType {
	/// File modified differently on multiple nodes
	ModifyModify,

	/// File deleted on one node, modified on another
	DeleteModify,

	/// File created with different content on multiple nodes
	CreateCreate,

	/// File vs directory conflict
	TypeMismatch,
}

/// A specific version of a file in a conflict
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FileVersion {
	/// Which node has this version
	pub node_index: usize,

	/// Node location string
	pub node_location: String,

	/// File metadata and chunks
	pub file_data: FileData,
}

impl FileVersion {
	/// Get the modification time
	#[allow(dead_code)]
	pub fn mtime(&self) -> u32 {
		self.file_data.mtime
	}

	/// Get the file size
	#[allow(dead_code)]
	pub fn size(&self) -> u64 {
		self.file_data.size
	}
}

impl Conflict {
	/// Create a new conflict
	#[allow(dead_code)]
	pub fn new(
		id: u64,
		path: PathBuf,
		conflict_type: ConflictType,
		versions: Vec<FileVersion>,
	) -> Self {
		Conflict { id, path, conflict_type, versions }
	}

	/// Get the number of conflicting versions
	#[allow(dead_code)]
	pub fn version_count(&self) -> usize {
		self.versions.len()
	}

	/// Find the version with the newest modification time
	#[allow(dead_code)]
	pub fn newest_version(&self) -> Option<usize> {
		self.versions.iter().enumerate().max_by_key(|(_, v)| v.mtime()).map(|(i, _)| i)
	}

	/// Find the version with the oldest modification time
	#[allow(dead_code)]
	pub fn oldest_version(&self) -> Option<usize> {
		self.versions.iter().enumerate().min_by_key(|(_, v)| v.mtime()).map(|(i, _)| i)
	}

	/// Find the version with the largest file
	#[allow(dead_code)]
	pub fn largest_version(&self) -> Option<usize> {
		self.versions.iter().enumerate().max_by_key(|(_, v)| v.size()).map(|(i, _)| i)
	}

	/// Find the version with the smallest file
	#[allow(dead_code)]
	pub fn smallest_version(&self) -> Option<usize> {
		self.versions.iter().enumerate().min_by_key(|(_, v)| v.size()).map(|(i, _)| i)
	}

	/// Find a version by node name/location
	#[allow(dead_code)]
	pub fn version_by_name(&self, name: &str) -> Option<usize> {
		self.versions
			.iter()
			.enumerate()
			.find(|(_, v)| v.node_location == name)
			.map(|(i, _)| i)
	}
}

/// Error type for conflict resolution
#[derive(Debug)]
pub enum ConflictResolutionError {
	/// No versions available
	NoVersions,

	/// Invalid version index
	InvalidVersion(usize),

	/// Node not found
	NodeNotFound(String),

	/// Strategy cannot be applied
	StrategyNotApplicable(String),
}

impl std::fmt::Display for ConflictResolutionError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			ConflictResolutionError::NoVersions => write!(f, "No versions available"),
			ConflictResolutionError::InvalidVersion(idx) => {
				write!(f, "Invalid version index: {}", idx)
			}
			ConflictResolutionError::NodeNotFound(name) => write!(f, "Node not found: {}", name),
			ConflictResolutionError::StrategyNotApplicable(msg) => {
				write!(f, "Strategy not applicable: {}", msg)
			}
		}
	}
}

impl std::error::Error for ConflictResolutionError {}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::types::FileType;

	fn create_test_file(mtime: u32, size: u64) -> FileData {
		FileData::builder(FileType::File, PathBuf::from("test.txt"))
			.mode(0o644)
			.user(1000)
			.group(1000)
			.ctime(0)
			.mtime(mtime)
			.size(size)
			.build()
	}

	#[test]
	fn test_conflict_creation() {
		let file = create_test_file(100, 1024);
		let version =
			FileVersion { node_index: 0, node_location: "node1".to_string(), file_data: file };

		let conflict = Conflict::new(
			1,
			PathBuf::from("test.txt"),
			ConflictType::ModifyModify,
			vec![version.clone()],
		);

		assert_eq!(conflict.id, 1);
		assert_eq!(conflict.version_count(), 1);
		assert_eq!(conflict.versions[0].node_index, 0);
	}

	#[test]
	fn test_newest_version() {
		let v1 = FileVersion {
			node_index: 0,
			node_location: "node1".to_string(),
			file_data: create_test_file(100, 1024),
		};
		let v2 = FileVersion {
			node_index: 1,
			node_location: "node2".to_string(),
			file_data: create_test_file(200, 1024),
		};

		let conflict =
			Conflict::new(1, PathBuf::from("test.txt"), ConflictType::ModifyModify, vec![v1, v2]);

		assert_eq!(conflict.newest_version(), Some(1));
	}

	#[test]
	fn test_largest_version() {
		let v1 = FileVersion {
			node_index: 0,
			node_location: "node1".to_string(),
			file_data: create_test_file(100, 1024),
		};
		let v2 = FileVersion {
			node_index: 1,
			node_location: "node2".to_string(),
			file_data: create_test_file(100, 2048),
		};

		let conflict =
			Conflict::new(1, PathBuf::from("test.txt"), ConflictType::ModifyModify, vec![v1, v2]);

		assert_eq!(conflict.largest_version(), Some(1));
	}

	#[test]
	fn test_version_by_name() {
		let v1 = FileVersion {
			node_index: 0,
			node_location: "node1".to_string(),
			file_data: create_test_file(100, 1024),
		};
		let v2 = FileVersion {
			node_index: 1,
			node_location: "server:/data".to_string(),
			file_data: create_test_file(100, 2048),
		};

		let conflict =
			Conflict::new(1, PathBuf::from("test.txt"), ConflictType::ModifyModify, vec![v1, v2]);

		assert_eq!(conflict.version_by_name("server:/data"), Some(1));
		assert_eq!(conflict.version_by_name("node1"), Some(0));
		assert_eq!(conflict.version_by_name("nonexistent"), None);
	}
}

// vim: ts=4
