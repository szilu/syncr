#![allow(dead_code)]

//! Unified configuration system for SyncR
//!
//! This module consolidates all configuration into a single `Config` struct,
//! eliminating fragmentation across 27 previously separate config types.
//!
//! The configuration follows a priority chain:
//! 1. Built-in defaults (Config::default())
//! 2. Config file (~/.config/syncr/config.toml or config.json)
//! 3. Environment variables (SYNCR_* prefix)
//! 4. CLI flags (highest priority)

use crate::strategies::{ConflictResolution, DeleteMode, MetadataStrategy, SymlinkMode};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ============================================================================
// MAIN CONFIGURATION STRUCT
// ============================================================================

/// Unified configuration for SyncR synchronization operations
///
/// This is the single source of truth for all SyncR configuration,
/// consolidating settings that were previously scattered across:
/// - RuntimeConfig (state directory, profile)
/// - SyncCliOptions (CLI-facing options)
/// - chunking.rs SyncConfig (API configuration)
/// - config/types.rs Config (comprehensive config hierarchy)
/// - 24 config submodule types (conflicts, deletion, metadata, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
	// ========================================================================
	// RUNTIME & STATE MANAGEMENT
	// ========================================================================
	/// Home directory for SyncR state (~/.syncr)
	pub syncr_dir: PathBuf,

	/// Profile name for configuration and state isolation
	pub profile: String,

	// ========================================================================
	// EXCLUSION & INCLUSION
	// ========================================================================
	/// Glob patterns to exclude from sync (e.g., "*.tmp", "node_modules/")
	pub exclude_patterns: Vec<String>,

	/// Glob patterns that override exclusions
	pub include_patterns: Vec<String>,

	/// Honor .gitignore, .syncignore and similar files
	pub respect_ignore_files: bool,

	/// Custom ignore files to check (beyond .gitignore/.syncignore)
	pub custom_ignore_files: Vec<PathBuf>,

	// ========================================================================
	// SYNC BEHAVIOR
	// ========================================================================
	/// Dry run mode - plan changes without applying them
	pub dry_run: bool,

	/// Bidirectional sync (if false, primary → secondary only)
	pub bidirectional: bool,

	/// Follow symlinks instead of preserving them
	pub follow_symlinks: bool,

	// ========================================================================
	// CONFLICT RESOLUTION
	// ========================================================================
	/// Strategy for automatic conflict resolution
	pub conflict_resolution: ConflictResolution,

	/// Auto-resolve conflicts (vs asking user interactively)
	pub auto_resolve: bool,

	/// Remember user decisions for identical conflicts in future runs
	pub remember_decisions: bool,

	// ========================================================================
	// DELETION HANDLING
	// ========================================================================
	/// How to handle file deletions during sync
	pub delete_mode: DeleteMode,

	/// Enable deletion protection (warn on mass deletions)
	pub deletion_protection: bool,

	/// Maximum number of files to delete in a single sync
	pub max_delete_count: Option<usize>,

	/// Maximum percentage of files to delete (0-100)
	pub max_delete_percent: Option<u8>,

	/// Backup deleted files instead of removing them
	pub backup_deleted: bool,

	/// Directory to store backups (if backup_deleted is true)
	pub backup_dir: Option<PathBuf>,

	/// Suffix for backup files
	pub backup_suffix: String,

	// ========================================================================
	// METADATA HANDLING
	// ========================================================================
	/// Metadata comparison and preservation strategy
	pub metadata_strategy: MetadataStrategy,

	/// Preserve file modification times
	pub preserve_timestamps: bool,

	/// Preserve file permissions/mode bits
	pub preserve_permissions: bool,

	/// Preserve file ownership (user/group)
	pub preserve_ownership: bool,

	/// Ignore timestamp differences smaller than this (seconds)
	pub ignore_time_diff_secs: u64,

	/// Always use checksums for comparison (vs modification time)
	pub always_checksum: bool,

	// ========================================================================
	// SYMLINK HANDLING
	// ========================================================================
	/// How to handle symlinks during sync
	pub symlink_mode: SymlinkMode,

	/// Convert absolute symlinks to relative
	pub make_symlinks_relative: bool,

	/// Skip dangling symlinks (targets that don't exist)
	pub skip_dangling_symlinks: bool,

	/// Maximum symlink recursion depth
	pub max_symlink_depth: u32,

	// ============================================================================
	// HARD LINKS
	// ============================================================================
	/// Preserve hard links during sync
	pub preserve_hardlinks: bool,

	// ============================================================================
	// SPECIAL FILES
	// ============================================================================
	/// Handle special files (sockets, device files, FIFOs)
	pub handle_special_files: bool,

	// ========================================================================
	// CHUNKING & PERFORMANCE
	// ========================================================================
	/// Chunk size in bits (2^chunk_bits = average chunk size)
	pub chunk_bits: u32,

	/// Maximum chunk size factor (multiplied by 2^chunk_bits)
	pub max_chunk_size_factor: usize,

	/// Number of parallel transfers
	pub parallel_transfers: usize,

	/// Number of parallel hashing operations (0 = auto)
	pub parallel_hashing: usize,

	/// Buffer size for transfers
	pub buffer_size: usize,

	/// Parallelism for directory scanning
	pub scan_parallelism: usize,

	// ========================================================================
	// COMPRESSION
	// ========================================================================
	/// Enable compression for transfers
	pub compress: bool,

	/// Compression level (0-9 for most algorithms)
	pub compress_level: u8,

	/// Compression algorithm
	pub compress_algorithm: CompressionAlgorithm,

	/// Bandwidth limit (e.g., "1M", "100k")
	pub bandwidth_limit: Option<String>,

	// ========================================================================
	// CACHING
	// ========================================================================
	/// Enable caching of file metadata and chunks
	pub cache_enabled: bool,

	/// Cache directory (defaults to ~/.cache/syncr)
	pub cache_dir: Option<PathBuf>,

	/// Cache TTL in seconds
	pub cache_ttl_secs: u64,

	/// Maximum cache size in MB
	pub cache_max_size_mb: usize,

	/// Cache eviction policy
	pub cache_eviction_policy: EvictionPolicy,

	// ========================================================================
	// SAFETY
	// ========================================================================
	/// Detect and prevent collision attacks
	pub collision_detection: bool,

	/// Use atomic operations for commits
	pub atomic_operations: bool,

	// ========================================================================
	// OUTPUT & LOGGING
	// ========================================================================
	/// Output mode (TUI, CLI, Progress, Quiet, JSON)
	pub output_mode: OutputMode,

	/// Show progress during sync
	pub show_progress: bool,

	/// Progress update rate (Hz)
	pub progress_rate: f64,

	/// Log level (trace, debug, info, warn, error)
	pub log_level: String,

	/// Log format (JSON, Pretty, Compact)
	pub log_format: LogFormat,

	/// Path to log file (if any)
	pub log_file: Option<PathBuf>,

	/// Color output mode
	pub color_mode: ColorMode,

	/// Use Unicode characters in output
	pub use_unicode: bool,

	// ========================================================================
	// REMOTE / SSH CONFIGURATION
	// ========================================================================
	/// SSH-specific configuration
	pub ssh: SshConfig,

	// ========================================================================
	// HOOKS & CALLBACKS
	// ========================================================================
	/// Pre-sync hook script
	pub pre_sync_hook: Option<String>,

	/// Post-sync hook script
	pub post_sync_hook: Option<String>,

	/// Conflict callback script
	pub on_conflict_hook: Option<String>,
}

