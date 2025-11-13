//! Configuration types and constants for SyncR
#![allow(dead_code)]

use crate::strategies::ConflictResolution;

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

/// Sync options for simple API
#[derive(Debug, Clone, Default)]
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
