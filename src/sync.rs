//! Synchronization API - high-level and mid-level interfaces

use crate::chunking::SyncOptions;
use crate::config::Config;
use crate::error::SyncError;
use crate::strategies::ConflictResolution;
use crate::types::SyncResult;
use std::path::PathBuf;

/// Simple n-way synchronization with default settings
///
/// # Arguments
/// * `locations` - Vector of directory paths (local or remote)
/// * `options` - Optional configuration (uses defaults if None)
///
/// # Returns
/// * `SyncResult` containing statistics
///
/// # Example
/// ```rust,ignore
/// let result = sync(vec!["./dir1", "./dir2"], None).await?;
/// println!("Synced {} files", result.files_synced);
/// ```
pub async fn sync(
	locations: Vec<&str>,
	options: Option<SyncOptions>,
) -> Result<SyncResult, SyncError> {
	let builder = SyncBuilder::new();
	let builder = locations.into_iter().fold(builder, |b, loc| b.add_location(loc));

	if let Some(opts) = options {
		let builder = if let Some(strategy) = opts.conflict_resolution {
			builder.conflict_resolution(strategy)
		} else {
			builder
		};
		builder.sync().await
	} else {
		builder.sync().await
	}
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
	/// Number of cache entries
	pub entries: usize,
	/// Database size in bytes
	pub database_size_bytes: u64,
	/// Number of active locks
	pub active_locks: usize,
}

/// No-op callback for internal sync operations
struct NoOpCallback;

impl crate::sync_impl::SyncProgressCallback for NoOpCallback {
	fn on_event(&self, _event: crate::sync_impl::SyncCallbackEvent) {
		// Do nothing - this is used when SyncBuilder doesn't have registered callbacks
	}
}

/// Builder for flexible sync configuration
pub struct SyncBuilder {
	locations: Vec<String>,
	config: Config,
}

impl SyncBuilder {
	/// Create a new sync builder with default settings
	pub fn new() -> Self {
		SyncBuilder { locations: Vec::new(), config: Config::default() }
	}

	/// Add a local directory to sync
	pub fn add_location(mut self, path: &str) -> Self {
		self.locations.push(path.to_string());
		self
	}

	/// Add a remote directory via SSH
	pub fn add_remote(mut self, location: &str) -> Self {
		self.locations.push(location.to_string());
		self
	}

	/// Set the profile name for state persistence
	pub fn profile(mut self, name: &str) -> Self {
		self.config.profile = name.to_string();
		self
	}

	/// Set conflict resolution strategy
	pub fn conflict_resolution(mut self, strategy: ConflictResolution) -> Self {
		self.config.conflict_resolution = strategy;
		self
	}

	/// Register progress callback
	pub fn on_progress<F>(self, _callback: F) -> Self
	where
		F: Fn(crate::callbacks::ProgressStats) + Send + 'static,
	{
		// TODO: Implement callback registration
		self
	}

	/// Register conflict resolution callback
	pub fn on_conflict<F>(self, _callback: F) -> Self
	where
		F: Fn(&crate::conflict::Conflict) -> Option<usize> + Send + 'static,
	{
		// TODO: Implement callback registration
		self
	}

	/// Set file/directory exclusion patterns (glob syntax)
	pub fn exclude_patterns(mut self, patterns: Vec<&str>) -> Self {
		self.config.exclude_patterns = patterns.iter().map(|s| s.to_string()).collect();
		self
	}

	/// Set chunk size (in bits, default: 20 = ~1MB avg)
	pub fn chunk_size_bits(mut self, bits: u32) -> Self {
		self.config.chunk_bits = bits;
		self
	}

	/// Set custom state directory (default: ~/.syncr)
	pub fn state_dir(mut self, path: &str) -> Self {
		self.config.syncr_dir = PathBuf::from(path);
		self
	}

	/// Enable/disable dry run mode
	pub fn dry_run(mut self, enabled: bool) -> Self {
		self.config.dry_run = enabled;
		self
	}

	/// Get the number of locations configured
	pub fn location_count(&self) -> usize {
		self.locations.len()
	}

	/// Get a reference to the locations
	pub fn locations(&self) -> &[String] {
		&self.locations
	}

	/// Get a reference to the sync configuration
	pub fn config(&self) -> &Config {
		&self.config
	}

	/// Get the profile name
	pub fn profile_name(&self) -> &str {
		&self.config.profile
	}

	/// Get the state directory
	pub fn state_directory(&self) -> &std::path::Path {
		&self.config.syncr_dir
	}

	/// Get the path to the state file for this profile
	pub fn state_path(&self) -> std::path::PathBuf {
		self.config.syncr_dir.join(format!("{}.profile.json", self.config.profile))
	}

