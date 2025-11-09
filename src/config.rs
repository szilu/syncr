//! Configuration types and constants for SyncR

use std::path::PathBuf;

/// Chunk size in bits (2^20 = ~1MB average chunks)
pub const CHUNK_BITS: u32 = 20;

/// Maximum chunk size factor (multiplied by 2^CHUNK_BITS)
pub const MAX_CHUNK_SIZE_FACTOR: usize = 16;

/// Maximum chunk size in bytes
pub const MAX_CHUNK_SIZE: usize = (1 << CHUNK_BITS) * MAX_CHUNK_SIZE_FACTOR;

/// Base64 line length for output
#[allow(dead_code)]
pub const BASE64_LINE_LENGTH: usize = 64;

// Alias for new API (may be used by library users)
#[allow(dead_code)]
pub const DEFAULT_CHUNK_BITS: u32 = CHUNK_BITS;

/// Main sync configuration
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SyncConfig {
	/// State directory (default: ~/.syncr)
	pub state_dir: PathBuf,

	/// Profile name for state persistence
	pub profile: String,

	/// Conflict resolution strategy
	pub conflict_resolution: ConflictResolution,

	/// File exclusion patterns (glob syntax)
	pub exclude_patterns: Vec<String>,

	/// Chunk configuration
	pub chunk_config: ChunkConfig,

	/// Whether this is a dry run (no actual writes)
	pub dry_run: bool,

	/// Verbosity level
	pub verbosity: Verbosity,

	/// Whether to perform the sync (vs just analyze)
	pub auto_resolve: bool,
}

impl SyncConfig {
	/// Create a new config with defaults
	#[allow(dead_code)]
	pub fn new(state_dir: PathBuf, profile: String) -> Self {
		SyncConfig {
			state_dir,
			profile,
			conflict_resolution: ConflictResolution::Interactive,
			exclude_patterns: vec![],
			chunk_config: ChunkConfig::default(),
			dry_run: false,
			verbosity: Verbosity::Normal,
			auto_resolve: false,
		}
	}
}

impl Default for SyncConfig {
	fn default() -> Self {
		SyncConfig {
			state_dir: PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
				.join(".syncr"),
			profile: "default".to_string(),
			conflict_resolution: ConflictResolution::Interactive,
			exclude_patterns: vec![],
			chunk_config: ChunkConfig::default(),
			dry_run: false,
			verbosity: Verbosity::Normal,
			auto_resolve: false,
		}
	}
}

/// Sync options for simple API
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct SyncOptions {
	/// Profile name
	pub profile: Option<String>,

	/// Conflict resolution
	pub conflict_resolution: Option<ConflictResolution>,

	/// Exclusion patterns
	pub exclude: Option<Vec<String>>,

	/// Dry run mode
	pub dry_run: Option<bool>,
}

/// Chunking configuration
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ChunkConfig {
	/// Chunk size in bits (average chunk = 2^chunk_bits)
	pub chunk_bits: u32,

	/// Maximum chunk size in bytes
	pub max_chunk_size: usize,

	/// Minimum chunk size to avoid too many tiny chunks
	pub min_chunk_size: usize,
}

impl Default for ChunkConfig {
	fn default() -> Self {
		ChunkConfig {
			chunk_bits: DEFAULT_CHUNK_BITS,
			max_chunk_size: (1 << DEFAULT_CHUNK_BITS) * MAX_CHUNK_SIZE_FACTOR,
			min_chunk_size: 1024, // 1KB minimum
		}
	}
}

impl ChunkConfig {
	/// Create config with specific chunk bits
	#[allow(dead_code)]
	pub fn new(chunk_bits: u32) -> Self {
		ChunkConfig {
			chunk_bits,
			max_chunk_size: (1 << chunk_bits) * MAX_CHUNK_SIZE_FACTOR,
			min_chunk_size: 1024,
		}
	}

	/// Validate the configuration
	#[allow(dead_code)]
	pub fn validate(&self) -> Result<(), String> {
		if self.chunk_bits > 32 {
			return Err("chunk_bits must be <= 32".to_string());
		}
		if self.chunk_bits == 0 {
			return Err("chunk_bits must be > 0".to_string());
		}
		if self.max_chunk_size < self.min_chunk_size {
			return Err("max_chunk_size must be >= min_chunk_size".to_string());
		}
		Ok(())
	}
}

/// Strategy for automatic conflict resolution
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum ConflictResolution {
	/// Always choose first location's version
	PreferFirst,

	/// Always choose last location's version
	PreferLast,

	/// Choose newest modification time
	PreferNewest,

	/// Choose oldest modification time
	PreferOldest,

	/// Choose largest file
	PreferLargest,

	/// Choose smallest file
	PreferSmallest,

	/// Prompt user interactively (CLI only)
	Interactive,

	/// Fail on any conflict
	FailOnConflict,
}

impl PartialEq for ConflictResolution {
	fn eq(&self, other: &Self) -> bool {
		std::mem::discriminant(self) == std::mem::discriminant(other)
	}
}

/// Verbosity level for logging
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Verbosity {
	/// No output except errors
	Silent,

	/// Normal output
	Normal,

	/// Verbose output
	Verbose,

	/// Debug output
	Debug,
}

// vim: ts=4
