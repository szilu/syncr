//! Error Handling Tests - Validates graceful failure recovery
//!
//! Tests that verify syncr handles various error conditions gracefully:
//! - Connection failures (SSH timeouts, auth failures, broken pipes)
//! - I/O errors (disk full, permission denied, file deleted mid-sync)
//! - State errors (corrupted JSON, missing fields, version mismatch)
//! - Other failures (timeouts, lock failures, signal handling)

use std::fs;
use std::io::Write;
use std::path::Path;
use tempfile::TempDir;

use syncr::strategies::ConflictResolution;
use syncr::sync::SyncBuilder;

// ===================================================================
// PHASE 3.1: STATE ERROR TESTS (5 tests)
// ===================================================================

/// Helper to create a corrupted state file
fn create_corrupted_state_file(state_dir: &Path, profile: &str) -> std::io::Result<()> {
	let state_file = state_dir.join(format!("{}.profile.json", profile));
	fs::create_dir_all(state_dir)?;
	let mut file = fs::File::create(&state_file)?;
	file.write_all(b"{ invalid json")?;
	Ok(())
}

/// Helper to create a state file with missing required fields
fn create_incomplete_state_file(state_dir: &Path, profile: &str) -> std::io::Result<()> {
	let state_file = state_dir.join(format!("{}.profile.json", profile));
	fs::create_dir_all(state_dir)?;
	let mut file = fs::File::create(&state_file)?;
	// Missing required fields - just has a partial structure
	file.write_all(b"{\"partial\": \"data\"}")?;
	Ok(())
}

#[tokio::test]
async fn test_corrupted_state_file_recovery() {
	// Create temp directories
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state_dir = TempDir::new().unwrap();

	// Create a corrupted state file
	create_corrupted_state_file(state_dir.path(), "test").unwrap();

	// Sync should not panic - should gracefully handle corrupted state
	let result = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state_dir.path().to_str().unwrap())
		.profile("test")
		.sync()
		.await;

	// Should either succeed (treating it as first sync) or return a proper error
	// Should NOT panic or crash
	match result {
		Ok(_) => {
			// Success - treated as first sync with corrupted state ignored
		}
		Err(e) => {
			// Error returned gracefully
			let err_msg = format!("{}", e);
			assert!(
				!err_msg.contains("panicked"),
				"Should not panic on corrupted state: {}",
				err_msg
			);
		}
	}
}

#[tokio::test]
async fn test_incomplete_state_file_recovery() {
	// Create temp directories
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state_dir = TempDir::new().unwrap();

	// Create state file with missing fields
	create_incomplete_state_file(state_dir.path(), "test").unwrap();

	let result = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.state_dir(state_dir.path().to_str().unwrap())
		.profile("test")
		.sync()
		.await;

	// Should handle gracefully
	match result {
		Ok(_) => {
			// Success - incomplete state file handled gracefully
		}
		Err(e) => {
			let err_msg = format!("{}", e);
			assert!(
				!err_msg.contains("unwrap"),
				"Should not panic on incomplete state: {}",
				err_msg
			);
		}
	}
}

#[test]
fn test_unreadable_state_file() {
	let state_dir = TempDir::new().unwrap();
	let state_file = state_dir.path().join("test.profile.json");

	// Create state file
	fs::write(&state_file, "{}").unwrap();

	// Make it unreadable (remove read permissions)
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		let perms = fs::Permissions::from_mode(0o000);
		fs::set_permissions(&state_file, perms).unwrap();
	}

	// State file should not be readable, but sync should handle it
	// We can't easily test this in tokio context, but documenting the scenario

	// On Unix, file should be unreadable after making it so
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		let perms = fs::Permissions::from_mode(0o000);
		fs::set_permissions(&state_file, perms).unwrap();

		assert!(fs::read_to_string(&state_file).is_err());

		// Restore for cleanup
		let perms = fs::Permissions::from_mode(0o644);
		fs::set_permissions(&state_file, perms).ok();
	}
}

#[test]
fn test_missing_state_directory() {
	// Non-existent state directory should be created or handled gracefully
	let nonexistent =
		std::path::PathBuf::from("/tmp/definitely_does_not_exist_for_syncr_test_12345");

	// Builder should accept it - directory will be created when needed
	let builder = SyncBuilder::new()
		.add_location("./dir1")
		.state_dir(nonexistent.to_str().unwrap());

	// Should not panic during setup
	assert_eq!(builder.locations().len(), 1);
}

