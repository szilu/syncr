//! Synchronization API - high-level and mid-level interfaces

use crate::config::{ConflictResolution, SyncConfig, SyncOptions};
use crate::error::SyncError;
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
	_locations: Vec<&str>,
	_options: Option<SyncOptions>,
) -> Result<SyncResult, SyncError> {
	// TODO: Implement high-level sync using the underlying sync implementation
	Err(SyncError::Other { message: "High-level sync API not yet implemented".to_string() })
}

/// Builder for flexible sync configuration
pub struct SyncBuilder {
	locations: Vec<String>,
	config: SyncConfig,
}

impl SyncBuilder {
	/// Create a new sync builder with default settings
	pub fn new() -> Self {
		SyncBuilder { locations: Vec::new(), config: SyncConfig::default() }
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
		self.config.chunk_config.chunk_bits = bits;
		self
	}

	/// Set custom state directory (default: ~/.syncr)
	pub fn state_dir(mut self, path: &str) -> Self {
		self.config.state_dir = PathBuf::from(path);
		self
	}

	/// Enable/disable dry run mode
	pub fn dry_run(mut self, enabled: bool) -> Self {
		self.config.dry_run = enabled;
		self
	}

	/// Execute the sync operation
	pub async fn sync(self) -> Result<SyncResult, SyncError> {
		if self.locations.is_empty() {
			return Err(SyncError::InvalidConfig {
				message: "At least one location is required".to_string(),
			});
		}

		// TODO: Implement builder-based sync
		Err(SyncError::Other { message: "SyncBuilder::sync() not yet implemented".to_string() })
	}
}

impl Default for SyncBuilder {
	fn default() -> Self {
		Self::new()
	}
}

/// Mid-level sync API with control over individual phases
pub mod phases {
	use crate::error::SyncError;
	use crate::types::SyncResult;

	/// Represents an active sync session with control over individual phases
	pub struct SyncSession {
		_locations: Vec<String>,
	}

	impl SyncSession {
		/// Create a new sync session
		pub async fn new(_locations: Vec<&str>) -> Result<Self, SyncError> {
			// TODO: Implement session creation
			Err(SyncError::Other { message: "SyncSession not yet implemented".to_string() })
		}

		/// Phase 1: Collect file metadata and chunks from all locations
		pub async fn collect(&mut self) -> Result<CollectionStats, SyncError> {
			// TODO: Implement collection phase
			Err(SyncError::Other { message: "Collect phase not yet implemented".to_string() })
		}

		/// Phase 2: Detect conflicts across all locations
		pub async fn detect_conflicts(&self) -> Result<Vec<crate::conflict::Conflict>, SyncError> {
			// TODO: Implement conflict detection
			Err(SyncError::Other { message: "Conflict detection not yet implemented".to_string() })
		}

		/// Resolve a specific conflict by choosing a winner
		pub fn resolve_conflict(
			&mut self,
			_conflict_id: u64,
			_winner_index: usize,
		) -> Result<(), SyncError> {
			// TODO: Implement conflict resolution
			Err(SyncError::Other { message: "Conflict resolution not yet implemented".to_string() })
		}

		/// Batch resolve conflicts using a strategy
		pub fn resolve_all_conflicts(
			&mut self,
			_strategy: crate::config::ConflictResolution,
		) -> Result<(), SyncError> {
			// TODO: Implement batch resolution
			Err(SyncError::Other {
				message: "Batch conflict resolution not yet implemented".to_string(),
			})
		}

		/// Phase 3: Transfer file/directory metadata to nodes
		pub async fn transfer_metadata(&mut self) -> Result<MetadataStats, SyncError> {
			// TODO: Implement metadata transfer
			Err(SyncError::Other { message: "Metadata transfer not yet implemented".to_string() })
		}

		/// Phase 4: Transfer missing chunks between nodes
		pub async fn transfer_chunks(&mut self) -> Result<ChunkStats, SyncError> {
			// TODO: Implement chunk transfer
			Err(SyncError::Other { message: "Chunk transfer not yet implemented".to_string() })
		}

		/// Phase 5: Commit all changes (rename temp files to final)
		pub async fn commit(self) -> Result<SyncResult, SyncError> {
			// TODO: Implement commit
			Err(SyncError::Other { message: "Commit not yet implemented".to_string() })
		}

		/// Abort the sync session (cleanup temp files)
		pub async fn abort(self) -> Result<(), SyncError> {
			// TODO: Implement abort
			Err(SyncError::Other { message: "Abort not yet implemented".to_string() })
		}
	}

	/// Statistics from collection phase
	#[derive(Debug, Clone)]
	pub struct CollectionStats {
		/// Number of files found
		pub files_found: usize,

		/// Total size of files
		pub total_size: u64,

		/// Number of chunks created
		pub chunks_created: usize,
	}

	/// Statistics from metadata transfer phase
	#[derive(Debug, Clone)]
	pub struct MetadataStats {
		/// Number of files to create
		pub files_to_create: usize,

		/// Number of files to delete
		pub files_to_delete: usize,

		/// Number of directories to create
		pub dirs_to_create: usize,
	}

	/// Statistics from chunk transfer phase
	#[derive(Debug, Clone)]
	pub struct ChunkStats {
		/// Number of chunks transferred
		pub chunks_transferred: usize,

		/// Number of chunks deduplicated
		pub chunks_deduplicated: usize,

		/// Total bytes transferred
		pub bytes_transferred: u64,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

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
}
