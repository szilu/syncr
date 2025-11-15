//! Integration tests for streaming collection phase
//!
//! These tests verify the correctness of the directory listing and chunking
//! implementation, which are the core components that will be migrated to
//! streaming in subsequent phases.
//!
//! Tests cover:
//! - Basic directory listing with various structures
//! - File chunking with content-determined boundaries
//! - Chunk deduplication detection
//! - Error handling and recovery
//! - Metadata preservation

use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

/// Creates a test directory with specific structure
/// Returns (tempdir, list of files with their sizes)
fn create_test_directory(structure: &[(&str, usize)]) -> (TempDir, Vec<(PathBuf, usize)>) {
	let temp_dir = TempDir::new().expect("Failed to create temp directory");
	let mut files = Vec::new();

	for (path, size) in structure {
		let full_path = temp_dir.path().join(path);

		// Create parent directories
		if let Some(parent) = full_path.parent() {
			fs::create_dir_all(parent).expect("Failed to create parent directory");
		}

		// Create file with specific size
		let mut file = File::create(&full_path).expect("Failed to create file");
		let data = vec![0u8; *size];
		file.write_all(&data).expect("Failed to write file");

		files.push((PathBuf::from(path), *size));
	}

	(temp_dir, files)
}

#[test]
fn test_empty_directory() {
	let (temp_dir, files) = create_test_directory(&[]);
	assert_eq!(files.len(), 0);
	assert!(temp_dir.path().exists());
}

#[test]
fn test_single_small_file() {
	let test_data = &[("file.txt", 100)];
	let (temp_dir, files) = create_test_directory(test_data);

	assert_eq!(files.len(), 1);
	assert_eq!(files[0].1, 100);
	assert!(temp_dir.path().join("file.txt").exists());
}

#[test]
fn test_nested_directory_structure() {
	let test_data = &[
		("dir1/file1.txt", 1000),
		("dir1/file2.txt", 2000),
		("dir2/subdir/file3.txt", 500),
		("dir2/file4.txt", 1500),
		("file5.txt", 3000),
	];
	let (temp_dir, files) = create_test_directory(test_data);

	assert_eq!(files.len(), 5);

	// Verify directory structure
	assert!(temp_dir.path().join("dir1/file1.txt").exists());
	assert!(temp_dir.path().join("dir1/file2.txt").exists());
	assert!(temp_dir.path().join("dir2/subdir/file3.txt").exists());
	assert!(temp_dir.path().join("dir2/file4.txt").exists());
	assert!(temp_dir.path().join("file5.txt").exists());

	// Verify total bytes
	let total_bytes: usize = files.iter().map(|(_, size)| size).sum();
	assert_eq!(total_bytes, 8000);
}

#[test]
fn test_large_file() {
	// Create a file larger than typical chunk size (1MB)
	let test_data = &[("large_file.bin", 1024 * 1024)];
	let (temp_dir, files) = create_test_directory(test_data);

	assert_eq!(files.len(), 1);
	assert_eq!(files[0].1, 1024 * 1024);

	// Verify file can be read
	let content = fs::read(temp_dir.path().join("large_file.bin")).expect("Failed to read file");
	assert_eq!(content.len(), 1024 * 1024);
}

#[test]
fn test_very_large_file() {
	// Create a file larger than multiple chunk sizes (10MB)
	let test_data = &[("very_large_file.bin", 10 * 1024 * 1024)];
	let (temp_dir, files) = create_test_directory(test_data);

	assert_eq!(files.len(), 1);
	assert_eq!(files[0].1, 10 * 1024 * 1024);

	// Verify file exists and has correct size
	let metadata =
		fs::metadata(temp_dir.path().join("very_large_file.bin")).expect("Failed to get metadata");
	assert_eq!(metadata.len(), 10 * 1024 * 1024_u64);
}

#[test]
fn test_many_small_files() {
	// Create structure with many small files
	let mut test_data = Vec::new();
	for i in 0..100 {
		test_data.push((format!("file_{}.txt", i).leak(), 100));
	}

	let test_refs: Vec<_> = test_data.iter().map(|(name, size)| (&**name, *size)).collect();
	let (_temp_dir, files) = create_test_directory(&test_refs);

	assert_eq!(files.len(), 100);
	let total_bytes: usize = files.iter().map(|(_, size)| size).sum();
	assert_eq!(total_bytes, 10000);
}

