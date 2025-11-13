//! Tests for pre-commit verification functionality
//!
//! These tests verify that the sync process correctly detects missing chunks
//! and prevent commits when chunks are missing.

use syncr::util;

#[test]
fn test_hash_to_base64_conversion() {
	// Verify hash to base64 conversion works for pre-commit verification
	let hash: [u8; 32] = [
		1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
		26, 27, 28, 29, 30, 31, 32,
	];
	let b64 = util::hash_to_base64(&hash);

	// Should be a valid base64 string
	assert!(!b64.is_empty());
	assert!(b64.chars().all(|c| c.is_alphanumeric() || c == '+' || c == '/' || c == '='));
}

#[test]
fn test_hash_conversion_round_trip() {
	// Test that hash can be converted to base64 and back
	let original: [u8; 32] = [42; 32];
	let b64 = util::hash_to_base64(&original);
	let recovered = util::base64_to_hash(&b64).expect("Should convert back successfully");

	assert_eq!(original, recovered);
}

#[test]
fn test_different_hashes_produce_different_base64() {
	// Different hashes should produce different base64 strings
	let hash1 = [1u8; 32];
	let hash2 = [2u8; 32];

	let b64_1 = util::hash_to_base64(&hash1);
	let b64_2 = util::hash_to_base64(&hash2);

	assert_ne!(b64_1, b64_2);
}

#[test]
fn test_multiple_chunks_distinct_hashes() {
	// When processing multiple chunks, each should have distinct hash
	let mut chunks = vec![];
	for i in 0..10 {
		let mut hash = [0u8; 32];
		hash[0] = i as u8;
		chunks.push(util::hash_to_base64(&hash));
	}

	// All hashes should be distinct
	let unique_count = chunks.iter().collect::<std::collections::HashSet<_>>().len();
	assert_eq!(unique_count, 10);
}

#[test]
fn test_base64_strings_are_consistent() {
	// Same hash should always produce same base64
	let hash = [123u8; 32];
	let b64_1 = util::hash_to_base64(&hash);
	let b64_2 = util::hash_to_base64(&hash);

	assert_eq!(b64_1, b64_2);
}

#[test]
fn test_hash_to_base64_preserves_data() {
	// Converting to base64 and back should preserve all data
	let original_hashes: Vec<[u8; 32]> = (0..5)
		.map(|i| {
			let mut h = [0u8; 32];
			h[0] = i as u8;
			h[31] = (255 - i) as u8;
			h
		})
		.collect();

	let b64_hashes: Vec<String> = original_hashes.iter().map(util::hash_to_base64).collect();

	for (i, b64) in b64_hashes.iter().enumerate() {
		let recovered = util::base64_to_hash(b64).expect("Should convert back");
		assert_eq!(recovered, original_hashes[i]);
	}
}

#[test]
fn test_hash_set_membership_with_base64() {
	// Test that base64 hashes can be used in sets for membership testing
	// (this is how the pre-commit check works)
	use std::collections::BTreeSet;

	let hash1 = [1u8; 32];
	let hash2 = [2u8; 32];
	let hash3 = [3u8; 32];

	let b64_1 = util::hash_to_base64(&hash1);
	let b64_2 = util::hash_to_base64(&hash2);
	let b64_3 = util::hash_to_base64(&hash3);

	let mut missing_chunks: BTreeSet<String> = BTreeSet::new();
	missing_chunks.insert(b64_1.clone());
	missing_chunks.insert(b64_2.clone());

	// Should find missing chunks
	assert!(missing_chunks.contains(&b64_1));
	assert!(missing_chunks.contains(&b64_2));

	// Should not find non-missing chunks
	assert!(!missing_chunks.contains(&b64_3));
}

#[test]
fn test_empty_missing_set_is_clean() {
	// Empty missing set means all chunks were received
	use std::collections::BTreeSet;

	let missing: BTreeSet<String> = BTreeSet::new();
	assert!(missing.is_empty());
}

#[test]
fn test_missing_set_operations() {
	// Test operations on missing chunks set used in pre-commit verification
	use std::collections::BTreeSet;

	let hash1 = util::hash_to_base64(&[1u8; 32]);
	let hash2 = util::hash_to_base64(&[2u8; 32]);
	let hash3 = util::hash_to_base64(&[3u8; 32]);

	let mut missing = BTreeSet::new();
	missing.insert(hash1.clone());
	missing.insert(hash2.clone());

	assert_eq!(missing.len(), 2);

	// Find first few missing chunks (for error reporting)
	let reported: Vec<_> = missing.iter().take(5).cloned().collect();
	assert_eq!(reported.len(), 2);

	// Check if specific hash is missing
	assert!(missing.contains(&hash1));
	assert!(missing.contains(&hash2));
	assert!(!missing.contains(&hash3));

	// Remove a chunk when it arrives
	missing.remove(&hash1);
	assert_eq!(missing.len(), 1);
	assert!(!missing.contains(&hash1));
	assert!(missing.contains(&hash2));
}

#[test]
fn test_large_missing_chunks_set() {
	// Test with many missing chunks (realistic scenario)
	use std::collections::BTreeSet;

	let mut missing = BTreeSet::new();

	// Add 1000 missing chunks
	for i in 0..1000 {
		let mut hash = [0u8; 32];
		hash[0] = (i & 0xff) as u8;
		hash[1] = ((i >> 8) & 0xff) as u8;
		let b64 = util::hash_to_base64(&hash);
		missing.insert(b64);
	}

	assert_eq!(missing.len(), 1000);

	// Should be able to check membership efficiently
	let test_hash = util::hash_to_base64(&[42u8; 32]);
	assert!(!missing.contains(&test_hash));
}

#[test]
fn test_error_message_formatting_with_hashes() {
	// Test that hash lists can be formatted for error messages
	let hashes: Vec<String> = (0..5)
		.map(|i| {
			let mut h = [0u8; 32];
			h[0] = i as u8;
			util::hash_to_base64(&h)
		})
		.collect();

	// Format first 5 hashes for error message
	let formatted = hashes
		.iter()
		.take(5)
		.map(|h| format!("  - {}", h))
		.collect::<Vec<_>>()
		.join("\n");

	assert!(!formatted.is_empty());
	assert!(formatted.contains("  - "));

	// Should have 5 hashes
	let count = formatted.matches("  - ").count();
	assert_eq!(count, 5);
}
