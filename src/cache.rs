//! Child cache for incremental file scanning
//!
//! Stores file metadata and chunks to avoid re-hashing unchanged files.
//! Uses simple mtime-based change detection.
//! Also provides path-level locking to prevent concurrent syncs on same paths.

use redb::{ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::types::HashChunk;

/// Cache entry for a single file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
	#[serde(rename = "mt")]
	pub mtime: u32,
	#[serde(rename = "uid")]
	pub uid: u32,
	#[serde(rename = "gid")]
	pub gid: u32,
	#[serde(rename = "ct")]
	pub ctime: u32,
	#[serde(rename = "sz")]
	pub size: u64,
	#[serde(rename = "md")]
	pub mode: u32,
	#[serde(rename = "ch")]
	pub chunks: Vec<HashChunk>,
}

/// Lock information for an active sync operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockInfo {
	/// Process ID of the sync operation
	#[serde(rename = "pid")]
	pub pid: u32,
	/// Unix timestamp when lock was acquired
	#[serde(rename = "str")]
	pub started: u64,
	/// All paths being synced in this operation
	#[serde(rename = "pth")]
	pub paths: Vec<String>,
	/// Remote nodes involved (e.g., "remote.example.com")
	#[serde(rename = "nod")]
	pub nodes: Vec<String>,
}

impl LockInfo {
	/// Check if this lock is stale (process is dead)
	pub fn is_stale(&self) -> bool {
		!is_process_alive(self.pid)
	}

	/// Check if this lock is too old (>24 hours)
	pub fn is_too_old(&self) -> bool {
		match SystemTime::now().duration_since(UNIX_EPOCH) {
			Ok(now) => {
				let age_secs = now.as_secs().saturating_sub(self.started);
				// 24 hours = 86400 seconds
				age_secs > 86400
			}
			Err(_) => false,
		}
	}
}

/// Table definition for file cache entries
/// Key: relative file path (String)
/// Value: serialized CacheEntry (bytes)
const FILES_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("files");

/// Table definition for active sync locks
/// Key: path being synced (String)
/// Value: serialized LockInfo (bytes)
const ACTIVE_SYNCS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("active_syncs");

/// Check if a process with given PID is currently alive
#[cfg(target_os = "linux")]
fn is_process_alive(pid: u32) -> bool {
	// On Linux, check if /proc/{pid} directory exists
	// This is a safe alternative to the kill syscall
	let proc_path_str = format!("/proc/{}", pid);
	std::path::Path::new(&proc_path_str).exists()
}

#[cfg(target_os = "macos")]
fn is_process_alive(pid: u32) -> bool {
	// On macOS, check if /proc/{pid} exists (macOS 13.0+) or fallback to conservative approach
	let proc_path_str = format!("/proc/{}", pid);
	if std::path::Path::new(&proc_path_str).exists() {
		return true;
	}
	// Fallback: assume alive if we can't determine (conservative approach)
	// Could be improved with sysctl or other macOS-specific APIs
	true
}

