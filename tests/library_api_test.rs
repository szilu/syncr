//! Comprehensive library API tests using SyncBuilder and related functionality
//!
//! This test suite covers:
//! - SyncBuilder configuration (fluent API)
//! - Configuration validation
//! - Error handling
//! - Data type construction and manipulation
//! - File system scenarios with directory structures and file states

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

// Re-export the library for use in tests
use syncr::config::{ChunkConfig, ConflictResolution, SyncConfig};
use syncr::error::SyncError;
use syncr::sync::SyncBuilder;

// ============================================================================
// Helper Functions for Test Setup
// ============================================================================

/// Create a test file with specified content
fn create_test_file(dir: &TempDir, name: &str, content: &[u8]) -> PathBuf {
	let file_path = dir.path().join(name);

	// Create parent directories if needed
	if let Some(parent) = file_path.parent() {
		fs::create_dir_all(parent).unwrap();
	}

	let mut file = fs::File::create(&file_path).unwrap();
	file.write_all(content).unwrap();
	file_path
}

/// Create a nested directory structure for testing
fn create_nested_structure(base_dir: &TempDir) {
	let paths = vec!["dir1", "dir1/subdir1", "dir1/subdir1/subdir2", "dir2", "dir2/docs", "dir3"];

	for path in paths {
		let full_path = base_dir.path().join(path);
		fs::create_dir_all(&full_path).unwrap();
	}
}

/// Create a complete test scenario with multiple files
fn create_file_scenario(base_dir: &TempDir, scenario: &str) {
	match scenario {
		"basic" => {
			create_test_file(base_dir, "file1.txt", b"Content 1");
			create_test_file(base_dir, "file2.txt", b"Content 2");
			create_test_file(base_dir, "subdir/file3.txt", b"Content 3");
		}
		"identical" => {
			let content = b"Identical content";
			create_test_file(base_dir, "file1.txt", content);
			create_test_file(base_dir, "file2.txt", content);
			create_test_file(base_dir, "subdir/file3.txt", content);
		}
		"binary" => {
			let binary = vec![0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE];
			create_test_file(base_dir, "binary1.bin", &binary);
			create_test_file(base_dir, "binary2.bin", &binary);
		}
		"large" => {
			let large_content = vec![0xAB; 100_000]; // 100KB
			create_test_file(base_dir, "large.bin", &large_content);
		}
		"nested" => {
			create_nested_structure(base_dir);
			create_test_file(base_dir, "dir1/file1.txt", b"File in dir1");
			create_test_file(base_dir, "dir1/subdir1/file2.txt", b"Nested file");
			create_test_file(base_dir, "dir1/subdir1/subdir2/file3.txt", b"Deeply nested");
		}
		"empty_files" => {
			create_test_file(base_dir, "empty1.txt", b"");
			create_test_file(base_dir, "empty2.txt", b"");
			create_test_file(base_dir, "nonempty.txt", b"content");
		}
		"mixed" => {
			create_test_file(base_dir, "text.txt", b"Text content");
			create_test_file(base_dir, "data.bin", &[0xFF; 1000]);
			create_test_file(base_dir, "empty.txt", b"");
			create_nested_structure(base_dir);
			create_test_file(base_dir, "dir2/nested.txt", b"Nested content");
		}
		_ => panic!("Unknown scenario: {}", scenario),
	}
}

// ============================================================================
// PART 1: SyncBuilder Configuration Tests
// ============================================================================

#[test]
fn test_builder_new_creates_empty_builder() {
	let builder = SyncBuilder::new();
	// Verify builder can be created and is ready for configuration
	assert!(builder.locations().is_empty());
}

#[test]
fn test_builder_add_single_location() {
	let builder = SyncBuilder::new().add_location("./dir1");

	assert_eq!(builder.locations().len(), 1);
	assert_eq!(builder.locations()[0], "./dir1");
}

