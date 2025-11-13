//! Cache validation functions

use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

/// Check if a cache entry is still valid (not expired)
///
/// # Arguments
/// * `created_at_secs` - Creation timestamp in seconds since UNIX_EPOCH
/// * `ttl_secs` - Time-to-live in seconds
///
/// # Returns
/// `true` if cache is still valid, `false` if expired
pub fn is_cache_valid(created_at_secs: u32, ttl_secs: u32) -> bool {
	let now = SystemTime::now();
	let created = UNIX_EPOCH + std::time::Duration::from_secs(created_at_secs as u64);
	let expiry = created + std::time::Duration::from_secs(ttl_secs as u64);
	now < expiry
}

/// Check if a cache entry is valid given current mtime
///
/// This is the extracted validation from the original cache.rs:159
/// Used to verify that a cached file entry is still fresh based on
/// the file's modification time.
///
/// # Arguments
/// * `rel_path` - Relative path of the cached file
/// * `created_at_secs` - Creation timestamp of cache entry (seconds since UNIX_EPOCH)
/// * `current_mtime` - Current modification time of the file
/// * `cached_mtime` - Modification time when file was cached
///
/// # Returns
/// `true` if cache is still valid (file hasn't been modified), `false` otherwise
pub fn is_file_cache_valid(
	created_at_secs: u32,
	current_mtime: u32,
	cached_mtime: u32,
	ttl_secs: u32,
) -> Result<bool, Box<dyn Error>> {
	// Cache is invalid if:
	// 1. TTL has expired
	// 2. File has been modified since cache was created
	if !is_cache_valid(created_at_secs, ttl_secs) {
		return Ok(false);
	}

	if current_mtime != cached_mtime {
		return Ok(false);
	}

	Ok(true)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_is_cache_valid_fresh() {
		let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as u32;
		let ttl = 3600; // 1 hour

		// Just created, should be valid
		assert!(is_cache_valid(now_secs, ttl));
	}

	#[test]
	fn test_is_cache_valid_expired() {
		// 10 seconds ago
		let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as u32;
		let created_secs = now_secs.saturating_sub(10);
		let ttl = 5; // 5 seconds

		// Should be expired
		assert!(!is_cache_valid(created_secs, ttl));
	}

	#[test]
	fn test_is_file_cache_valid_not_modified() {
		let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as u32;
		let mtime = 1234567890u32;

		// File not modified, cache not expired
		let result = is_file_cache_valid(now_secs, mtime, mtime, 3600).unwrap();
		assert!(result);
	}

	#[test]
	fn test_is_file_cache_valid_modified() {
		let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as u32;

		// File has been modified
		let result = is_file_cache_valid(now_secs, 1234567890, 1234567880, 3600).unwrap();
		assert!(!result);
	}

	#[test]
	fn test_is_file_cache_valid_expired() {
		let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as u32;
		let created_secs = now_secs.saturating_sub(10);
		let mtime = 1234567890u32;

		// Cache expired
		let result = is_file_cache_valid(created_secs, mtime, mtime, 5).unwrap();
		assert!(!result);
	}
}