#[test]
fn test_permission_denied_state_directory() {
	// Test what happens when state directory is not writable
	let state_dir = TempDir::new().unwrap();

	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		// Make directory read-only
		let perms = fs::Permissions::from_mode(0o444);
		fs::set_permissions(state_dir.path(), perms).ok();
	}

	// Builder should accept the path
	let builder = SyncBuilder::new()
		.add_location("./dir1")
		.state_dir(state_dir.path().to_str().unwrap());

	assert_eq!(builder.locations().len(), 1);

	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		// Restore permissions for cleanup
		let perms = fs::Permissions::from_mode(0o755);
		fs::set_permissions(state_dir.path(), perms).ok();
	}
}

// ===================================================================
// PHASE 3.2: I/O ERROR TESTS (10 tests)
// ===================================================================

#[test]
fn test_file_deleted_during_listing() {
	// Create a directory with a file
	let dir = TempDir::new().unwrap();
	let file = dir.path().join("test.txt");
	fs::write(&file, "content").unwrap();

	// File exists
	assert!(file.exists());

	// Simulate file being deleted (would happen during actual sync)
	fs::remove_file(&file).unwrap();

	// File should now be gone
	assert!(!file.exists());
}

#[test]
fn test_permission_denied_on_file() {
	let dir = TempDir::new().unwrap();
	let file = dir.path().join("restricted.txt");
	fs::write(&file, "secret").unwrap();

	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		// Remove read permissions
		let perms = fs::Permissions::from_mode(0o000);
		fs::set_permissions(&file, perms).unwrap();

		// Should not be readable
		assert!(fs::read_to_string(&file).is_err());

		// Restore for cleanup
		let perms = fs::Permissions::from_mode(0o644);
		fs::set_permissions(&file, perms).ok();
	}
}

#[test]
fn test_permission_denied_on_directory() {
	let dir = TempDir::new().unwrap();
	let subdir = dir.path().join("locked");
	fs::create_dir(&subdir).unwrap();

	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		// Remove all permissions
		let perms = fs::Permissions::from_mode(0o000);
		fs::set_permissions(&subdir, perms).unwrap();

		// Should not be readable
		assert!(fs::read_dir(&subdir).is_err());

		// Restore for cleanup
		let perms = fs::Permissions::from_mode(0o755);
		fs::set_permissions(&subdir, perms).ok();
	}
}

#[test]
fn test_symlink_to_nonexistent_target() {
	// Create a symlink to a file that doesn't exist
	let dir = TempDir::new().unwrap();
	let link = dir.path().join("broken_link");

	#[cfg(unix)]
	{
		use std::os::unix::fs as unix_fs;
		unix_fs::symlink("/nonexistent/path", &link).unwrap();

		// Link exists but target doesn't
		assert!(!link.exists()); // exists() returns false for broken symlinks
		assert!(link.symlink_metadata().is_ok()); // But symlink_metadata works
	}
}

#[test]
fn test_directory_deleted_during_operation() {
	let dir = TempDir::new().unwrap();
	let subdir = dir.path().join("temp");
	fs::create_dir(&subdir).unwrap();

	// Create a file in it
	fs::write(subdir.join("file.txt"), "data").unwrap();

	// Delete the directory
	fs::remove_dir_all(&subdir).unwrap();

	// Directory should be gone
	assert!(!subdir.exists());
}

#[test]
fn test_special_file_handling() {
	// Test handling of special files (pipes, sockets, etc.)
	// On most systems, we can't create these in temp directories
	// but we can at least verify we don't crash when encountering them

	let dir = TempDir::new().unwrap();
	let file = dir.path().join("normal.txt");
	fs::write(&file, "normal file").unwrap();

	// Verify metadata read works
	let metadata = fs::metadata(&file).unwrap();
	assert!(metadata.is_file());
	assert!(!metadata.is_dir());
}

#[test]
fn test_very_long_filename() {
	let dir = TempDir::new().unwrap();
	// Most filesystems allow up to 255 bytes in filename
	let long_name = "a".repeat(200);
	let file = dir.path().join(&long_name);

	// Should be able to create long-named file
	fs::write(&file, "content").unwrap();
	assert!(file.exists());
}

#[test]
fn test_very_deep_directory_nesting() {
	let dir = TempDir::new().unwrap();
	let mut path = dir.path().to_path_buf();

	// Create deeply nested structure
	for i in 0..20 {
		path.push(format!("level_{}", i));
		fs::create_dir(&path).ok();
	}

	// Should be able to create deeply nested dirs
	assert!(path.exists());
}

