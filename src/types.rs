//! Core data types for SyncR

use serde::ser::{Serialize, SerializeStruct, Serializer};
use serde::{Deserialize, Serialize as SerdeSerialize};
use std::collections::BTreeMap;
use std::path;
use std::time::Duration;

/// Configuration (kept for backward compatibility with existing code)
#[derive(Debug, Clone)]
pub struct Config {
	pub syncr_dir: path::PathBuf,
	pub profile: String,
}

/// Sync operation phases
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SyncPhase {
	/// Initializing sync session
	Initializing,

	/// Collecting file metadata from nodes
	Collecting,

	/// Detecting conflicts
	DetectingConflicts,

	/// Resolving conflicts
	ResolvingConflicts,

	/// Transferring file/directory metadata
	TransferringMetadata,

	/// Transferring file chunks
	TransferringChunks,

	/// Committing changes
	Committing,

	/// Sync complete
	Complete,
}

impl std::fmt::Display for SyncPhase {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			SyncPhase::Initializing => write!(f, "Initializing"),
			SyncPhase::Collecting => write!(f, "Collecting"),
			SyncPhase::DetectingConflicts => write!(f, "Detecting conflicts"),
			SyncPhase::ResolvingConflicts => write!(f, "Resolving conflicts"),
			SyncPhase::TransferringMetadata => write!(f, "Transferring metadata"),
			SyncPhase::TransferringChunks => write!(f, "Transferring chunks"),
			SyncPhase::Committing => write!(f, "Committing"),
			SyncPhase::Complete => write!(f, "Complete"),
		}
	}
}

/// Result of a sync operation
#[derive(Debug, Clone)]
pub struct SyncResult {
	/// Number of files successfully synced
	pub files_synced: usize,

	/// Number of directories created
	pub dirs_created: usize,

	/// Number of files deleted
	pub files_deleted: usize,

	/// Total bytes transferred
	pub bytes_transferred: u64,

	/// Number of chunks transferred
	pub chunks_transferred: usize,

	/// Number of chunks deduplicated (already present)
	pub chunks_deduplicated: usize,

	/// Number of conflicts encountered
	pub conflicts_encountered: usize,

	/// Number of conflicts resolved
	pub conflicts_resolved: usize,

	/// Duration of sync operation
	pub duration: Duration,

	/// Any non-fatal errors encountered
	pub errors: Vec<String>,
}

impl Default for SyncResult {
	fn default() -> Self {
		SyncResult {
			files_synced: 0,
			dirs_created: 0,
			files_deleted: 0,
			bytes_transferred: 0,
			chunks_transferred: 0,
			chunks_deduplicated: 0,
			conflicts_encountered: 0,
			conflicts_resolved: 0,
			duration: Duration::ZERO,
			errors: vec![],
		}
	}
}

#[derive(Clone, Debug)]
pub struct FileChunk {
	pub path: path::PathBuf,
	pub offset: u64,
	pub size: usize,
}

#[derive(Clone, PartialEq, Debug)]
pub struct HashChunk {
	pub hash: [u8; 32], // BLAKE3 binary hash
	pub offset: u64,
	pub size: u32, // Changed from usize for cache optimization
}

impl SerdeSerialize for HashChunk {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		let hash_b64 = crate::util::hash_to_base64(&self.hash);
		let mut state = serializer.serialize_struct("HashChunk", 3)?;
		state.serialize_field("h", &hash_b64)?;
		state.serialize_field("of", &self.offset)?;
		state.serialize_field("sz", &self.size)?;
		state.end()
	}
}

impl<'de> Deserialize<'de> for HashChunk {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		use serde::de;

		#[derive(Deserialize)]
		struct HashChunkHelper {
			#[serde(rename = "h")]
			hash: String,
			#[serde(rename = "of")]
			offset: u64,
			#[serde(rename = "sz")]
			size: u32,
		}

		let helper = HashChunkHelper::deserialize(deserializer)?;
		let hash = crate::util::base64_to_hash(&helper.hash)
			.map_err(|_| de::Error::custom("invalid hash base64"))?;

		Ok(HashChunk { hash, offset: helper.offset, size: helper.size })
	}
}

#[derive(Clone, PartialEq, Debug, SerdeSerialize, Deserialize)]
pub enum FileType {
	File,
	Dir,
	SymLink,
}

