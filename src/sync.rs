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

	/// Get the number of locations configured
	pub fn location_count(&self) -> usize {
		self.locations.len()
	}

	/// Get a reference to the locations
	pub fn locations(&self) -> &[String] {
		&self.locations
	}

	/// Get a reference to the sync configuration
	pub fn config(&self) -> &SyncConfig {
		&self.config
	}

	/// Execute the sync operation
	pub async fn sync(self) -> Result<SyncResult, SyncError> {
		if self.locations.is_empty() {
			return Err(SyncError::InvalidConfig {
				message: "At least one location is required".to_string(),
			});
		}

		// Convert locations to &str for connection
		let locations: Vec<&str> = self.locations.iter().map(|s| s.as_str()).collect();

		// Create and execute sync session
		let mut session = phases::SyncSession::new(locations).await.map_err(|e| {
			SyncError::Other { message: format!("Failed to initialize sync: {}", e) }
		})?;

		// Run through all phases
		session
			.collect()
			.await
			.map_err(|e| SyncError::Other { message: format!("Collection failed: {}", e) })?;

		// Detect conflicts
		let conflicts = session.detect_conflicts().await.map_err(|e| SyncError::Other {
			message: format!("Conflict detection failed: {}", e),
		})?;

		// Resolve conflicts based on strategy
		if !conflicts.is_empty() {
			session.resolve_all_conflicts(self.config.conflict_resolution.clone()).map_err(
				|e| SyncError::Other { message: format!("Conflict resolution failed: {}", e) },
			)?;
		}

		// Transfer metadata
		session.transfer_metadata().await.map_err(|e| SyncError::Other {
			message: format!("Metadata transfer failed: {}", e),
		})?;

		// Transfer chunks
		session
			.transfer_chunks()
			.await
			.map_err(|e| SyncError::Other { message: format!("Chunk transfer failed: {}", e) })?;

		// Commit changes
		session
			.commit()
			.await
			.map_err(|e| SyncError::Other { message: format!("Commit failed: {}", e) })
	}
}

impl Default for SyncBuilder {
	fn default() -> Self {
		Self::new()
	}
}

/// Mid-level sync API with control over individual phases
pub mod phases {
	use crate::config::ConflictResolution;
	use crate::connection;
	use crate::error::SyncError;
	use crate::types::SyncResult;

	/// Represents an active sync session with control over individual phases
	#[allow(dead_code)]
	pub struct SyncSession {
		nodes: Vec<connection::Node>,
		collected: bool,
		conflicts_detected: Vec<ConflictInfo>,
	}

	#[derive(Debug, Clone)]
	#[allow(dead_code)]
	struct ConflictInfo {
		path: String,
	}

	impl SyncSession {
		/// Create a new sync session
		pub async fn new(locations: Vec<&str>) -> Result<Self, SyncError> {
			if locations.is_empty() {
				return Err(SyncError::InvalidConfig {
					message: "At least one location required".to_string(),
				});
			}

			// Connect to all nodes in parallel
			let mut nodes =
				connection::connect_all(locations).await.map_err(SyncError::Connection)?;

			// Perform handshake with each node
			for node in &mut nodes {
				Self::handshake(node).await.map_err(|e| SyncError::Other {
					message: format!("Handshake failed: {}", e),
				})?;
			}

			Ok(SyncSession { nodes, collected: false, conflicts_detected: Vec::new() })
		}

		async fn handshake(node: &mut connection::Node) -> Result<(), Box<dyn std::error::Error>> {
			use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

			// Read ready signal
			let mut buf = String::new();
			node.stdout().read_line(&mut buf).await?;
			if buf.trim() != "." {
				return Err("Expected ready signal from server".into());
			}

			// Send protocol version
			node.stdin().write_all(b"VERSION:1\n").await?;

			// Read server version
			buf.clear();
			node.stdout().read_line(&mut buf).await?;

			if !buf.starts_with("VERSION:") {
				return Err("Invalid handshake response".into());
			}

			Ok(())
		}

		/// Phase 1: Collect file metadata and chunks from all locations
		pub async fn collect(&mut self) -> Result<CollectionStats, SyncError> {
			// Basic implementation: just mark as collected
			// In a full implementation, this would:
			// 1. Send LIST command to each node
			// 2. Parse responses
			// 3. Build a map of files and chunks
			self.collected = true;

			Ok(CollectionStats { files_found: 0, total_size: 0, chunks_created: 0 })
		}

		/// Phase 2: Detect conflicts across all locations
		pub async fn detect_conflicts(&self) -> Result<Vec<crate::conflict::Conflict>, SyncError> {
			// In a full implementation, this would compare file metadata across nodes
			// For now, return empty (no conflicts detected)
			Ok(Vec::new())
		}

		/// Resolve a specific conflict by choosing a winner
		pub fn resolve_conflict(
			&mut self,
			_conflict_id: u64,
			_winner_index: usize,
		) -> Result<(), SyncError> {
			Ok(())
		}

		/// Batch resolve conflicts using a strategy
		pub fn resolve_all_conflicts(
			&mut self,
			_strategy: ConflictResolution,
		) -> Result<(), SyncError> {
			// In a full implementation, this would apply the conflict resolution strategy
			// to all detected conflicts
			Ok(())
		}

		/// Phase 3: Transfer file/directory metadata to nodes
		pub async fn transfer_metadata(&mut self) -> Result<MetadataStats, SyncError> {
			// In a full implementation, this would:
			// 1. Send WRITE commands to nodes that need file/directory creation
			// 2. Track statistics
			Ok(MetadataStats { files_to_create: 0, files_to_delete: 0, dirs_to_create: 0 })
		}

		/// Phase 4: Transfer missing chunks between nodes
		pub async fn transfer_chunks(&mut self) -> Result<ChunkStats, SyncError> {
			// In a full implementation, this would:
			// 1. Identify missing chunks on each node
			// 2. Transfer them from nodes that have them
			// 3. Track statistics
			Ok(ChunkStats { chunks_transferred: 0, chunks_deduplicated: 0, bytes_transferred: 0 })
		}

		/// Phase 5: Commit all changes (rename temp files to final)
		pub async fn commit(self) -> Result<SyncResult, SyncError> {
			// In a full implementation, this would:
			// 1. Send COMMIT command to each node
			// 2. Verify all commits succeeded
			// 3. Return final statistics

			Ok(SyncResult {
				files_synced: 0,
				dirs_created: 0,
				files_deleted: 0,
				bytes_transferred: 0,
				chunks_transferred: 0,
				chunks_deduplicated: 0,
				conflicts_encountered: 0,
				conflicts_resolved: 0,
				duration: std::time::Duration::from_secs(0),
				errors: Vec::new(),
			})
		}

		/// Abort the sync session (cleanup temp files)
		pub async fn abort(self) -> Result<(), SyncError> {
			// In a full implementation, this would send cleanup commands to nodes
			Ok(())
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
	use crate::config::ConflictResolution;

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
		assert_eq!(builder.config.chunk_config.chunk_bits, 21);
	}
}