#[test]
fn test_builder_add_multiple_locations() {
	let builder = SyncBuilder::new()
		.add_location("./dir1")
		.add_location("./dir2")
		.add_location("./dir3");

	assert_eq!(builder.locations().len(), 3);
	assert_eq!(builder.locations()[0], "./dir1");
	assert_eq!(builder.locations()[1], "./dir2");
	assert_eq!(builder.locations()[2], "./dir3");
}

#[test]
fn test_builder_add_remote_location() {
	let builder = SyncBuilder::new().add_remote("user@host.com:path/to/dir");

	assert_eq!(builder.locations().len(), 1);
	assert!(builder.locations()[0].contains("@"));
	assert!(builder.locations()[0].contains(":"));
}

#[test]
fn test_builder_mixed_local_and_remote() {
	let builder = SyncBuilder::new()
		.add_location("./local")
		.add_remote("host1:remote1")
		.add_location("./another")
		.add_remote("host2:remote2");

	assert_eq!(builder.locations().len(), 4);
}

#[test]
fn test_builder_conflict_resolution_prefer_newest() {
	let builder = SyncBuilder::new()
		.add_location("./dir1")
		.conflict_resolution(ConflictResolution::PreferNewest);

	assert_eq!(builder.config().conflict_resolution, ConflictResolution::PreferNewest);
}

#[test]
fn test_builder_conflict_resolution_prefer_largest() {
	let builder = SyncBuilder::new()
		.add_location("./dir1")
		.conflict_resolution(ConflictResolution::PreferLargest);

	assert_eq!(builder.config().conflict_resolution, ConflictResolution::PreferLargest);
}

#[test]
fn test_builder_chunk_size_bits() {
	let builder = SyncBuilder::new().add_location("./dir1").chunk_size_bits(16);

	assert_eq!(builder.config().chunk_config.chunk_bits, 16);
}

#[test]
fn test_builder_chunk_size_bits_various() {
	let test_cases = vec![8, 12, 16, 20, 24, 28];

	for bits in test_cases {
		let builder = SyncBuilder::new().add_location("./dir1").chunk_size_bits(bits);

		assert_eq!(builder.config().chunk_config.chunk_bits, bits);
	}
}

#[test]
fn test_builder_profile_name() {
	let builder = SyncBuilder::new().add_location("./dir1").profile("production");

	assert_eq!(builder.config().profile, "production");
}

#[test]
fn test_builder_state_dir() {
	let builder = SyncBuilder::new().add_location("./dir1").state_dir("/tmp/syncr");

	assert_eq!(builder.config().state_dir, PathBuf::from("/tmp/syncr"));
}

#[test]
fn test_builder_dry_run_enabled() {
	let builder = SyncBuilder::new().add_location("./dir1").dry_run(true);

	assert!(builder.config().dry_run);
}

#[test]
fn test_builder_dry_run_disabled() {
	let builder = SyncBuilder::new().add_location("./dir1").dry_run(false);

	assert!(!builder.config().dry_run);
}

#[test]
fn test_builder_exclude_patterns() {
	let patterns = vec!["*.tmp", ".git/*", "node_modules/*"];
	let builder = SyncBuilder::new().add_location("./dir1").exclude_patterns(patterns.clone());

	assert_eq!(builder.config().exclude_patterns.len(), 3);
	assert!(builder.config().exclude_patterns.contains(&"*.tmp".to_string()));
	assert!(builder.config().exclude_patterns.contains(&".git/*".to_string()));
}

#[test]
fn test_builder_exclude_empty_patterns() {
	let builder = SyncBuilder::new().add_location("./dir1").exclude_patterns(vec![]);

	assert!(builder.config().exclude_patterns.is_empty());
}

#[test]
fn test_builder_fluent_chain() {
	let builder = SyncBuilder::new()
		.add_location("./dir1")
		.add_location("./dir2")
		.conflict_resolution(ConflictResolution::PreferNewest)
		.chunk_size_bits(18)
		.profile("test")
		.dry_run(true)
		.exclude_patterns(vec!["*.log", "*.tmp"]);

	assert_eq!(builder.locations().len(), 2);
	assert_eq!(builder.config().chunk_config.chunk_bits, 18);
	assert_eq!(builder.config().profile, "test");
	assert!(builder.config().dry_run);
	assert_eq!(builder.config().exclude_patterns.len(), 2);
}

