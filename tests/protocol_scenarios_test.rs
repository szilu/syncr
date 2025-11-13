//! Protocol scenario tests
//!
//! Tests protocol behavior through end-to-end sync operations.
//! These tests exercise the protocol layer by performing actual syncs
//! and verifying the results match the expected behavior.
//!
//! This approach tests:
//! - LIST command: File enumeration and chunking
//! - READ command: Chunk retrieval
//! - WRITE command: File creation
//! - COMMIT command: Atomic finalization

use std::fs;
use std::path::Path;
use tempfile::TempDir;

use syncr::sync::SyncBuilder;

// ============================================================================
// Helper Functions
// ============================================================================

fn create_file(dir: &Path, name: &str, content: &[u8]) {
	let path = dir.join(name);
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent).ok();
	}
	fs::write(&path, content).unwrap();
}

fn create_dir(dir: &Path, name: &str) {
	fs::create_dir(dir.join(name)).ok();
}

fn read_file(dir: &Path, name: &str) -> Option<Vec<u8>> {
	let path = dir.join(name);
	fs::read(&path).ok()
}

fn file_exists(dir: &Path, name: &str) -> bool {
	dir.join(name).exists()
}

// ============================================================================
// Protocol Tests via Sync Operations
// ============================================================================

// ===== LIST Command Tests (File Enumeration) =====

#[tokio::test]
async fn test_list_empty_directory() {
	// Protocol LIST: Empty directory should enumerate cleanly
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	// Sync empty directory - should complete without errors
	let result = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await;

	assert!(result.is_ok(), "Sync should succeed for empty directories");
}

#[tokio::test]
async fn test_list_single_file() {
	// Protocol LIST: Single file is enumerated and transferred
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	create_file(dir1.path(), "test.txt", b"Hello, World!");

	let result = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await;

	assert!(result.is_ok(), "Sync should succeed");

	// Verify file was transferred
	assert!(file_exists(dir2.path(), "test.txt"), "File should be transferred");
	assert_eq!(
		read_file(dir2.path(), "test.txt"),
		Some(b"Hello, World!".to_vec()),
		"Content should match"
	);
}

#[tokio::test]
async fn test_list_multiple_files() {
	// Protocol LIST: Multiple files are enumerated
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	create_file(dir1.path(), "file1.txt", b"content1");
	create_file(dir1.path(), "file2.txt", b"content2");
	create_file(dir1.path(), "file3.txt", b"content3");

	let result = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await;

	assert!(result.is_ok(), "Sync should succeed");

	// All files should be transferred
	assert!(file_exists(dir2.path(), "file1.txt"));
	assert!(file_exists(dir2.path(), "file2.txt"));
	assert!(file_exists(dir2.path(), "file3.txt"));
}

#[tokio::test]
async fn test_list_nested_directories() {
	// Protocol LIST: Nested directory structure is preserved
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	create_dir(dir1.path(), "subdir");
	create_file(dir1.path().join("subdir").as_path(), "nested.txt", b"nested content");
	create_file(dir1.path(), "root.txt", b"root content");

	let result = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await;

	assert!(result.is_ok(), "Sync should succeed");

	// Verify directory structure
	assert!(dir2.path().join("subdir").is_dir(), "Subdirectory should exist");
	assert!(file_exists(dir2.path().join("subdir").as_path(), "nested.txt"));
	assert!(file_exists(dir2.path(), "root.txt"));
}

#[tokio::test]
async fn test_list_mixed_files_and_dirs() {
	// Protocol LIST: Mixed files and directories
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	create_file(dir1.path(), "file.txt", b"content");
	create_dir(dir1.path(), "dir1");
	create_dir(dir1.path(), "dir2");
	create_file(dir1.path().join("dir1").as_path(), "nested.txt", b"nested");

	let result = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await;

	assert!(result.is_ok());
	assert!(file_exists(dir2.path(), "file.txt"));
	assert!(dir2.path().join("dir1").is_dir());
	assert!(dir2.path().join("dir2").is_dir());
}

// ===== Chunking Tests =====

#[tokio::test]
async fn test_chunking_small_file() {
	// Protocol LIST/READ: Small file is chunked and transferred correctly
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	let content = b"small";
	create_file(dir1.path(), "small.txt", content);

	let result = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await;

	assert!(result.is_ok());
	assert_eq!(read_file(dir2.path(), "small.txt"), Some(content.to_vec()));
}

#[tokio::test]
async fn test_chunking_large_file() {
	// Protocol LIST/READ: Large file is chunked and transferred correctly
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	let content = vec![b'A'; 1024 * 1024]; // 1MB
	create_file(dir1.path(), "large.bin", &content);

	let result = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await;

	assert!(result.is_ok());

	let transferred = read_file(dir2.path(), "large.bin").unwrap();
	assert_eq!(transferred.len(), content.len(), "Size should match");
	assert_eq!(transferred, content, "Content should match");
}

#[tokio::test]
async fn test_chunking_multiple_files() {
	// Protocol LIST/READ: Multiple files use chunking correctly
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	for i in 0..5 {
		let content = vec![b'X'; (i + 1) * 10000];
		create_file(dir1.path(), &format!("file{}.bin", i), &content);
	}

	let result = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await;

	assert!(result.is_ok());

	// Verify all files
	for i in 0..5 {
		let name = format!("file{}.bin", i);
		assert!(file_exists(dir2.path(), &name), "File {} should exist", name);
	}
}

#[tokio::test]
async fn test_chunking_hash_consistency() {
	// Protocol LIST: Same file content should produce consistent hashes
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	let content = b"consistent content for hashing";
	create_file(dir1.path(), "hash_test.txt", content);

	// First sync
	SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await
		.unwrap();

	// Verify content transferred
	assert_eq!(read_file(dir2.path(), "hash_test.txt"), Some(content.to_vec()));

	// Second sync should also work (verify hash is consistent)
	let result2 = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await;

	assert!(result2.is_ok(), "Second sync should also work");
}

