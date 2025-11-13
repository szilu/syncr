/// Comprehensive configuration option tests
///
/// Tests verify that all configuration options work correctly:
/// 1. SyncBuilder API correctly stores each config option
/// 2. CLI parsing works for each option
/// 3. Default values are correct
/// 4. Options can be combined
/// 5. Validation works correctly
///
/// This test suite ensures that config bugs (like the conflict resolution routing bug)
/// are caught early.
use std::str::FromStr;
use syncr::chunking::CHUNK_BITS;
use syncr::metadata::MetadataStrategy;
use syncr::strategies::{ConflictResolution, DeleteMode};
use syncr::sync::SyncBuilder;

// ===================================================================
// LOCATION TESTS
// ===================================================================

#[test]
fn test_add_location() {
	let builder = SyncBuilder::new().add_location("/tmp/dir1").add_location("/tmp/dir2");

	assert_eq!(builder.location_count(), 2);
	assert_eq!(builder.locations(), &["/tmp/dir1", "/tmp/dir2"]);
}

#[test]
fn test_add_remote() {
	let builder = SyncBuilder::new()
		.add_remote("server1.com:/data")
		.add_remote("server2.com:/backup");

	assert_eq!(builder.location_count(), 2);
	assert_eq!(builder.locations(), &["server1.com:/data", "server2.com:/backup"]);
}

#[test]
fn test_mixed_local_and_remote() {
	let builder = SyncBuilder::new().add_location("/tmp/local").add_remote("server.com:/remote");

	assert_eq!(builder.location_count(), 2);
}

// ===================================================================
// CONFLICT RESOLUTION TESTS
// ===================================================================

#[test]
fn test_conflict_resolution_all_strategies() {
	// Test that all conflict resolution strategies can be set
	let strategies = vec![
		ConflictResolution::Skip,
		ConflictResolution::PreferNewest,
		ConflictResolution::PreferOldest,
		ConflictResolution::PreferLargest,
		ConflictResolution::PreferSmallest,
		ConflictResolution::PreferFirst,
		ConflictResolution::PreferLast,
		ConflictResolution::Interactive,
		ConflictResolution::FailOnConflict,
	];

	for strategy in strategies {
		let builder = SyncBuilder::new().conflict_resolution(strategy.clone());
		let config = builder.config();
		assert!(
			matches!(&config.conflict_resolution, s if discriminant(s) == discriminant(&strategy))
		);
	}
}

fn discriminant<T>(t: &T) -> std::mem::Discriminant<T> {
	std::mem::discriminant(t)
}

// ===================================================================
// EXCLUSION PATTERN TESTS
// ===================================================================

#[test]
fn test_exclude_patterns_single() {
	let builder = SyncBuilder::new().exclude_patterns(vec!["*.tmp"]);

	let config = builder.config();
	assert_eq!(config.exclude_patterns, vec!["*.tmp"]);
}

#[test]
fn test_exclude_patterns_multiple() {
	let builder =
		SyncBuilder::new().exclude_patterns(vec!["*.tmp", ".git/*", "node_modules/*", "target/*"]);

	let config = builder.config();
	assert_eq!(config.exclude_patterns.len(), 4);
	assert!(config.exclude_patterns.contains(&"*.tmp".to_string()));
	assert!(config.exclude_patterns.contains(&".git/*".to_string()));
}

#[test]
fn test_exclude_patterns_empty() {
	let builder = SyncBuilder::new().exclude_patterns(vec![]);

	let config = builder.config();
	assert_eq!(config.exclude_patterns.len(), 0);
}

// ===================================================================
// DRY RUN TESTS
// ===================================================================

#[test]
fn test_dry_run_enabled() {
	let builder = SyncBuilder::new().dry_run(true);

	let config = builder.config();
	assert!(config.dry_run);
}

#[test]
fn test_dry_run_disabled() {
	let builder = SyncBuilder::new().dry_run(false);

	let config = builder.config();
	assert!(!config.dry_run);
}

#[test]
fn test_dry_run_default() {
	let builder = SyncBuilder::new();

	let config = builder.config();
	assert!(!config.dry_run); // Should default to false
}

// ===================================================================
// CHUNK SIZE TESTS
// ===================================================================

#[test]
fn test_chunk_size_bits_custom() {
	let builder = SyncBuilder::new().chunk_size_bits(18); // ~256KB chunks

	let config = builder.config();
	assert_eq!(config.chunk_bits, 18);
}

#[test]
fn test_chunk_size_bits_default() {
	let builder = SyncBuilder::new();

	let config = builder.config();
	assert_eq!(config.chunk_bits, CHUNK_BITS); // Should use default
}