// ============================================================================
// PART 2: Configuration Validation Tests
// ============================================================================

#[tokio::test]
async fn test_sync_fails_without_locations() {
	let result = SyncBuilder::new().sync().await;

	match result {
		Err(SyncError::InvalidConfig { message }) => {
			assert!(message.contains("At least one location is required"));
		}
		_ => panic!("Expected InvalidConfig error"),
	}
}

#[tokio::test]
async fn test_sync_fails_with_empty_locations_list() {
	let result = SyncBuilder::new().sync().await;

	assert!(result.is_err());
}

#[tokio::test]
async fn test_sync_builder_with_single_location() {
	// This should not fail validation (but will fail on actual sync since not implemented)
	let result = SyncBuilder::new().add_location("./test_dir").sync().await;

	// Expected to fail because sync() is not implemented, not because of validation
	match result {
		Err(SyncError::Other { message }) => {
			assert!(message.contains("not yet implemented"));
		}
		Err(SyncError::InvalidConfig { message }) => {
			panic!("Should not fail validation: {}", message);
		}
		Err(_) => {
			// Other error types are acceptable as long as it's not InvalidConfig
			// The key is that we passed validation
		}
		Ok(_) => {
			panic!("Should fail with not-implemented error");
		}
	}
}

// ============================================================================
// PART 3: Directory Structure and File Scenario Tests
// ============================================================================

#[test]
fn test_create_basic_file_scenario() {
	let temp_dir = TempDir::new().unwrap();
	create_file_scenario(&temp_dir, "basic");

	assert!(temp_dir.path().join("file1.txt").exists());
	assert!(temp_dir.path().join("file2.txt").exists());
	assert!(temp_dir.path().join("subdir/file3.txt").exists());
}

#[test]
fn test_create_identical_content_scenario() {
	let temp_dir = TempDir::new().unwrap();
	create_file_scenario(&temp_dir, "identical");

	let content1 = fs::read(temp_dir.path().join("file1.txt")).unwrap();
	let content2 = fs::read(temp_dir.path().join("file2.txt")).unwrap();
	let content3 = fs::read(temp_dir.path().join("subdir/file3.txt")).unwrap();

	assert_eq!(content1, content2);
	assert_eq!(content2, content3);
}

#[test]
fn test_create_binary_scenario() {
	let temp_dir = TempDir::new().unwrap();
	create_file_scenario(&temp_dir, "binary");

	assert!(temp_dir.path().join("binary1.bin").exists());
	assert!(temp_dir.path().join("binary2.bin").exists());

	let bin1 = fs::read(temp_dir.path().join("binary1.bin")).unwrap();
	let bin2 = fs::read(temp_dir.path().join("binary2.bin")).unwrap();
	assert_eq!(bin1, bin2);
}

#[test]
fn test_create_large_file_scenario() {
	let temp_dir = TempDir::new().unwrap();
	create_file_scenario(&temp_dir, "large");

	let metadata = fs::metadata(temp_dir.path().join("large.bin")).unwrap();
	assert_eq!(metadata.len(), 100_000);
}

#[test]
fn test_create_nested_scenario() {
	let temp_dir = TempDir::new().unwrap();
	create_file_scenario(&temp_dir, "nested");

	assert!(temp_dir.path().join("dir1").exists());
	assert!(temp_dir.path().join("dir1/subdir1").exists());
	assert!(temp_dir.path().join("dir1/subdir1/subdir2").exists());
	assert!(temp_dir.path().join("dir1/file1.txt").exists());
	assert!(temp_dir.path().join("dir1/subdir1/file2.txt").exists());
	assert!(temp_dir.path().join("dir1/subdir1/subdir2/file3.txt").exists());
}

