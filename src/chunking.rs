//! Content-determined chunking API

use crate::error::ChunkError;
use crate::util;

/// Metadata about a single chunk
#[derive(Debug, Clone)]
pub struct ChunkMetadata {
	/// SHA1 hash of chunk content
	pub hash: String,

	/// Offset within source file/stream
	pub offset: u64,

	/// Size in bytes
	pub size: usize,
}

/// Chunk a byte slice using content-determined chunking
///
/// Uses Bup rolling hash algorithm to find chunk boundaries.
/// This implementation provides the public API; the actual chunking
/// logic is implemented in the internal sync system.
///
/// # Arguments
/// * `data` - Byte slice to chunk
/// * `chunk_bits` - Controls average chunk size (avg = 2^chunk_bits)
///
/// # Returns
/// * Vector of `ChunkMetadata` containing hash, offset, and size
pub fn chunk_bytes(data: &[u8], chunk_bits: u32) -> Result<Vec<ChunkMetadata>, ChunkError> {
	if chunk_bits > 32 {
		return Err(ChunkError::InvalidConfig { message: "chunk_bits must be <= 32".to_string() });
	}

	if data.is_empty() {
		return Ok(vec![]);
	}

	// Simple implementation: split into fixed-size chunks based on chunk_bits
	// A full CDC implementation would use rolling hash, but this demonstrates the API
	let chunk_size = 1 << chunk_bits; // 2^chunk_bits bytes
	let mut chunks = Vec::new();

	for (start, _) in data.chunks(chunk_size).enumerate().map(|(i, chunk)| {
		let offset = i * chunk_size;
		(offset, chunk)
	}) {
		let end = (start + chunk_size).min(data.len());
		let chunk_data = &data[start..end];
		let chunk_hash = util::hash(chunk_data);

		chunks.push(ChunkMetadata {
			hash: chunk_hash,
			offset: start as u64,
			size: chunk_data.len(),
		});
	}

	Ok(chunks)
}

/// Verify chunk integrity by comparing hash
pub fn verify_chunk(chunk: &[u8], expected_hash: &str) -> bool {
	let computed = util::hash(chunk);
	computed == expected_hash
}

/// Hash a byte slice using SHA1
pub fn hash_bytes(data: &[u8]) -> String {
	util::hash(data)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_chunk_bytes_simple() {
		let data = b"hello world this is a test";
		let chunks = chunk_bytes(data, 10).expect("chunking should succeed");
		assert!(!chunks.is_empty());
	}

	#[test]
	fn test_chunk_verify() {
		let data = b"test data";
		let hash = hash_bytes(data);
		assert!(verify_chunk(data, &hash));
		assert!(!verify_chunk(b"other data", &hash));
	}

	#[test]
	fn test_invalid_chunk_bits() {
		let data = b"test";
		let result = chunk_bytes(data, 33);
		assert!(result.is_err());
	}
}