// ===== WRITE Command Tests (File Creation) =====

#[tokio::test]
async fn test_write_create_file() {
	// Protocol WRITE: File is created with correct content
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	create_file(dir1.path(), "new_file.txt", b"new content");

	let result = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await;

	assert!(result.is_ok());
	assert_eq!(read_file(dir2.path(), "new_file.txt"), Some(b"new content".to_vec()));
}

#[tokio::test]
async fn test_write_create_nested_path() {
	// Protocol WRITE: Nested directories are created
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	create_dir(dir1.path(), "level1");
	create_dir(dir1.path().join("level1").as_path(), "level2");
	create_file(dir1.path().join("level1").join("level2").as_path(), "deep.txt", b"deep content");

	let result = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await;

	assert!(result.is_ok());
	assert!(file_exists(dir2.path().join("level1").join("level2").as_path(), "deep.txt"));
}

#[tokio::test]
async fn test_write_create_directory() {
	// Protocol WRITE: Directories are created
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	create_dir(dir1.path(), "empty_dir");

	let result = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await;

	assert!(result.is_ok());
	assert!(dir2.path().join("empty_dir").is_dir());
}

#[tokio::test]
async fn test_write_overwrite_file() {
	// Protocol WRITE/COMMIT: Files are overwritten correctly
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	// Initial file
	create_file(dir1.path(), "file.txt", b"original");

	// First sync
	SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await
		.unwrap();

	// Update file
	create_file(dir1.path(), "file.txt", b"updated");

	// Second sync
	let result = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await;

	assert!(result.is_ok());
	assert_eq!(read_file(dir2.path(), "file.txt"), Some(b"updated".to_vec()));
}

// ===== COMMIT Command Tests (Atomicity) =====

#[tokio::test]
async fn test_commit_atomic_finalization() {
	// Protocol COMMIT: Changes are atomically finalized
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	create_file(dir1.path(), "file1.txt", b"content1");
	create_file(dir1.path(), "file2.txt", b"content2");
	create_file(dir1.path(), "file3.txt", b"content3");

	let result = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await;

	assert!(result.is_ok());

	// All files should exist and be visible
	assert!(file_exists(dir2.path(), "file1.txt"));
	assert!(file_exists(dir2.path(), "file2.txt"));
	assert!(file_exists(dir2.path(), "file3.txt"));

	// No temporary files should remain
	for entry in fs::read_dir(dir2.path()).unwrap() {
		let entry = entry.unwrap();
		let file_name = entry.file_name();
		let file_name_str = file_name.to_string_lossy();
		assert!(!file_name_str.ends_with(".SyNcR-TmP"), "No temporary files should remain");
	}
}

#[tokio::test]
async fn test_commit_temp_files_cleaned() {
	// Protocol COMMIT: Temporary files are cleaned up
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	create_file(dir1.path(), "test.txt", b"content");

	let result = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await;

	assert!(result.is_ok());

	// Count temporary files
	let temp_files: Vec<_> = fs::read_dir(dir2.path())
		.unwrap()
		.filter_map(|e| e.ok())
		.filter(|e| e.file_name().to_string_lossy().ends_with(".SyNcR-TmP"))
		.collect();

	assert_eq!(temp_files.len(), 0, "All temporary files should be cleaned up");
}

// ===== Bidirectional Sync Tests =====

#[tokio::test]
async fn test_bidirectional_sync() {
	// Protocol: Bidirectional sync transfers files both ways
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	create_file(dir1.path(), "from_dir1.txt", b"content1");
	create_file(dir2.path(), "from_dir2.txt", b"content2");

	let result = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await;

	assert!(result.is_ok());

	// Files should be transferred both directions
	assert!(file_exists(dir2.path(), "from_dir1.txt"));
	assert!(file_exists(dir1.path(), "from_dir2.txt"));
}

#[tokio::test]
async fn test_multiway_sync() {
	// Protocol: Multi-way sync among three directories
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let dir3 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	create_file(dir1.path(), "file1.txt", b"content1");
	create_file(dir2.path(), "file2.txt", b"content2");
	create_file(dir3.path(), "file3.txt", b"content3");

	let result = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.add_location(dir3.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await;

	assert!(result.is_ok());

	// All files should be present in all directories
	for dir in &[dir1.path(), dir2.path(), dir3.path()] {
		assert!(file_exists(dir, "file1.txt"));
		assert!(file_exists(dir, "file2.txt"));
		assert!(file_exists(dir, "file3.txt"));
	}
}

// ===== Consistency Tests =====

#[tokio::test]
async fn test_protocol_consistency_idempotent() {
	// Protocol: Syncing twice produces same result (idempotent)
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	create_file(dir1.path(), "file.txt", b"content");

	// First sync
	SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await
		.unwrap();

	let first_content = read_file(dir2.path(), "file.txt");

	// Second sync (no changes)
	SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await
		.unwrap();

	let second_content = read_file(dir2.path(), "file.txt");

	assert_eq!(first_content, second_content, "Content should be consistent");
}

#[tokio::test]
async fn test_protocol_no_corruption() {
	// Protocol: File content is never corrupted (binary files)
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	// Binary data with all byte values
	let binary_data: Vec<u8> = (0..=255).collect();
	create_file(dir1.path(), "binary.bin", &binary_data);

	let result = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state.path().to_str().unwrap())
		.sync()
		.await;

	assert!(result.is_ok());
	assert_eq!(
		read_file(dir2.path(), "binary.bin"),
		Some(binary_data),
		"Binary data should not be corrupted"
	);
}