#[derive(Clone, PartialEq, Debug, Deserialize)]
pub struct FileData {
	#[serde(rename = "tp")]
	pub tp: FileType,
	#[serde(rename = "p")]
	pub path: path::PathBuf,
	#[serde(rename = "md")]
	pub mode: u32,
	#[serde(rename = "usr")]
	pub user: u32,
	#[serde(rename = "grp")]
	pub group: u32,
	#[serde(rename = "ct")]
	pub ctime: u32,
	#[serde(rename = "mt")]
	pub mtime: u32,
	#[serde(rename = "sz")]
	pub size: u64,
	#[serde(rename = "ch")]
	pub chunks: Vec<HashChunk>,
	#[serde(rename = "tgt")]
	pub target: Option<path::PathBuf>,
}

impl Serialize for FileData {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		let mut state = serializer.serialize_struct("File", 10)?;
		// Use short field names (max 3-4 chars)
		match &self.tp {
			FileType::File => state.serialize_field("tp", "F")?,
			FileType::SymLink => state.serialize_field("tp", "L")?,
			FileType::Dir => state.serialize_field("tp", "D")?,
		};
		state.serialize_field("p", &self.path.to_str())?;
		state.serialize_field("md", &self.mode)?;
		state.serialize_field("usr", &self.user)?;
		state.serialize_field("grp", &self.group)?;
		state.serialize_field("ct", &self.ctime)?;
		state.serialize_field("mt", &self.mtime)?;
		state.serialize_field("sz", &self.size)?;
		state.serialize_field("ch", &self.chunks)?;
		state.serialize_field("tgt", &self.target)?;
		state.end()
	}
}

/// File operation type for tracking changes across syncs
#[derive(Clone, PartialEq, Debug)]
#[allow(dead_code)]
pub enum FileOperation {
	Create,
	Modify,
	Delete,
}

/// State from a previous sync, used for three-way merge detection
#[derive(Clone, Debug, SerdeSerialize, Deserialize)]
pub struct PreviousSyncState {
	/// Files that existed in the previous sync, keyed by path
	pub files: BTreeMap<String, FileData>,
	/// Timestamp of the previous sync
	pub timestamp: u64,
}

#[cfg(test)]
mod test {
	use super::*;

	#[test]
	fn test_file_type_equality() {
		assert_eq!(FileType::File, FileType::File);
		assert_eq!(FileType::Dir, FileType::Dir);
		assert_eq!(FileType::SymLink, FileType::SymLink);
		assert_ne!(FileType::File, FileType::Dir);
	}

	#[test]
	fn test_hash_chunk_creation() {
		let hash = [0u8; 32];
		let chunk = HashChunk { hash, offset: 0, size: 1024 };
		assert_eq!(chunk.hash, hash);
		assert_eq!(chunk.offset, 0);
		assert_eq!(chunk.size, 1024);
	}

	#[test]
	fn test_hash_chunk_equality() {
		let hash = [1u8; 32];
		let chunk1 = HashChunk { hash, offset: 0, size: 1024 };
		let chunk2 = HashChunk { hash, offset: 0, size: 1024 };
		assert_eq!(chunk1, chunk2);
	}

	#[test]
	fn test_file_data_creation() {
		let fd = FileData {
			tp: FileType::File,
			path: path::PathBuf::from("/test/file.txt"),
			mode: 0o644,
			user: 1000,
			group: 1000,
			ctime: 1234567890,
			mtime: 1234567890,
			size: 4096,
			chunks: vec![],
			target: None,
		};
		assert_eq!(fd.tp, FileType::File);
		assert_eq!(fd.mode, 0o644);
		assert_eq!(fd.size, 4096);
		assert_eq!(fd.chunks.len(), 0);
	}