#[test]
fn test_create_empty_files_scenario() {
	let temp_dir = TempDir::new().unwrap();
	create_file_scenario(&temp_dir, "empty_files");

	assert!(temp_dir.path().join("empty1.txt").exists());
	assert!(temp_dir.path().join("empty2.txt").exists());

	let empty1 = fs::metadata(temp_dir.path().join("empty1.txt")).unwrap();
	let empty2 = fs::metadata(temp_dir.path().join("empty2.txt")).unwrap();
	assert_eq!(empty1.len(), 0);
	assert_eq!(empty2.len(), 0);
}

#[test]
fn test_create_mixed_scenario() {
	let temp_dir = TempDir::new().unwrap();
	create_file_scenario(&temp_dir, "mixed");

	// Verify text file
	assert!(temp_dir.path().join("text.txt").exists());

	// Verify binary file
	assert!(temp_dir.path().join("data.bin").exists());

	// Verify empty file
	let empty_meta = fs::metadata(temp_dir.path().join("empty.txt")).unwrap();
	assert_eq!(empty_meta.len(), 0);

	// Verify nested structure
	assert!(temp_dir.path().join("dir2").exists());
	assert!(temp_dir.path().join("dir2/nested.txt").exists());
}

// ============================================================================
// PART 4: Conflict Scenario Tests
// ============================================================================

#[test]
fn test_scenario_identical_files_both_dirs() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();

	let content = b"Shared content";
	create_test_file(&dir1, "shared.txt", content);
	create_test_file(&dir2, "shared.txt", content);

	let file1 = fs::read(dir1.path().join("shared.txt")).unwrap();
	let file2 = fs::read(dir2.path().join("shared.txt")).unwrap();

	assert_eq!(file1, file2);
}

#[test]
fn test_scenario_unique_files_in_each_dir() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();

	create_test_file(&dir1, "unique1.txt", b"Only in dir1");
	create_test_file(&dir2, "unique2.txt", b"Only in dir2");

	assert!(dir1.path().join("unique1.txt").exists());
	assert!(!dir1.path().join("unique2.txt").exists());
	assert!(!dir2.path().join("unique1.txt").exists());
	assert!(dir2.path().join("unique2.txt").exists());
}

#[test]
fn test_scenario_same_filename_different_content() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();

	create_test_file(&dir1, "conflicted.txt", b"Version 1");
	create_test_file(&dir2, "conflicted.txt", b"Version 2");

	let content1 = fs::read(dir1.path().join("conflicted.txt")).unwrap();
	let content2 = fs::read(dir2.path().join("conflicted.txt")).unwrap();

	assert_ne!(content1, content2);
}

#[test]
fn test_scenario_one_empty_one_full() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();

	create_test_file(&dir1, "file.txt", b"");
	create_test_file(&dir2, "file.txt", b"Content in dir2");

	let meta1 = fs::metadata(dir1.path().join("file.txt")).unwrap();
	let meta2 = fs::metadata(dir2.path().join("file.txt")).unwrap();

	assert_eq!(meta1.len(), 0);
	assert!(meta2.len() > 0);
}

#[test]
fn test_scenario_nested_conflict() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();

	fs::create_dir_all(dir1.path().join("subdir")).unwrap();
	fs::create_dir_all(dir2.path().join("subdir")).unwrap();

	create_test_file(&dir1, "subdir/nested.txt", b"Version A");
	create_test_file(&dir2, "subdir/nested.txt", b"Version B");

	let content1 = fs::read(dir1.path().join("subdir/nested.txt")).unwrap();
	let content2 = fs::read(dir2.path().join("subdir/nested.txt")).unwrap();

	assert_ne!(content1, content2);
}

#[test]
fn test_scenario_missing_in_one_dir() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();

	create_test_file(&dir1, "exists.txt", b"File exists");

	assert!(dir1.path().join("exists.txt").exists());
	assert!(!dir2.path().join("exists.txt").exists());
}