	/// List all profiles in a state directory
	pub async fn list_profiles(state_dir: &std::path::Path) -> Result<Vec<String>, SyncError> {
		use tokio::fs;

		if !state_dir.exists() {
			return Ok(Vec::new());
		}

		let mut profiles = Vec::new();
		let mut entries = fs::read_dir(state_dir).await.map_err(|e| SyncError::Other {
			message: format!("Failed to read state directory: {}", e),
		})?;

		while let Some(entry) = entries.next_entry().await.map_err(|e| SyncError::Other {
			message: format!("Failed to read directory entry: {}", e),
		})? {
			let filename = entry.file_name();
			if let Some(name) = filename.to_str() {
				if name.ends_with(".profile.json") {
					let profile_name = name.trim_end_matches(".profile.json").to_string();
					profiles.push(profile_name);
				}
			}
		}

		profiles.sort();
		Ok(profiles)
	}

	/// Check if a profile exists in a state directory
	pub async fn profile_exists(state_dir: &std::path::Path, profile: &str) -> bool {
		let profile_path = state_dir.join(format!("{}.profile.json", profile));
		profile_path.exists()
	}

	/// Delete a profile from a state directory
	pub async fn delete_profile(
		state_dir: &std::path::Path,
		profile: &str,
	) -> Result<(), SyncError> {
		use tokio::fs;

		let profile_path = state_dir.join(format!("{}.profile.json", profile));

		if profile_path.exists() {
			fs::remove_file(&profile_path).await.map_err(|e| SyncError::Other {
				message: format!("Failed to delete profile: {}", e),
			})?;
		}

		Ok(())
	}

	/// Load previously saved state for this profile
	pub async fn load_state(&self) -> Result<Option<crate::types::PreviousSyncState>, SyncError> {
		use serde::{Deserialize, Serialize};
		use std::collections::BTreeMap;
		use tokio::fs;

		#[derive(Serialize, Deserialize)]
		struct StateWrapper {
			#[serde(default)]
			files: BTreeMap<String, crate::types::FileData>,
			#[serde(default)]
			timestamp: u64,
		}

		let state_file =
			self.config.syncr_dir.join(format!("{}.profile.json", self.config.profile));

		// If file doesn't exist, this is the first sync
		if !state_file.exists() {
			return Ok(None);
		}

		// Try to read and parse the state
		let contents = fs::read_to_string(&state_file).await.map_err(|e| SyncError::Other {
			message: format!("Failed to read state file: {}", e),
		})?;

		let wrapper: StateWrapper = json5::from_str(&contents).map_err(|e| SyncError::Other {
			message: format!("Failed to parse state file: {}", e),
		})?;

		Ok(Some(crate::types::PreviousSyncState {
			files: wrapper.files,
			timestamp: wrapper.timestamp,
		}))
	}

	/// Save state for this profile
	pub async fn save_state(
		&self,
		state: &crate::types::PreviousSyncState,
	) -> Result<(), SyncError> {
		use serde::Serialize;
		use tokio::fs;

		#[derive(Serialize)]
		struct StateWrapper {
			files: std::collections::BTreeMap<String, crate::types::FileData>,
			timestamp: u64,
		}

		// Ensure state directory exists
		fs::create_dir_all(&self.config.syncr_dir).await.map_err(|e| SyncError::Other {
			message: format!("Failed to create state directory: {}", e),
		})?;

		let state_file =
			self.config.syncr_dir.join(format!("{}.profile.json", self.config.profile));

		let wrapper = StateWrapper { files: state.files.clone(), timestamp: state.timestamp };

		let json = serde_json::to_string_pretty(&wrapper).map_err(|e| SyncError::Other {
			message: format!("Failed to serialize state: {}", e),
		})?;

		fs::write(&state_file, json).await.map_err(|e| SyncError::Other {
			message: format!("Failed to write state file: {}", e),
		})?;

		Ok(())
	}

	/// Clear saved state for this profile
	pub async fn clear_state(&self) -> Result<(), SyncError> {
		use tokio::fs;

		let state_file =
			self.config.syncr_dir.join(format!("{}.profile.json", self.config.profile));

		if state_file.exists() {
			fs::remove_file(&state_file).await.map_err(|e| SyncError::Other {
				message: format!("Failed to delete state file: {}", e),
			})?;
		}

		Ok(())
	}

	/// Clear the sync cache
	pub async fn clear_cache(&self) -> Result<(), SyncError> {
		use tokio::fs;

		let cache_db_path = self.config.syncr_dir.join("cache.db");

		if cache_db_path.exists() {
			fs::remove_file(&cache_db_path).await.map_err(|e| SyncError::Other {
				message: format!("Failed to delete cache file: {}", e),
			})?;
		}

		Ok(())
	}

