//! Conflict detection and resolution

use crate::types::FileData;
use std::path::PathBuf;

/// Represents a sync conflict between nodes
#[derive(Debug, Clone)]
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
	pub fn mtime(&self) -> u32 {
		self.file_data.mtime
	}

	/// Get the file size
	pub fn size(&self) -> u64 {
		self.file_data.size
	}
}

impl Conflict {
	/// Create a new conflict
	pub fn new(
		id: u64,
		path: PathBuf,
		conflict_type: ConflictType,
		versions: Vec<FileVersion>,
	) -> Self {
		Conflict { id, path, conflict_type, versions }
	}

	/// Get the number of conflicting versions
	pub fn version_count(&self) -> usize {
		self.versions.len()
	}

	/// Find the version with the newest modification time
	pub fn newest_version(&self) -> Option<usize> {
		self.versions.iter().enumerate().max_by_key(|(_, v)| v.mtime()).map(|(i, _)| i)
	}

	/// Find the version with the oldest modification time
	pub fn oldest_version(&self) -> Option<usize> {
		self.versions.iter().enumerate().min_by_key(|(_, v)| v.mtime()).map(|(i, _)| i)
	}

	/// Find the version with the largest file
	pub fn largest_version(&self) -> Option<usize> {
		self.versions.iter().enumerate().max_by_key(|(_, v)| v.size()).map(|(i, _)| i)
	}

	/// Find the version with the smallest file
	pub fn smallest_version(&self) -> Option<usize> {
		self.versions.iter().enumerate().min_by_key(|(_, v)| v.size()).map(|(i, _)| i)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::types::FileType;

	fn create_test_file(mtime: u32, size: u64) -> FileData {
		FileData {
			tp: FileType::File,
			path: PathBuf::from("test.txt"),
			mode: 0o644,
			user: 1000,
			group: 1000,
			ctime: 0,
			mtime,
			size,
			chunks: vec![],
		}
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
}