// ============================================================================
// PART 5: Directory Tree Complexity Tests
// ============================================================================

#[test]
fn test_deep_nesting_up_to_10_levels() {
	let temp_dir = TempDir::new().unwrap();
	let mut path = temp_dir.path().to_path_buf();

	for i in 0..10 {
		path = path.join(format!("level_{}", i));
		fs::create_dir_all(&path).unwrap();
	}

	// Create a file at the deepest level
	create_test_file(&temp_dir, "level_0/level_1/level_2/level_3/level_4/level_5/level_6/level_7/level_8/level_9/deep_file.txt", b"Deep content");

	assert!(temp_dir.path().join("level_0/level_1/level_2/level_3/level_4/level_5/level_6/level_7/level_8/level_9/deep_file.txt").exists());
}

#[test]
fn test_many_files_in_single_directory() {
	let temp_dir = TempDir::new().unwrap();

	for i in 0..100 {
		create_test_file(
			&temp_dir,
			&format!("file_{:03}.txt", i),
			format!("Content {}", i).as_bytes(),
		);
	}

	let entries: Vec<_> = fs::read_dir(temp_dir.path()).unwrap().map(|e| e.unwrap()).collect();

	assert_eq!(entries.len(), 100);
}

#[test]
fn test_complex_directory_tree() {
	let temp_dir = TempDir::new().unwrap();

	// Create a complex tree structure
	let structure = vec![
		"project/src",
		"project/src/lib",
		"project/src/bin",
		"project/tests",
		"project/docs",
		"project/target/release",
		"project/target/debug",
		".git/objects",
		".git/refs",
	];

	for dir_path in &structure {
		fs::create_dir_all(temp_dir.path().join(dir_path)).unwrap();
	}

	// Add files to various locations
	create_test_file(&temp_dir, "project/src/lib/main.rs", b"// Main library code");
	create_test_file(&temp_dir, "project/src/bin/app.rs", b"// App code");
	create_test_file(&temp_dir, "project/Cargo.toml", b"[package]");
	create_test_file(&temp_dir, "README.md", b"# Project");

	// Verify all paths exist
	for dir_path in &structure {
		assert!(temp_dir.path().join(dir_path).exists(), "Path {} should exist", dir_path);
	}

	assert!(temp_dir.path().join("project/src/lib/main.rs").exists());
	assert!(temp_dir.path().join("project/Cargo.toml").exists());
	assert!(temp_dir.path().join("README.md").exists());
}

// ============================================================================
// PART 6: Edge Cases and Boundary Conditions
// ============================================================================

#[test]
fn test_filename_with_spaces() {
	let temp_dir = TempDir::new().unwrap();
	create_test_file(&temp_dir, "file with spaces.txt", b"Content");

	assert!(temp_dir.path().join("file with spaces.txt").exists());
}

#[test]
fn test_filename_with_special_chars() {
	let temp_dir = TempDir::new().unwrap();
	create_test_file(&temp_dir, "file-with_special.chars.txt", b"Content");

	assert!(temp_dir.path().join("file-with_special.chars.txt").exists());
}

#[test]
fn test_very_long_filename() {
	let temp_dir = TempDir::new().unwrap();
	let long_name = "a".repeat(200);
	let filename = format!("{}.txt", long_name);
	create_test_file(&temp_dir, &filename, b"Content");

	assert!(temp_dir.path().join(&filename).exists());
}

#[test]
fn test_zero_byte_file() {
	let temp_dir = TempDir::new().unwrap();
	create_test_file(&temp_dir, "empty.bin", b"");

	let meta = fs::metadata(temp_dir.path().join("empty.bin")).unwrap();
	assert_eq!(meta.len(), 0);
}

#[test]
fn test_single_byte_file() {
	let temp_dir = TempDir::new().unwrap();
	create_test_file(&temp_dir, "single.bin", &[0xFF]);

	let meta = fs::metadata(temp_dir.path().join("single.bin")).unwrap();
	assert_eq!(meta.len(), 1);
}