	/// Get cache statistics
	pub async fn cache_stats(&self) -> Result<CacheStats, SyncError> {
		use tokio::fs;

		let cache_db_path = self.config.syncr_dir.join("cache.db");

		let database_size_bytes = if cache_db_path.exists() {
			fs::metadata(&cache_db_path).await.map(|m| m.len()).unwrap_or(0)
		} else {
			0
		};

		// For now, return basic stats
		// In a real implementation, this would query the cache database
		Ok(CacheStats { entries: 0, database_size_bytes, active_locks: 0 })
	}

	/// Cleanup stale locks
	pub async fn cleanup_stale_locks(&self) -> Result<usize, SyncError> {
		// For now, return 0 (no stale locks cleaned)
		// In a real implementation, this would check for and clean stale locks
		Ok(0)
	}

	/// Execute the sync operation
	pub async fn sync(self) -> Result<SyncResult, SyncError> {
		use std::path::Path;

		if self.locations.is_empty() {
			return Err(SyncError::InvalidConfig {
				message: "At least one location is required".to_string(),
			});
		}

		// Validate that local directories exist (only for absolute paths)
		for location in &self.locations {
			// Skip remote locations (those containing ':' and not starting with '/', '.', or '~')
			if location.contains(':')
				&& !location.starts_with('/')
				&& !location.starts_with('.')
				&& !location.starts_with('~')
			{
				// Remote location, skip validation
				continue;
			}

			let path = Path::new(location);

			// Only validate absolute paths (starting with '/')
			// Relative paths will be validated later when connecting
			if path.is_absolute() && !path.starts_with("~") {
				if !path.exists() {
					return Err(SyncError::InvalidConfig {
						message: format!("Directory does not exist: {}", location),
					});
				}

				if !path.is_dir() {
					return Err(SyncError::InvalidConfig {
						message: format!("Path is not a directory: {}", location),
					});
				}
			}
		}

		// Use the SyncBuilder's config directly (it's already the unified Config)
		let config = self.config.clone();

		// Convert locations to &str for sync_impl
		let locations: Vec<&str> = self.locations.iter().map(|s| s.as_str()).collect();

		// Create a no-op callback (sync_impl will handle conflicts internally)
		let callback = Box::new(NoOpCallback);

		// Delegate to the working sync_impl implementation
		crate::sync_impl::sync_with_callbacks(config, locations, callback, None)
			.await
			.map_err(|e| SyncError::Other { message: e.to_string() })
	}
}

impl Default for SyncBuilder {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::strategies::ConflictResolution;

	#[test]
	fn test_builder_creation() {
		let builder =
			SyncBuilder::new().add_location("./dir1").add_location("./dir2").profile("test");

		assert_eq!(builder.locations.len(), 2);
		assert_eq!(builder.config.profile, "test");
	}

	#[test]
	fn test_builder_exclusions() {
		let builder = SyncBuilder::new().exclude_patterns(vec!["*.tmp", ".git/*"]);

		assert_eq!(builder.config.exclude_patterns.len(), 2);
	}

	#[test]
	fn test_builder_conflict_resolution() {
		let builder = SyncBuilder::new()
			.add_location("./dir1")
			.conflict_resolution(ConflictResolution::PreferNewest);

		assert_eq!(builder.locations.len(), 1);
		assert_eq!(builder.config.conflict_resolution, ConflictResolution::PreferNewest);
	}

	#[test]
	fn test_builder_validation() {
		// Empty builder should fail validation
		let result = tokio::runtime::Runtime::new()
			.unwrap()
			.block_on(async { SyncBuilder::new().sync().await });

		assert!(result.is_err());
		match result {
			Err(SyncError::InvalidConfig { message }) => {
				assert!(message.contains("location"));
			}
			_ => panic!("Expected InvalidConfig error"),
		}
	}

	#[test]
	fn test_remote_location_detection() {
		let builder = SyncBuilder::new()
			.add_location("./local")
			.add_remote("remote.host:/path/to/dir")
			.add_location("../relative/path");

		assert_eq!(builder.locations.len(), 3);
		assert_eq!(builder.locations[0], "./local");
		assert_eq!(builder.locations[1], "remote.host:/path/to/dir");
		assert_eq!(builder.locations[2], "../relative/path");
	}

	#[test]
	fn test_builder_dry_run() {
		let builder = SyncBuilder::new().dry_run(true);
		assert!(builder.config.dry_run);

		let builder = builder.dry_run(false);
		assert!(!builder.config.dry_run);
	}

	#[test]
	fn test_builder_chunk_size() {
		let builder = SyncBuilder::new().chunk_size_bits(21);
		assert_eq!(builder.config.chunk_bits, 21);
	}
}

// vim: ts=4