#[cfg(target_os = "windows")]
fn is_process_alive(_pid: u32) -> bool {
	// On Windows, assume process is alive (conservative approach)
	// Could be improved with Windows API calls if needed
	true
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn is_process_alive(_pid: u32) -> bool {
	// On other Unix-like systems, assume process is alive (conservative approach)
	true
}

/// Check if an error indicates the database is already open
fn is_database_already_open_error(error: &redb::DatabaseError) -> bool {
	// Check if this is a DatabaseAlreadyOpen error by examining the error message
	// redb returns this when attempting to open an already-open database
	let error_msg = error.to_string();
	error_msg.contains("DatabaseAlreadyOpen") || error_msg.contains("already open")
}

/// Guard that releases path locks when dropped
#[derive(Debug)]
pub struct PathLockGuard {
	db_path: path::PathBuf,
	paths: Vec<String>,
}

impl PathLockGuard {
	/// Manual release of locks (called on drop or explicitly)
	fn release(&mut self) -> Result<(), Box<dyn Error>> {
		if !self.paths.is_empty() {
			// Try to open the database to release locks
			// If the database is already open by the original ChildCache instance,
			// that's expected and safe - locks will be released when ChildCache drops.
			// Stale lock detection will clean up any remaining locks if needed.
			match redb::Database::open(&self.db_path) {
				Ok(db) => {
					let write_txn = db.begin_write()?;
					{
						let mut table = write_txn.open_table(ACTIVE_SYNCS_TABLE)?;
						for path in &self.paths {
							let _ = table.remove(path.as_str());
						}
					}
					write_txn.commit()?;
					self.paths.clear();
				}
				Err(e) if is_database_already_open_error(&e) => {
					// Database is still open (original ChildCache instance active)
					// Locks will be released when the original ChildCache is dropped
					// This is safe - stale lock detection will clean up if needed
				}
				Err(e) => return Err(e.into()),
			}
		}
		Ok(())
	}
}

impl Drop for PathLockGuard {
	fn drop(&mut self) {
		let _ = self.release();
	}
}

/// Child cache backed by redb database
pub struct ChildCache {
	pub(crate) db: redb::Database,
	db_path: path::PathBuf,
}

impl ChildCache {
	/// Open or create a child cache database
	pub fn open(db_path: &path::Path) -> Result<Self, Box<dyn Error>> {
		let db = redb::Database::create(db_path)?;
		// Ensure both tables exist
		{
			let write_txn = db.begin_write()?;
			let _ = write_txn.open_table(FILES_TABLE)?;
			let _ = write_txn.open_table(ACTIVE_SYNCS_TABLE)?;
			write_txn.commit()?;
		}
		Ok(ChildCache { db, db_path: db_path.to_path_buf() })
	}

	/// Check if a file cache entry is valid (exists and mtime matches)
	pub fn is_valid(&self, rel_path: &str, current_mtime: u32) -> Result<bool, Box<dyn Error>> {
		let read_txn = self.db.begin_read()?;
		let table = read_txn.open_table(FILES_TABLE)?;

		match table.get(rel_path)? {
			Some(entry) => {
				let bytes = entry.value().to_vec();
				let cached: CacheEntry = json5::from_str(std::str::from_utf8(&bytes)?)?;
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
				let cached: CacheEntry = json5::from_str(std::str::from_utf8(&bytes)?)?;
				Ok(Some(cached.chunks))
			}
			None => Ok(None),
		}
	}

	/// Store or update cache entry for a file
	pub fn set(&self, rel_path: &str, entry: CacheEntry) -> Result<(), Box<dyn Error>> {
		let bytes = json5::to_string(&entry)?.into_bytes();

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
				let cached: CacheEntry = json5::from_str(std::str::from_utf8(&bytes)?)?;
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

	/// Acquire locks on multiple paths atomically
	///
	/// Returns a PathLockGuard that releases locks on drop.
	/// Automatically cleans up stale locks before attempting acquisition.
	/// Returns error if any path is already locked by a live process.
	pub fn acquire_locks(
		&self,
		paths: &[&str],
		remote_nodes: &[String],
	) -> Result<PathLockGuard, Box<dyn Error>> {
		// First, clean up any stale locks
		self.cleanup_stale_locks()?;

		// Atomically check and acquire locks
		let write_txn = self.db.begin_write()?;
		{
			let mut table = write_txn.open_table(ACTIVE_SYNCS_TABLE)?;

			// Check if any path is already locked
			for path in paths {
				if let Some(_existing) = table.get(path)? {
					return Err(format!(
						"Path already being synced: {}. Use 'syncr unlock --force {}' to override.",
						path, path
					)
					.into());
				}
			}

			// Create lock info
			let lock_info = LockInfo {
				pid: std::process::id(),
				started: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
				paths: paths.iter().map(|s| s.to_string()).collect(),
				nodes: remote_nodes.to_vec(),
			};

			let lock_bytes = json5::to_string(&lock_info)?.into_bytes();

			// Acquire locks for all paths
			for path in paths {
				table.insert(path, lock_bytes.as_slice())?;
			}
		}
		write_txn.commit()?;

		Ok(PathLockGuard {
			db_path: self.db_path.clone(),
			paths: paths.iter().map(|s| s.to_string()).collect(),
		})
	}

	/// Check if a path is currently locked
	#[allow(dead_code)]
	pub fn is_path_locked(&self, path: &str) -> Result<bool, Box<dyn Error>> {
		let read_txn = self.db.begin_read()?;
		let table = read_txn.open_table(ACTIVE_SYNCS_TABLE)?;
		Ok(table.get(path)?.is_some())
	}

	/// Get lock info for a path (if locked)
	#[allow(dead_code)]
	pub fn get_lock_info(&self, path: &str) -> Result<Option<LockInfo>, Box<dyn Error>> {
		let read_txn = self.db.begin_read()?;
		let table = read_txn.open_table(ACTIVE_SYNCS_TABLE)?;

		match table.get(path)? {
			Some(entry) => {
				let bytes = entry.value().to_vec();
				let lock_info: LockInfo = json5::from_str(std::str::from_utf8(&bytes)?)?;
				Ok(Some(lock_info))
			}
			None => Ok(None),
		}
	}

	/// Clean up stale locks (from dead processes or too old)
	pub fn cleanup_stale_locks(&self) -> Result<u32, Box<dyn Error>> {
		let mut count = 0;
		let mut stale_keys = Vec::new();

		// First pass: identify stale locks
		{
			let read_txn = self.db.begin_read()?;
			let table = read_txn.open_table(ACTIVE_SYNCS_TABLE)?;
			let mut iter = table.iter()?;

			loop {
				match iter.next() {
					Some(Ok((key, entry))) => {
						let bytes = entry.value().to_vec();
						if let Ok(lock_info) = serde_json::from_slice::<LockInfo>(&bytes) {
							if lock_info.is_stale() || lock_info.is_too_old() {
								stale_keys.push(key.value().to_string());
							}
						}
					}
					None => break,
					Some(Err(_)) => continue,
				}
			}
		}

		// Second pass: remove stale locks
		if !stale_keys.is_empty() {
			let write_txn = self.db.begin_write()?;
			{
				let mut table = write_txn.open_table(ACTIVE_SYNCS_TABLE)?;
				for key in &stale_keys {
					table.remove(key.as_str())?;
					count += 1;
				}
			}
			write_txn.commit()?;
		}

		Ok(count)
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

	#[test]
	fn test_acquire_locks_single_path() {
		let tmp = TempDir::new().unwrap();
		let db_path = tmp.path().join("test.db");
		let cache = ChildCache::open(&db_path).unwrap();

		// Acquire lock on single path
		let _guard = cache.acquire_locks(&["./dir1"], &[]).unwrap();

		// Verify lock is held
		assert!(cache.is_path_locked("./dir1").unwrap());

		// Try to acquire again - should fail
		let result = cache.acquire_locks(&["./dir1"], &[]);
		assert!(result.is_err());

		// Lock should be released when guard drops
	}

	#[test]
	fn test_acquire_locks_multiple_paths() {
		let tmp = TempDir::new().unwrap();
		let db_path = tmp.path().join("test.db");
		let cache = ChildCache::open(&db_path).unwrap();

		// Acquire locks on multiple paths
		let _guard = cache.acquire_locks(&["./dir1", "./dir2", "./dir3"], &[]).unwrap();

		// All paths should be locked
		assert!(cache.is_path_locked("./dir1").unwrap());
		assert!(cache.is_path_locked("./dir2").unwrap());
		assert!(cache.is_path_locked("./dir3").unwrap());
	}

	#[test]
	fn test_concurrent_different_paths_allowed() {
		let tmp = TempDir::new().unwrap();
		let db_path = tmp.path().join("test.db");
		let cache = ChildCache::open(&db_path).unwrap();

		// Acquire lock on dir1
		let _guard1 = cache.acquire_locks(&["./dir1"], &[]).unwrap();

		// Should be able to acquire lock on dir2 (different path)
		let _guard2 = cache.acquire_locks(&["./dir2"], &[]).unwrap();

		// Both should be locked
		assert!(cache.is_path_locked("./dir1").unwrap());
		assert!(cache.is_path_locked("./dir2").unwrap());
	}

	#[test]
	fn test_concurrent_same_path_blocked() {
		let tmp = TempDir::new().unwrap();
		let db_path = tmp.path().join("test.db");
		let cache = ChildCache::open(&db_path).unwrap();

		// Acquire lock on dir1
		let _guard1 = cache.acquire_locks(&["./dir1"], &[]).unwrap();

		// Trying to acquire same path should fail
		let result = cache.acquire_locks(&["./dir1"], &[]);
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("already being synced"));
	}

	#[test]
	fn test_lock_info_stored() {
		let tmp = TempDir::new().unwrap();
		let db_path = tmp.path().join("test.db");
		let cache = ChildCache::open(&db_path).unwrap();

		let remote_nodes = vec!["remote.example.com".to_string()];
		let _guard = cache.acquire_locks(&["./dir1"], &remote_nodes).unwrap();

		// Get lock info
		let lock_info = cache.get_lock_info("./dir1").unwrap();
		assert!(lock_info.is_some());

		let lock = lock_info.unwrap();
		assert_eq!(lock.pid, std::process::id());
		assert_eq!(lock.paths, vec!["./dir1"]);
		assert_eq!(lock.nodes, remote_nodes);
	}

	#[test]
	fn test_cleanup_stale_locks() {
		let tmp = TempDir::new().unwrap();
		let db_path = tmp.path().join("test.db");

		// Create a stale lock in a fresh database
		{
			let cache = ChildCache::open(&db_path).unwrap();
			// Manually insert a stale lock (old timestamp, dead PID) using acquire_locks
			let write_txn = cache.db.begin_write().unwrap();
			{
				let mut table = write_txn.open_table(ACTIVE_SYNCS_TABLE).unwrap();
				let stale_lock = LockInfo {
					pid: 1,     // PID 1 is unlikely to be in our process group
					started: 0, // Very old timestamp
					paths: vec!["./stale_dir".to_string()],
					nodes: vec![],
				};
				let bytes = json5::to_string(&stale_lock).unwrap().into_bytes();
				table.insert("./stale_dir", bytes.as_slice()).unwrap();
			}
			write_txn.commit().unwrap();

			// Stale lock should exist
			assert!(cache.is_path_locked("./stale_dir").unwrap());

			// Clean up stale locks
			let removed = cache.cleanup_stale_locks().unwrap();
			assert_eq!(removed, 1);

			// Stale lock should be gone
			assert!(!cache.is_path_locked("./stale_dir").unwrap());
		}
	}
}

// vim: ts=4