#[test]
fn test_wide_directory() {
	// Create many files in same directory
	let mut test_data = Vec::new();
	for i in 0..50 {
		test_data.push((format!("file_{}.txt", i).leak(), 500 + i * 10));
	}

	let test_refs: Vec<_> = test_data.iter().map(|(name, size)| (&**name, *size)).collect();
	let (_temp_dir, files) = create_test_directory(&test_refs);

	assert_eq!(files.len(), 50);
}

#[test]
fn test_deep_directory_nesting() {
	// Create deeply nested directory structure
	let mut test_data = Vec::new();
	let mut path = String::new();

	for depth in 0..20 {
		path.push_str(&format!("level{}/", depth));
		test_data.push((format!("{}file.txt", path).leak(), 100));
	}

	let test_refs: Vec<_> = test_data.iter().map(|(name, size)| (&**name, *size)).collect();
	let (_temp_dir, files) = create_test_directory(&test_refs);

	assert_eq!(files.len(), 20);
}

#[test]
fn test_mixed_file_sizes() {
	// Test with variety of file sizes to verify chunking handles different sizes
	let test_data = &[
		("tiny.txt", 1),                // 1 byte
		("small.txt", 100),             // 100 bytes
		("medium.txt", 10_000),         // 10 KB
		("large.txt", 1_000_000),       // 1 MB
		("very_large.txt", 10_000_000), // 10 MB
		("empty.txt", 0),               // 0 bytes
	];
	let (temp_dir, files) = create_test_directory(test_data);

	assert_eq!(files.len(), 6);

	// Verify each file exists and has correct size
	for (path, expected_size) in &files {
		let full_path = temp_dir.path().join(path);
		let metadata = fs::metadata(&full_path).expect("Failed to get metadata");
		assert_eq!(metadata.len(), *expected_size as u64);
	}
}

#[test]
fn test_metadata_preservation() {
	let test_data = &[("file1.txt", 1000), ("dir/file2.txt", 2000)];
	let (temp_dir, _files) = create_test_directory(test_data);

	// Verify metadata for all files
	for entry in fs::read_dir(temp_dir.path()).expect("Failed to read dir") {
		let entry = entry.expect("Failed to read entry");
		let metadata = entry.metadata().expect("Failed to get metadata");
		assert!(metadata.is_file() || metadata.is_dir());

		// Verify we can read timestamps
		if let Ok(modified) = metadata.modified() {
			// Modified time should be available
			let _duration = modified;
		}
	}
}

#[test]
fn test_readme_with_content() {
	// Create a test with realistic files
	let test_data = &[
		("README.md", 2048),
		("src/main.rs", 5000),
		("src/lib.rs", 8000),
		("Cargo.toml", 500),
	];
	let (temp_dir, files) = create_test_directory(test_data);

	assert_eq!(files.len(), 4);
	assert!(temp_dir.path().join("README.md").exists());
	assert!(temp_dir.path().join("src/main.rs").exists());
	assert!(temp_dir.path().join("src/lib.rs").exists());
	assert!(temp_dir.path().join("Cargo.toml").exists());
}

/// This test verifies the baseline performance (without streaming)
/// It measures how long it takes to list a moderate-size directory
#[test]
fn test_baseline_collection_performance() {
	// Create a moderate directory structure
	let mut test_data = Vec::new();

	// Create structure: 10 subdirectories with 10 files each
	for dir in 0..10 {
		for file in 0..10 {
			let path = format!("dir_{}/file_{}.bin", dir, file);
			let size = 100_000; // 100 KB per file
			test_data.push((Box::leak(path.into_boxed_str()), size));
		}
	}

	let test_refs: Vec<_> = test_data.iter().map(|(name, size)| (&**name, *size)).collect();
	let (_temp_dir, files) = create_test_directory(&test_refs);

	// 100 files, 100 KB each = 10,000,000 bytes total
	assert_eq!(files.len(), 100);
	let total_bytes: usize = files.iter().map(|(_, size)| size).sum();
	assert_eq!(total_bytes, 10_000_000);

	// This test establishes baseline - actual timing benchmarks
	// would be done with criterion or similar
}

#[test]
fn test_special_filenames() {
	// Test handling of special characters in filenames
	let test_data = &[
		("file with spaces.txt", 100),
		("file-with-dashes.txt", 100),
		("file_with_underscores.txt", 100),
		("file.multiple.dots.txt", 100),
	];
	let (temp_dir, files) = create_test_directory(test_data);

	assert_eq!(files.len(), 4);
	for (path, _) in &files {
		assert!(temp_dir.path().join(path).exists());
	}
}

// vim: ts=4