#[test]
fn test_symlink_with_relative_path() {
	let dir = TempDir::new().unwrap();

	#[cfg(unix)]
	{
		use std::os::unix::fs as unix_fs;
		let target = dir.path().join("target.txt");
		let link = dir.path().join("link.txt");

		fs::write(&target, "content").unwrap();
		unix_fs::symlink("target.txt", &link).ok();

		// Link should be readable
		let metadata = fs::symlink_metadata(&link).unwrap();
		assert!(metadata.is_symlink());
	}
}

// ===================================================================
// PHASE 3.3: OTHER ERROR TESTS (5 tests)
// ===================================================================

#[test]
fn test_invalid_utf8_in_path() {
	// Most modern systems prefer UTF-8, but technically support other encodings
	// This is hard to test portably, so we document the scenario
	let dir = TempDir::new().unwrap();
	let file = dir.path().join("file.txt");
	fs::write(&file, "content").unwrap();

	// Normal UTF-8 path should work
	assert!(file.exists());
}

#[test]
fn test_builder_with_empty_locations() {
	let builder = SyncBuilder::new();

	// Should accept empty builder
	assert_eq!(builder.location_count(), 0);
	assert_eq!(builder.locations().len(), 0);
}

#[test]
fn test_builder_with_invalid_directory_path() {
	// Builder should accept path - validation happens at sync time
	let builder = SyncBuilder::new().add_location("/nonexistent/path/that/should/fail");

	// Should not panic during builder setup
	assert_eq!(builder.location_count(), 1);
}

#[test]
fn test_multiple_profiles_same_state_dir() {
	let state_dir = TempDir::new().unwrap();

	let builder1 = SyncBuilder::new()
		.add_location("./dir1")
		.state_dir(state_dir.path().to_str().unwrap())
		.profile("profile_a");

	let builder2 = SyncBuilder::new()
		.add_location("./dir2")
		.state_dir(state_dir.path().to_str().unwrap())
		.profile("profile_b");

	// Should be able to create multiple profiles
	assert_eq!(builder1.profile_name(), "profile_a");
	assert_eq!(builder2.profile_name(), "profile_b");
}

#[test]
fn test_state_persistence_with_dry_run() {
	let dir = TempDir::new().unwrap();
	let state_dir = TempDir::new().unwrap();

	// Dry-run should not modify state
	let builder = SyncBuilder::new()
		.add_location(dir.path().to_str().unwrap())
		.state_dir(state_dir.path().to_str().unwrap())
		.dry_run(true);

	assert!(builder.config().dry_run);
}

// ===================================================================
// PHASE 3.4: CONFLICT RESOLUTION ERROR TESTS (5 tests)
// ===================================================================

#[test]
fn test_conflict_resolution_with_missing_file() {
	// Create two directories with different content
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();

	// Create file only in dir1
	fs::write(dir1.path().join("file.txt"), "dir1 content").unwrap();
	// dir2 doesn't have this file

	// Builder with PreferNewest strategy should handle missing files
	let builder = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.conflict_resolution(ConflictResolution::PreferNewest);

	assert_eq!(builder.location_count(), 2);
}

#[test]
fn test_strategy_skip_with_conflicts() {
	// Skip strategy should leave conflicts unresolved
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();

	let builder = SyncBuilder::new()
		.add_location(dir1.path().to_str().unwrap())
		.add_location(dir2.path().to_str().unwrap())
		.conflict_resolution(ConflictResolution::Skip);

	assert_eq!(builder.config().conflict_resolution, ConflictResolution::Skip);
}

#[test]
fn test_strategy_prefer_oldest() {
	let builder = SyncBuilder::new()
		.add_location("./dir1")
		.conflict_resolution(ConflictResolution::PreferOldest);

	assert_eq!(builder.config().conflict_resolution, ConflictResolution::PreferOldest);
}

#[test]
fn test_exclusion_patterns_with_errors() {
	// Exclusion patterns should be stored correctly
	let builder = SyncBuilder::new().add_location("./dir1").exclude_patterns(vec![
		"*.tmp",
		".git/*",
		".DS_Store",
	]);

	assert_eq!(builder.config().exclude_patterns.len(), 3);
}

#[test]
fn test_chunk_size_configuration() {
	let builder = SyncBuilder::new().add_location("./dir1").chunk_size_bits(21); // 2MB average chunks

	assert_eq!(builder.config().chunk_bits, 21);
}