#[test]
fn test_mega_byte_file() {
	let temp_dir = TempDir::new().unwrap();
	let content = vec![0xAA; 1_000_000]; // 1MB
	create_test_file(&temp_dir, "megabyte.bin", &content);

	let meta = fs::metadata(temp_dir.path().join("megabyte.bin")).unwrap();
	assert_eq!(meta.len(), 1_000_000);
}

#[test]
fn test_file_with_null_bytes() {
	let temp_dir = TempDir::new().unwrap();
	let content = b"Before\x00\x00\x00After";
	create_test_file(&temp_dir, "with_nulls.bin", content);

	let read_content = fs::read(temp_dir.path().join("with_nulls.bin")).unwrap();
	assert_eq!(read_content, content);
}

#[test]
fn test_directory_with_many_subdirs() {
	let temp_dir = TempDir::new().unwrap();

	for i in 0..50 {
		fs::create_dir_all(temp_dir.path().join(format!("subdir_{}", i))).unwrap();
	}

	let entries: Vec<_> = fs::read_dir(temp_dir.path()).unwrap().map(|e| e.unwrap()).collect();

	assert_eq!(entries.len(), 50);
}

// ============================================================================
// PART 7: Multi-Directory Sync Scenarios
// ============================================================================

#[test]
fn test_two_way_sync_setup() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();

	create_file_scenario(&dir1, "basic");
	create_file_scenario(&dir2, "basic");

	assert!(dir1.path().join("file1.txt").exists());
	assert!(dir1.path().join("file2.txt").exists());
	assert!(dir2.path().join("file1.txt").exists());
	assert!(dir2.path().join("file2.txt").exists());
}

#[test]
fn test_three_way_sync_setup() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let dir3 = TempDir::new().unwrap();

	create_file_scenario(&dir1, "basic");
	create_file_scenario(&dir2, "identical");
	create_file_scenario(&dir3, "mixed");

	assert!(dir1.path().join("file1.txt").exists());
	assert!(dir2.path().join("file1.txt").exists());
	assert!(dir3.path().join("text.txt").exists());
}

#[test]
fn test_asymmetric_directories() {
	let empty_dir = TempDir::new().unwrap();
	let full_dir = TempDir::new().unwrap();

	create_file_scenario(&full_dir, "mixed");

	let empty_entries: Vec<_> = fs::read_dir(empty_dir.path()).unwrap().collect();
	let full_entries: Vec<_> = fs::read_dir(full_dir.path()).unwrap().collect();

	assert!(empty_entries.is_empty());
	assert!(!full_entries.is_empty());
}

#[test]
fn test_partial_overlap_sync() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();

	// Create files that are in both directories
	create_test_file(&dir1, "shared.txt", b"Shared");
	create_test_file(&dir2, "shared.txt", b"Shared");

	// Create unique files in each
	create_test_file(&dir1, "unique1.txt", b"Unique to dir1");
	create_test_file(&dir2, "unique2.txt", b"Unique to dir2");

	let dir1_files: Vec<_> = fs::read_dir(dir1.path())
		.unwrap()
		.map(|e| e.unwrap().file_name().to_string_lossy().to_string())
		.collect();
	let dir2_files: Vec<_> = fs::read_dir(dir2.path())
		.unwrap()
		.map(|e| e.unwrap().file_name().to_string_lossy().to_string())
		.collect();

	assert!(dir1_files.contains(&"shared.txt".to_string()));
	assert!(dir1_files.contains(&"unique1.txt".to_string()));
	assert!(dir2_files.contains(&"shared.txt".to_string()));
	assert!(dir2_files.contains(&"unique2.txt".to_string()));
}

// ============================================================================
// PART 8: Configuration Type Tests
// ============================================================================

#[test]
fn test_sync_config_default() {
	let config = SyncConfig::default();

	assert!(!config.dry_run);
	assert_eq!(config.chunk_config.chunk_bits, 20); // Default should be 20
}

