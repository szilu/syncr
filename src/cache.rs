//! Child cache for incremental file scanning
//!
//! Stores file metadata and chunks to avoid re-hashing unchanged files.
//! Uses simple mtime-based change detection.

use redb::{ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::path;

use crate::types::HashChunk;

/// Cache entry for a single file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
	pub mtime: u32,
	pub uid: u32,
	pub gid: u32,
	pub ctime: u32,
	pub size: u64,
	pub mode: u32,
	pub chunks: Vec<HashChunk>,
}

/// Table definition for file cache entries
/// Key: relative file path (String)
/// Value: serialized CacheEntry (bytes)
const FILES_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("files");

/// Child cache backed by redb database
pub struct ChildCache {
	db: redb::Database,
}

impl ChildCache {
	/// Open or create a child cache database
	pub fn open(db_path: &path::Path) -> Result<Self, Box<dyn Error>> {
		let db = redb::Database::create(db_path)?;
		// Ensure the table exists
		{
			let write_txn = db.begin_write()?;
			let _ = write_txn.open_table(FILES_TABLE)?;
			write_txn.commit()?;
		}
		Ok(ChildCache { db })
	}

	/// Check if a file cache entry is valid (exists and mtime matches)
	pub fn is_valid(&self, rel_path: &str, current_mtime: u32) -> Result<bool, Box<dyn Error>> {
		let read_txn = self.db.begin_read()?;
		let table = read_txn.open_table(FILES_TABLE)?;

		match table.get(rel_path)? {
			Some(entry) => {
				let bytes = entry.value().to_vec();
				let cached: CacheEntry = bincode::deserialize(&bytes)?;
				Ok(cached.mtime == current_mtime)
			}
			None => Ok(false),
		}
	}

	/// Get cached chunks for a file if valid
	pub fn get_chunks(
		&self,
		rel_path: &str,
		current_mtime: u32,
	) -> Result<Option<Vec<HashChunk>>, Box<dyn Error>> {
		if !self.is_valid(rel_path, current_mtime)? {
			return Ok(None);
		}

		let read_txn = self.db.begin_read()?;
		let table = read_txn.open_table(FILES_TABLE)?;

		match table.get(rel_path)? {
			Some(entry) => {
				let bytes = entry.value().to_vec();
				let cached: CacheEntry = bincode::deserialize(&bytes)?;
				Ok(Some(cached.chunks))
			}
			None => Ok(None),
		}
	}

	/// Store or update cache entry for a file
	pub fn set(&self, rel_path: &str, entry: CacheEntry) -> Result<(), Box<dyn Error>> {
		let bytes = bincode::serialize(&entry)?;

		let write_txn = self.db.begin_write()?;
		{
			let mut table = write_txn.open_table(FILES_TABLE)?;
			table.insert(rel_path, bytes.as_slice())?;
		}
		write_txn.commit()?;

		Ok(())
	}

	/// Get complete cache entry (for debugging/inspection)
	#[allow(dead_code)]
	pub fn get_entry(&self, rel_path: &str) -> Result<Option<CacheEntry>, Box<dyn Error>> {
		let read_txn = self.db.begin_read()?;
		let table = read_txn.open_table(FILES_TABLE)?;

		match table.get(rel_path)? {
			Some(entry) => {
				let bytes = entry.value().to_vec();
				let cached: CacheEntry = bincode::deserialize(&bytes)?;
				Ok(Some(cached))
			}
			None => Ok(None),
		}
	}

	/// Clear all cache entries (for testing)
	#[allow(dead_code)]
	pub fn clear(&self) -> Result<(), Box<dyn Error>> {
		let write_txn = self.db.begin_write()?;
		{
			let mut table = write_txn.open_table(FILES_TABLE)?;
			let mut iter = table.iter()?;
			let mut keys_to_remove = Vec::new();
			loop {
				match iter.next() {
					Some(Ok((key, _))) => {
						keys_to_remove.push(key.value().to_string());
					}
					None => break,
					Some(Err(e)) => return Err(e.into()),
				}
			}
			drop(iter);
			for key in keys_to_remove {
				table.remove(key.as_str())?;
			}
		}
		write_txn.commit()?;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use tempfile::TempDir;

	#[test]
	fn test_cache_create_and_set() {
		let tmp = TempDir::new().unwrap();
		let db_path = tmp.path().join("test.db");
		let cache = ChildCache::open(&db_path).unwrap();

		let chunks = vec![HashChunk { hash: [1u8; 32], offset: 0, size: 1024 }];
		let entry = CacheEntry {
			mtime: 12345,
			uid: 1000,
			gid: 1000,
			ctime: 12345,
			size: 1024,
			mode: 0o644,
			chunks,
		};

		cache.set("file.txt", entry).unwrap();

		assert!(cache.is_valid("file.txt", 12345).unwrap());
		assert!(!cache.is_valid("file.txt", 12346).unwrap());
	}

	#[test]
	fn test_cache_get_chunks() {
		let tmp = TempDir::new().unwrap();
		let db_path = tmp.path().join("test.db");
		let cache = ChildCache::open(&db_path).unwrap();

		let chunks = vec![
			HashChunk { hash: [1u8; 32], offset: 0, size: 1024 },
			HashChunk { hash: [2u8; 32], offset: 1024, size: 512 },
		];

		let entry = CacheEntry {
			mtime: 12345,
			uid: 1000,
			gid: 1000,
			ctime: 12345,
			size: 1536,
			mode: 0o644,
			chunks: chunks.clone(),
		};

		cache.set("file.txt", entry).unwrap();

		let retrieved = cache.get_chunks("file.txt", 12345).unwrap().unwrap();
		assert_eq!(retrieved.len(), 2);
		assert_eq!(retrieved[0].hash, [1u8; 32]);
		assert_eq!(retrieved[1].hash, [2u8; 32]);
	}

	#[test]
	fn test_cache_mtime_invalidation() {
		let tmp = TempDir::new().unwrap();
		let db_path = tmp.path().join("test.db");
		let cache = ChildCache::open(&db_path).unwrap();

		let chunks = vec![HashChunk { hash: [1u8; 32], offset: 0, size: 1024 }];
		let entry = CacheEntry {
			mtime: 12345,
			uid: 1000,
			gid: 1000,
			ctime: 12345,
			size: 1024,
			mode: 0o644,
			chunks,
		};

		cache.set("file.txt", entry).unwrap();

		// Cache hit with correct mtime
		assert_eq!(
			cache.get_chunks("file.txt", 12345).unwrap(),
			Some(vec![HashChunk { hash: [1u8; 32], offset: 0, size: 1024 }])
		);

		// Cache miss with different mtime
		assert_eq!(cache.get_chunks("file.txt", 99999).unwrap(), None);
	}
}

// vim: ts=4