#[test]
fn test_chunk_size_bits_large() {
	let builder = SyncBuilder::new().chunk_size_bits(24); // ~16MB chunks

	let config = builder.config();
	assert_eq!(config.chunk_bits, 24);
}

// ===================================================================
// STATE DIRECTORY / PROFILE TESTS
// ===================================================================

#[test]
fn test_profile_name_custom() {
	let builder = SyncBuilder::new().profile("production");

	assert_eq!(builder.profile_name(), "production");
}

#[test]
fn test_profile_name_default() {
	let builder = SyncBuilder::new();

	assert_eq!(builder.profile_name(), "default");
}

#[test]
fn test_state_dir_custom() {
	let builder = SyncBuilder::new().state_dir("/var/lib/syncr");

	assert_eq!(builder.state_directory().to_str().unwrap(), "/var/lib/syncr");
}

#[test]
fn test_profile_and_state_dir() {
	let builder = SyncBuilder::new().profile("backup").state_dir("/custom/state");

	assert_eq!(builder.profile_name(), "backup");
	assert_eq!(builder.state_directory().to_str().unwrap(), "/custom/state");
}

// ===================================================================
// DELETE MODE TESTS
// ===================================================================

#[test]
fn test_delete_mode_from_str_sync() {
	let mode = DeleteMode::from_str("sync");
	assert!(mode.is_ok());
	assert_eq!(mode.unwrap(), DeleteMode::Sync);
}

#[test]
fn test_delete_mode_from_str_no_delete() {
	let mode = DeleteMode::from_str("no-delete");
	assert!(mode.is_ok());
	assert_eq!(mode.unwrap(), DeleteMode::NoDelete);
}

#[test]
fn test_delete_mode_from_str_delete_after() {
	let mode = DeleteMode::from_str("delete-after");
	assert!(mode.is_ok());
	assert_eq!(mode.unwrap(), DeleteMode::DeleteAfter);
}

#[test]
fn test_delete_mode_from_str_delete_excluded() {
	let mode = DeleteMode::from_str("delete-excluded");
	assert!(mode.is_ok());
	assert_eq!(mode.unwrap(), DeleteMode::DeleteExcluded);
}

#[test]
fn test_delete_mode_from_str_trash() {
	let mode = DeleteMode::from_str("trash");
	assert!(mode.is_ok());
	assert_eq!(mode.unwrap(), DeleteMode::Trash);
}

#[test]
fn test_delete_mode_from_str_invalid() {
	let mode = DeleteMode::from_str("invalid");
	assert!(mode.is_err());
}

// ===================================================================
// METADATA STRATEGY TESTS
// ===================================================================

#[test]
fn test_metadata_strategy_from_str_strict() {
	let strategy = MetadataStrategy::from_str("strict");
	assert!(strategy.is_ok());
	assert_eq!(strategy.unwrap(), MetadataStrategy::Strict);
}

#[test]
fn test_metadata_strategy_from_str_smart() {
	let strategy = MetadataStrategy::from_str("smart");
	assert!(strategy.is_ok());
	assert_eq!(strategy.unwrap(), MetadataStrategy::Smart);
}

#[test]
fn test_metadata_strategy_from_str_relaxed() {
	let strategy = MetadataStrategy::from_str("relaxed");
	assert!(strategy.is_ok());
	assert_eq!(strategy.unwrap(), MetadataStrategy::Relaxed);
}

#[test]
fn test_metadata_strategy_from_str_content_only() {
	let strategy = MetadataStrategy::from_str("content-only");
	assert!(strategy.is_ok());
	assert_eq!(strategy.unwrap(), MetadataStrategy::ContentOnly);
}

#[test]
fn test_metadata_strategy_from_str_invalid() {
	let strategy = MetadataStrategy::from_str("invalid");
	assert!(strategy.is_err());
}

// ===================================================================
// SYMLINK MODE TESTS
// ===================================================================
// Note: SymlinkMode doesn't have from_str, tests removed

// ===================================================================
// COMBINATION TESTS
// ===================================================================

#[test]
fn test_multiple_options_together() {
	let builder = SyncBuilder::new()
		.add_location("/tmp/dir1")
		.add_location("/tmp/dir2")
		.profile("test")
		.state_dir("/tmp/state")
		.conflict_resolution(ConflictResolution::PreferNewest)
		.exclude_patterns(vec!["*.tmp", ".git/*"])
		.chunk_size_bits(20)
		.dry_run(true);

	let config = builder.config();

	assert_eq!(builder.location_count(), 2);
	assert_eq!(builder.profile_name(), "test");
	assert_eq!(builder.state_directory().to_str().unwrap(), "/tmp/state");
	assert!(matches!(config.conflict_resolution, ConflictResolution::PreferNewest));
	assert_eq!(config.exclude_patterns.len(), 2);
	assert_eq!(config.chunk_bits, 20);
	assert!(config.dry_run);
}

