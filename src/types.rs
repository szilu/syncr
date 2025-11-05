use serde::ser::{Serialize, SerializeStruct, Serializer};
use serde::{Deserialize, Serialize as SerdeSerialize};
use std::collections::BTreeMap;
use std::path;

#[derive(Debug)]
pub struct Config {
	pub syncr_dir: path::PathBuf,
	pub profile: String,
}

#[derive(Clone, Debug)]
pub struct FileChunk {
	pub path: path::PathBuf,
	pub offset: u64,
	pub size: usize,
}

#[derive(Clone, PartialEq, Debug, SerdeSerialize, Deserialize)]
pub struct HashChunk {
	pub hash: String,
	pub offset: u64,
	pub size: usize,
}

#[derive(Clone, PartialEq, Debug, SerdeSerialize, Deserialize)]
pub enum FileType {
	File,
	Dir,
	SymLink,
}

#[derive(Clone, PartialEq, Debug, Deserialize)]
pub struct FileData {
	pub tp: FileType,
	pub path: path::PathBuf,
	pub mode: u32,
	pub user: u32,
	pub group: u32,
	pub ctime: u32,
	pub mtime: u32,
	pub size: u64,
	pub chunks: Vec<HashChunk>,
}

impl Serialize for FileData {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		let mut state = serializer.serialize_struct("File", 2)?;
		match &self.tp {
			FileType::File => state.serialize_field("type", "F")?,
			FileType::SymLink => state.serialize_field("type", "L")?,
			FileType::Dir => state.serialize_field("type", "D")?,
		};
		state.serialize_field("path", &self.path.to_str())?;
		state.end()
	}
}

/// File operation type for tracking changes across syncs
#[allow(dead_code)]
#[derive(Clone, PartialEq, Debug)]
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
		let chunk = HashChunk { hash: String::from("abc123"), offset: 0, size: 1024 };
		assert_eq!(chunk.hash, "abc123");
		assert_eq!(chunk.offset, 0);
		assert_eq!(chunk.size, 1024);
	}

	#[test]
	fn test_hash_chunk_equality() {
		let chunk1 = HashChunk { hash: String::from("abc123"), offset: 0, size: 1024 };
		let chunk2 = HashChunk { hash: String::from("abc123"), offset: 0, size: 1024 };
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
		};
		assert_eq!(fd.tp, FileType::File);
		assert_eq!(fd.mode, 0o644);
		assert_eq!(fd.size, 4096);
		assert_eq!(fd.chunks.len(), 0);
	}

	#[test]
	fn test_file_data_with_chunks() {
		let chunk1 = HashChunk { hash: String::from("hash1"), offset: 0, size: 1024 };
		let chunk2 = HashChunk { hash: String::from("hash2"), offset: 1024, size: 512 };

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
		};

		assert_eq!(fd.chunks.len(), 2);
		assert_eq!(fd.chunks[0].hash, "hash1");
		assert_eq!(fd.chunks[1].hash, "hash2");
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
}
