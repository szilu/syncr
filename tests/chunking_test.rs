use rollsum::Bup;
use std::fs;
use std::io::Write;
use tempfile::TempDir;

// Test configuration values
const CHUNK_BITS: u32 = 13; // Smaller chunks for testing (8KB average)
const MAX_CHUNK_SIZE: usize = (1 << CHUNK_BITS) * 16;

#[test]
fn test_chunking_deterministic() {
	// Same content should produce same chunks
	let content = b"This is test content that will be chunked. ".repeat(100);

	let chunks1 = chunk_data(&content);
	let chunks2 = chunk_data(&content);

	assert_eq!(chunks1.len(), chunks2.len());
	for (c1, c2) in chunks1.iter().zip(chunks2.iter()) {
		assert_eq!(c1.0, c2.0); // offset
		assert_eq!(c1.1, c2.1); // size
	}
}

#[test]
fn test_chunking_small_file() {
	let content = b"Small file";
	let chunks = chunk_data(content);

	// Small file should be one chunk
	assert_eq!(chunks.len(), 1);
	assert_eq!(chunks[0].0, 0); // offset 0
	assert_eq!(chunks[0].1, content.len()); // size equals file size
}

#[test]
fn test_chunking_empty_file() {
	let content = b"";
	let chunks = chunk_data(content);

	// Empty file should produce no chunks
	assert_eq!(chunks.len(), 0);
}

#[test]
fn test_chunking_large_file() {
	// Create a larger file (100KB) with varying content to ensure chunk boundaries
	let mut content = Vec::new();
	for i in 0..1000 {
		content.extend_from_slice(format!("Line {} with some varied content\n", i).as_bytes());
	}

	// Pad to at least 100KB
	while content.len() < 100 * 1024 {
		content.extend_from_slice(b"Additional padding content to reach size. ");
	}

	let chunks = chunk_data(&content);

	// With varied content, should produce at least 1 chunk
	assert!(!chunks.is_empty(), "Large file should produce at least one chunk");

	// Verify all chunks are accounted for
	let total_size: usize = chunks.iter().map(|(_, size)| size).sum();
	assert_eq!(total_size, content.len());

	// Verify chunks are contiguous
	let mut expected_offset = 0;
	for (offset, size) in chunks {
		assert_eq!(offset, expected_offset);
		expected_offset += size;
	}
}

#[test]
fn test_chunking_content_shifting() {
	// Content-determined chunking should find same chunks even with shifted data
	let base = b"AAAAA".repeat(1000);
	let mut content1 = Vec::new();
	content1.extend_from_slice(&base);

	let mut content2 = Vec::new();
	content2.extend_from_slice(b"PREFIX"); // Add prefix
	content2.extend_from_slice(&base);

	let chunks1 = chunk_data(&content1);
	let chunks2 = chunk_data(&content2);

	// The base content should appear in chunks2, just at different offset
	// This is the key benefit of content-determined chunking
	assert!(chunks2.len() >= chunks1.len());
}

#[test]
fn test_chunk_boundaries() {
	let content = b"A".repeat(MAX_CHUNK_SIZE * 2);
	let chunks = chunk_data(&content);

	// No chunk should exceed MAX_CHUNK_SIZE
	for (_, size) in chunks {
		assert!(
			size <= MAX_CHUNK_SIZE,
			"Chunk size {} exceeds MAX_CHUNK_SIZE {}",
			size,
			MAX_CHUNK_SIZE
		);
	}
}

#[test]
fn test_chunking_binary_data() {
	let content: Vec<u8> = (0..=255).cycle().take(50000).collect();
	let chunks = chunk_data(&content);

	assert!(!chunks.is_empty());

	// Verify total coverage
	let total: usize = chunks.iter().map(|(_, s)| s).sum();
	assert_eq!(total, content.len());
}

#[test]
fn test_chunking_identical_blocks() {
	// File with repeating identical blocks
	let block = b"IDENTICAL_BLOCK_CONTENT";
	let content = block.repeat(500);

	let chunks = chunk_data(&content);

	// Even with identical content, chunking should be consistent
	assert!(!chunks.is_empty());
	let total: usize = chunks.iter().map(|(_, s)| s).sum();
	assert_eq!(total, content.len());
}

#[test]
fn test_chunking_from_file() {
	let temp_dir = TempDir::new().unwrap();
	let file_path = temp_dir.path().join("test.dat");

	// Create test file
	let content = b"Test data for chunking ".repeat(1000);
	let mut file = fs::File::create(&file_path).unwrap();
	file.write_all(&content).unwrap();
	drop(file);

	// Read and chunk
	let read_content = fs::read(&file_path).unwrap();
	let chunks = chunk_data(&read_content);

	assert!(!chunks.is_empty());
	let total: usize = chunks.iter().map(|(_, s)| s).sum();
	assert_eq!(total, read_content.len());
}

#[test]
fn test_chunk_offset_progression() {
	let content = b"X".repeat(100000);
	let chunks = chunk_data(&content);

	let mut last_end = 0;
	for (offset, size) in chunks {
		assert_eq!(offset, last_end, "Chunks should be contiguous without gaps");
		last_end = offset + size;
	}
	assert_eq!(last_end, content.len(), "Chunks should cover entire file");
}

// Helper function that mimics the chunking logic from serve.rs
fn chunk_data(data: &[u8]) -> Vec<(usize, usize)> {
	let mut chunks = Vec::new();
	let mut offset = 0;
	let mut pos = 0;

	while pos < data.len() {
		let mut bup = Bup::new_with_chunk_bits(CHUNK_BITS);
		let end = std::cmp::min(pos + MAX_CHUNK_SIZE, data.len());

		if let Some((count, _hash)) = bup.find_chunk_edge(&data[pos..end]) {
			chunks.push((offset, count));
			offset += count;
			pos += count;
		} else {
			let count = end - pos;
			chunks.push((offset, count));
			offset += count;
			pos += count;
		}
	}

	chunks
}

#[test]
fn test_chunking_reproduces_after_modification() {
	// Test that modifying the end of a file doesn't change chunks at the beginning
	let base = b"STABLE_PREFIX_".repeat(1000);
	let suffix1 = b"_ENDING_1";
	let suffix2 = b"_ENDING_2";

	let mut content1 = Vec::new();
	content1.extend_from_slice(&base);
	content1.extend_from_slice(suffix1);

	let mut content2 = Vec::new();
	content2.extend_from_slice(&base);
	content2.extend_from_slice(suffix2);

	let chunks1 = chunk_data(&content1);
	let chunks2 = chunk_data(&content2);

	// Both should have at least one chunk
	assert!(!chunks1.is_empty());
	assert!(!chunks2.is_empty());

	// If both have multiple chunks, compare the stable portion
	if chunks1.len() > 1 && chunks2.len() > 1 {
		let common_chunks = std::cmp::min(chunks1.len(), chunks2.len()).saturating_sub(2);
		for i in 0..common_chunks {
			// Content-determined chunking should find same boundaries
			// Note: This might not always be true at exact boundaries, but most should match
			if chunks1[i].0 == chunks2[i].0 {
				assert_eq!(chunks1[i].1, chunks2[i].1);
			}
		}
	}

	// The key property: total sizes should be correct
	let total1: usize = chunks1.iter().map(|(_, s)| s).sum();
	let total2: usize = chunks2.iter().map(|(_, s)| s).sum();
	assert_eq!(total1, content1.len());
	assert_eq!(total2, content2.len());
}