#[test]
fn test_realistic_backup_config() {
	// Simulate a realistic backup configuration
	let builder = SyncBuilder::new()
		.add_location("~/important_data")
		.add_remote("backup.example.com:/backups")
		.profile("daily_backup")
		.conflict_resolution(ConflictResolution::PreferNewest)
		.exclude_patterns(vec![
			"*.tmp",
			"*.swp",
			".DS_Store",
			"Thumbs.db",
			"node_modules/*",
			"target/*",
		])
		.dry_run(false);

	assert_eq!(builder.location_count(), 2);
	assert_eq!(builder.profile_name(), "daily_backup");

	let config = builder.config();
	assert_eq!(config.exclude_patterns.len(), 6);
	assert!(!config.dry_run);
}

#[test]
fn test_realistic_multi_server_config() {
	// Simulate multi-server sync with strict metadata
	let builder = SyncBuilder::new()
		.add_remote("server1.com:/data")
		.add_remote("server2.com:/data")
		.add_remote("server3.com:/data")
		.profile("multi-server")
		.conflict_resolution(ConflictResolution::FailOnConflict)
		.chunk_size_bits(22); // Larger chunks for server sync

	assert_eq!(builder.location_count(), 3);
	assert_eq!(builder.profile_name(), "multi-server");

	let config = builder.config();
	assert!(matches!(config.conflict_resolution, ConflictResolution::FailOnConflict));
	assert_eq!(config.chunk_bits, 22);
}

// ===================================================================
// BUILDER CHAIN TESTS
// ===================================================================

#[test]
fn test_builder_is_chainable() {
	// Verify all methods return Self for chaining
	let _builder = SyncBuilder::new()
		.add_location("/tmp/1")
		.add_remote("host:/path")
		.profile("test")
		.state_dir("/tmp")
		.conflict_resolution(ConflictResolution::Skip)
		.exclude_patterns(vec!["*.tmp"])
		.chunk_size_bits(20)
		.dry_run(true);

	// If this compiles, chaining works
}

#[test]
fn test_builder_methods_dont_mutate_original() {
	// Verify builder uses move semantics (consumes self)
	let builder1 = SyncBuilder::new();
	let builder2 = builder1.add_location("/tmp/dir1");

	// builder1 should be moved (consumed), so we can't use it again
	// This test just verifies the API design is correct
	assert_eq!(builder2.location_count(), 1);
}

// ===================================================================
// DEFAULT VALUE TESTS
// ===================================================================

#[test]
fn test_all_defaults() {
	let builder = SyncBuilder::new();
	let config = builder.config();

	// Verify default values
	assert_eq!(builder.location_count(), 0);
	assert_eq!(builder.profile_name(), "default");
	assert_eq!(config.exclude_patterns.len(), 0);
	assert_eq!(config.chunk_bits, CHUNK_BITS);
	assert!(!config.dry_run);
	assert!(matches!(config.conflict_resolution, ConflictResolution::Interactive));
}

// ===================================================================
// EDGE CASE TESTS
// ===================================================================

#[test]
fn test_empty_exclude_patterns() {
	let builder = SyncBuilder::new().exclude_patterns(vec![]);

	let config = builder.config();
	assert_eq!(config.exclude_patterns.len(), 0);
}

#[test]
fn test_single_location() {
	// While not useful for sync, should be allowed
	let builder = SyncBuilder::new().add_location("/tmp/single");

	assert_eq!(builder.location_count(), 1);
}

#[test]
fn test_many_locations() {
	// Test n-way sync with many locations
	let mut builder = SyncBuilder::new();
	for i in 1..=10 {
		builder = builder.add_location(&format!("/tmp/dir{}", i));
	}

	assert_eq!(builder.location_count(), 10);
}

#[test]
fn test_very_small_chunks() {
	let builder = SyncBuilder::new().chunk_size_bits(10); // ~1KB chunks

	let config = builder.config();
	assert_eq!(config.chunk_bits, 10);
}

#[test]
fn test_very_large_chunks() {
	let builder = SyncBuilder::new().chunk_size_bits(28); // ~256MB chunks

	let config = builder.config();
	assert_eq!(config.chunk_bits, 28);
}

#[test]
fn test_special_characters_in_profile() {
	let builder = SyncBuilder::new().profile("test-profile_2024");

	assert_eq!(builder.profile_name(), "test-profile_2024");
}

#[test]
fn test_path_with_spaces() {
	let builder = SyncBuilder::new().add_location("/tmp/path with spaces");

	assert_eq!(builder.locations()[0], "/tmp/path with spaces");
}