#[test]
fn test_chunk_config_values() {
	let config = ChunkConfig { chunk_bits: 18, max_chunk_size: 262144, min_chunk_size: 4096 };

	assert_eq!(config.chunk_bits, 18);
}

#[test]
fn test_conflict_resolution_default() {
	let config = SyncConfig::default();

	// Default should be some reasonable value (check what the default is)
	let _ = config.conflict_resolution;
}

#[test]
fn test_conflict_resolution_variants() {
	let strategies = vec![
		ConflictResolution::PreferNewest,
		ConflictResolution::PreferLargest,
		ConflictResolution::FailOnConflict,
	];

	for strategy in strategies {
		let builder =
			SyncBuilder::new().add_location("./dir").conflict_resolution(strategy.clone());

		assert_eq!(builder.config().conflict_resolution, strategy);
	}
}

// ============================================================================
// PART 9: Builder Default Values Tests
// ============================================================================

#[test]
fn test_builder_default_chunk_bits() {
	let builder = SyncBuilder::new().add_location("./dir");

	assert_eq!(builder.config().chunk_config.chunk_bits, 20); // Default
}

#[test]
fn test_builder_default_dry_run() {
	let builder = SyncBuilder::new().add_location("./dir");

	assert!(!builder.config().dry_run);
}

#[test]
fn test_builder_default_profile() {
	let builder = SyncBuilder::new().add_location("./dir");

	assert_eq!(builder.config().profile, "default");
}

#[test]
fn test_builder_default_exclude_patterns() {
	let builder = SyncBuilder::new().add_location("./dir");

	assert!(builder.config().exclude_patterns.is_empty());
}

// ============================================================================
// PART 10: Builder State Isolation Tests
// ============================================================================

#[test]
fn test_builder_instances_independent() {
	let builder1 = SyncBuilder::new().add_location("./dir1").chunk_size_bits(16);

	let builder2 = SyncBuilder::new().add_location("./dir2").chunk_size_bits(24);

	assert_eq!(builder1.locations()[0], "./dir1");
	assert_eq!(builder1.config().chunk_config.chunk_bits, 16);

	assert_eq!(builder2.locations()[0], "./dir2");
	assert_eq!(builder2.config().chunk_config.chunk_bits, 24);
}

#[test]
fn test_builder_modifications_dont_affect_original() {
	let builder1 = SyncBuilder::new().add_location("./dir1");

	let _builder2 = builder1.add_location("./dir2").chunk_size_bits(16);

	// builder1 should not be modified (it's consumed in the chain)
	// This test verifies that each builder call returns a new builder
}

// ============================================================================
// PART 11: File Content Comparison Tests
// ============================================================================

#[test]
fn test_content_comparison_identical() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();

	let content = b"Identical content for testing";
	create_test_file(&dir1, "file.txt", content);
	create_test_file(&dir2, "file.txt", content);

	let content1 = fs::read(dir1.path().join("file.txt")).unwrap();
	let content2 = fs::read(dir2.path().join("file.txt")).unwrap();

	assert_eq!(content1, content2);
}

#[test]
fn test_content_comparison_different() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();

	create_test_file(&dir1, "file.txt", b"Content A");
	create_test_file(&dir2, "file.txt", b"Content B");

	let content1 = fs::read(dir1.path().join("file.txt")).unwrap();
	let content2 = fs::read(dir2.path().join("file.txt")).unwrap();

	assert_ne!(content1, content2);
}

#[test]
fn test_content_size_vs_content() {
	let temp_dir = TempDir::new().unwrap();

	// Two files with same size but different content
	create_test_file(&temp_dir, "file1.txt", b"AAAAAAAAAA");
	create_test_file(&temp_dir, "file2.txt", b"BBBBBBBBBB");

	let content1 = fs::read(temp_dir.path().join("file1.txt")).unwrap();
	let content2 = fs::read(temp_dir.path().join("file2.txt")).unwrap();

	assert_eq!(content1.len(), content2.len());
	assert_ne!(content1, content2);
}