impl Default for Config {
	fn default() -> Self {
		Config {
			// Runtime
			syncr_dir: std::env::var("HOME")
				.ok()
				.map(|h| PathBuf::from(h).join(".syncr"))
				.unwrap_or_else(|| PathBuf::from(".syncr")),
			profile: "default".to_string(),

			// Exclusions
			exclude_patterns: vec![],
			include_patterns: vec![],
			respect_ignore_files: true,
			custom_ignore_files: vec![],

			// Sync behavior
			dry_run: false,
			bidirectional: true,
			follow_symlinks: false,

			// Conflict resolution
			conflict_resolution: ConflictResolution::Interactive,
			auto_resolve: false,
			remember_decisions: false,

			// Deletion
			delete_mode: DeleteMode::Sync,
			deletion_protection: true,
			max_delete_count: Some(1000),
			max_delete_percent: Some(50),
			backup_deleted: false,
			backup_dir: None,
			backup_suffix: ".syncr-deleted".to_string(),

			// Metadata
			metadata_strategy: MetadataStrategy::Smart,
			preserve_timestamps: true,
			preserve_permissions: true,
			preserve_ownership: true,
			ignore_time_diff_secs: 1,
			always_checksum: false,

			// Symlinks
			symlink_mode: SymlinkMode::Preserve,
			make_symlinks_relative: false,
			skip_dangling_symlinks: true,
			max_symlink_depth: 40,

			// Hard links
			preserve_hardlinks: true,

			// Special files
			handle_special_files: false,

			// Chunking
			chunk_bits: 20, // ~1MB chunks
			max_chunk_size_factor: 16,

			// Performance
			parallel_transfers: 4,
			parallel_hashing: 0, // 0 = auto
			buffer_size: 65536,
			scan_parallelism: 4,

			// Compression
			compress: false,
			compress_level: 6,
			compress_algorithm: CompressionAlgorithm::Zstd,
			bandwidth_limit: None,

			// Cache
			cache_enabled: true,
			cache_dir: None,
			cache_ttl_secs: 3600,
			cache_max_size_mb: 1024,
			cache_eviction_policy: EvictionPolicy::Lru,

			// Safety
			collision_detection: true,
			atomic_operations: true,

			// Output
			output_mode: OutputMode::Cli,
			show_progress: true,
			progress_rate: 0.5,
			log_level: "info".to_string(),
			log_format: LogFormat::Pretty,
			log_file: None,
			color_mode: ColorMode::Auto,
			use_unicode: true,

			// SSH
			ssh: SshConfig::default(),

			// Hooks
			pre_sync_hook: None,
			post_sync_hook: None,
			on_conflict_hook: None,
		}
	}
}