	#[test]
	fn test_file_data_with_chunks() {
		let mut hash1 = [0u8; 32];
		hash1[0] = 1;
		let mut hash2 = [0u8; 32];
		hash2[0] = 2;

		let chunk1 = HashChunk { hash: hash1, offset: 0, size: 1024 };
		let chunk2 = HashChunk { hash: hash2, offset: 1024, size: 512 };

		let fd = FileData {
			tp: FileType::File,
			path: path::PathBuf::from("/test/file.txt"),
			mode: 0o644,
			user: 1000,
			group: 1000,
			ctime: 1234567890,
			mtime: 1234567890,
			size: 1536,
			chunks: vec![chunk1, chunk2],
			target: None,
		};

		assert_eq!(fd.chunks.len(), 2);
		assert_eq!(fd.chunks[0].hash, hash1);
		assert_eq!(fd.chunks[1].hash, hash2);
		assert_eq!(fd.chunks[1].offset, 1024);
	}

	#[test]
	fn test_file_data_equality() {
		let fd1 = FileData {
			tp: FileType::File,
			path: path::PathBuf::from("/test/file.txt"),
			mode: 0o644,
			user: 1000,
			group: 1000,
			ctime: 1234567890,
			mtime: 1234567890,
			size: 1024,
			chunks: vec![],
			target: None,
		};

		let fd2 = FileData {
			tp: FileType::File,
			path: path::PathBuf::from("/test/file.txt"),
			mode: 0o644,
			user: 1000,
			group: 1000,
			ctime: 1234567890,
			mtime: 1234567890,
			size: 1024,
			chunks: vec![],
			target: None,
		};

		assert_eq!(fd1, fd2);
	}

	#[test]
	fn test_config_creation() {
		let config = Config {
			syncr_dir: path::PathBuf::from("/home/user/.syncr"),
			profile: String::from("test"),
		};
		assert_eq!(config.syncr_dir, path::PathBuf::from("/home/user/.syncr"));
		assert_eq!(config.profile, "test");
	}

	#[test]
	fn test_symlink_data_creation() {
		let fd = FileData {
			tp: FileType::SymLink,
			path: path::PathBuf::from("link"),
			mode: 0o777,
			user: 1000,
			group: 1000,
			ctime: 1234567890,
			mtime: 1234567890,
			size: 0,
			chunks: vec![],
			target: Some(path::PathBuf::from("target")),
		};
		assert_eq!(fd.tp, FileType::SymLink);
		assert_eq!(fd.size, 0);
		assert_eq!(fd.target, Some(path::PathBuf::from("target")));
		assert_eq!(fd.chunks.len(), 0);
	}

	#[test]
	fn test_symlink_data_without_target() {
		let fd = FileData {
			tp: FileType::SymLink,
			path: path::PathBuf::from("link"),
			mode: 0o777,
			user: 1000,
			group: 1000,
			ctime: 1234567890,
			mtime: 1234567890,
			size: 0,
			chunks: vec![],
			target: None,
		};
		assert_eq!(fd.tp, FileType::SymLink);
		assert_eq!(fd.target, None);
	}

	#[test]
	fn test_symlink_data_equality() {
		let fd1 = FileData {
			tp: FileType::SymLink,
			path: path::PathBuf::from("link"),
			mode: 0o777,
			user: 1000,
			group: 1000,
			ctime: 1234567890,
			mtime: 1234567890,
			size: 0,
			chunks: vec![],
			target: Some(path::PathBuf::from("target")),
		};

		let fd2 = FileData {
			tp: FileType::SymLink,
			path: path::PathBuf::from("link"),
			mode: 0o777,
			user: 1000,
			group: 1000,
			ctime: 1234567890,
			mtime: 1234567890,
			size: 0,
			chunks: vec![],
			target: Some(path::PathBuf::from("target")),
		};

		assert_eq!(fd1, fd2);
	}

	#[test]
	fn test_symlink_data_inequality() {
		let fd1 = FileData {
			tp: FileType::SymLink,
			path: path::PathBuf::from("link"),
			mode: 0o777,
			user: 1000,
			group: 1000,
			ctime: 1234567890,
			mtime: 1234567890,
			size: 0,
			chunks: vec![],
			target: Some(path::PathBuf::from("target1")),
		};

		let fd2 = FileData {
			tp: FileType::SymLink,
			path: path::PathBuf::from("link"),
			mode: 0o777,
			user: 1000,
			group: 1000,
			ctime: 1234567890,
			mtime: 1234567890,
			size: 0,
			chunks: vec![],
			target: Some(path::PathBuf::from("target2")),
		};

		assert_ne!(fd1, fd2);
	}
}

// vim: ts=4