// ============================================================================
// NESTED CONFIGURATION STRUCTS (for complex subsystems)
// ============================================================================

/// SSH/Remote connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SshConfig {
	/// Custom SSH command (overrides default "ssh")
	pub ssh_command: Option<String>,

	/// Connection timeout in seconds
	pub connection_timeout: u64,

	/// SSH connection pool size
	pub pool_size: usize,

	/// Number of retries for failed connections
	pub retry_count: u32,

	/// Delay between retries in milliseconds
	pub retry_delay_ms: u64,

	/// Enable SSH compression
	pub compression: bool,

	/// Custom port (if not in location string)
	pub port: Option<u16>,
}

impl Default for SshConfig {
	fn default() -> Self {
		SshConfig {
			ssh_command: None,
			connection_timeout: 30,
			pool_size: 4,
			retry_count: 3,
			retry_delay_ms: 1000,
			compression: false,
			port: None,
		}
	}
}

/// Performance limits configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LimitsConfig {
	/// Maximum memory to use in MB
	pub max_memory_mb: usize,

	/// Maximum number of open files
	pub max_open_files: Option<usize>,

	/// I/O priority level
	pub io_priority: IoPriority,

	/// Process nice value (-20 to 19)
	pub nice: Option<i32>,

	/// Maximum CPU usage percentage (0-100)
	pub max_cpu_percent: Option<u8>,
}

impl Default for LimitsConfig {
	fn default() -> Self {
		LimitsConfig {
			max_memory_mb: 1024,
			max_open_files: None,
			io_priority: IoPriority::Normal,
			nice: None,
			max_cpu_percent: None,
		}
	}
}

/// Disk I/O configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DiskConfig {
	/// Disk cache mode
	pub cache_mode: CacheMode,

	/// Read-ahead buffer size
	pub read_ahead: Option<usize>,

	/// Use direct I/O (bypass OS cache)
	pub direct: bool,

	/// Use splice for zero-copy transfers
	pub splice: bool,
}

impl Default for DiskConfig {
	fn default() -> Self {
		DiskConfig { cache_mode: CacheMode::Async, read_ahead: None, direct: false, splice: true }
	}
}

// ============================================================================
// ENUMERATIONS (consolidated from config submodules)
// ============================================================================

/// Compression algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum CompressionAlgorithm {
	#[default]
	Zstd,
	Gzip,
	Lz4,
}

/// Chunking algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ChunkingAlgorithm {
	#[default]
	Bup, // Bupsplit rolling hash (current default)
	Fastcdc, // FastCDC (future)
	Fixed,   // Fixed-size chunks
}

/// I/O priority level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum IoPriority {
	Idle,
	Low,
	#[default]
	Normal,
	High,
}

/// Disk cache mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum CacheMode {
	Sync,
	#[default]
	Async,
	Direct,
}

/// Cache eviction policy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum EvictionPolicy {
	#[default]
	Lru, // Least recently used
	Lfu,  // Least frequently used
	Fifo, // First in, first out
}

/// Ownership preservation mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PreserveMode {
	#[default]
	Auto, // Preserve if capable (e.g., running as root)
	Always, // Always try to preserve
	Never,  // Never preserve
}

/// Metadata reconciliation mode for nodes with different capabilities
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ReconciliationMode {
	#[default]
	Lcd, // Least common denominator (default)
	BestEffort, // Each node preserves what it can
	SourceWins, // First node is authoritative
}

/// Output display mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum OutputMode {
	Tui, // Terminal UI
	#[default]
	Cli, // CLI with interactive prompts
	Progress, // Progress bar only
	Quiet, // Silent mode
	Json, // Machine-readable JSON
}

/// Log format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LogFormat {
	Json,
	#[default]
	Pretty,
	Compact,
}

/// Color output mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ColorMode {
	#[default]
	Auto,
	Always,
	Never,
}

// ============================================================================
// TYPE ALIASES FOR BACKWARD COMPATIBILITY
// ============================================================================

/// Alias for runtime configuration (Phase 1 compatibility)
pub type RuntimeConfig = Config;

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_config_default() {
		let config = Config::default();
		assert_eq!(config.profile, "default");
		assert_eq!(config.chunk_bits, 20);
		assert!(!config.dry_run);
		assert!(config.show_progress);
	}

	#[test]
	fn test_ssh_config_default() {
		let ssh = SshConfig::default();
		assert_eq!(ssh.connection_timeout, 30);
		assert_eq!(ssh.pool_size, 4);
		assert_eq!(ssh.retry_count, 3);
	}

	#[test]
	fn test_config_serialization() {
		let config = Config::default();
		let json = serde_json::to_string(&config).expect("Failed to serialize");
		let deserialized: Config = serde_json::from_str(&json).expect("Failed to deserialize");
		assert_eq!(config.profile, deserialized.profile);
		assert_eq!(config.chunk_bits, deserialized.chunk_bits);
	}
}

// vim: ts=4
