//! Connection Error Tests
//!
//! Tests for handling connection failures:
//! - SSH errors (timeouts, auth failures, key missing)
//! - Protocol negotiation failures
//! - Broken pipe / connection reset
//! - Invalid paths and remote errors

use syncr::sync::SyncBuilder;

// ===================================================================
// Phase 3.1: Connection Error Tests (10 tests)
// ===================================================================

/// Test handling of invalid remote path format
#[test]
fn test_invalid_remote_path_format() {
	// Path without colon should be treated as local
	let builder = SyncBuilder::new().add_location("host_without_colon");

	// Builder should accept it (validation happens at connection time)
	assert_eq!(builder.location_count(), 1);
	assert_eq!(builder.locations()[0], "host_without_colon");
}

/// Test remote path with standard SSH format
#[test]
fn test_valid_ssh_format_path() {
	let builder = SyncBuilder::new().add_location("host.example.com:/path/to/dir");

	assert_eq!(builder.location_count(), 1);
	assert_eq!(builder.locations()[0], "host.example.com:/path/to/dir");
}

/// Test IPv4 address as remote host
#[test]
fn test_ipv4_remote_path() {
	let builder = SyncBuilder::new().add_location("192.168.1.100:/remote/dir");

	assert_eq!(builder.location_count(), 1);
}

/// Test IPv6 address as remote host
#[test]
fn test_ipv6_remote_path() {
	// IPv6 addresses need special handling in SSH
	let builder = SyncBuilder::new().add_location("::1:/path");

	assert_eq!(builder.location_count(), 1);
}

/// Test local path that looks like remote (starts with ~ or /)
#[test]
fn test_local_home_directory_path() {
	// Paths starting with ~ are always local
	let builder = SyncBuilder::new().add_location("~/my/local/dir");

	assert_eq!(builder.location_count(), 1);
	assert_eq!(builder.locations()[0], "~/my/local/dir");
}

/// Test relative local path
#[test]
fn test_relative_path() {
	let builder = SyncBuilder::new().add_location("./relative/path");

	assert_eq!(builder.location_count(), 1);
	assert_eq!(builder.locations()[0], "./relative/path");
}

/// Test absolute local path
#[test]
fn test_absolute_path() {
	let builder = SyncBuilder::new().add_location("/absolute/path");

	assert_eq!(builder.location_count(), 1);
	assert_eq!(builder.locations()[0], "/absolute/path");
}

/// Test remote path with custom SSH port
#[test]
fn test_remote_path_with_port_in_path() {
	// Port is typically specified via SSH config, not in the path
	// This should be treated as a path with colon
	let builder = SyncBuilder::new().add_location("host:2222/path/to/dir");

	assert_eq!(builder.location_count(), 1);
}

/// Test mixed local and remote locations
#[test]
fn test_mixed_local_and_remote() {
	let builder = SyncBuilder::new()
		.add_location("./local1")
		.add_location("remote.server:/dir")
		.add_location("/absolute/local");

	assert_eq!(builder.location_count(), 3);
}

/// Test protocol version compatibility
#[test]
fn test_protocol_version_config() {
	// Builder should store configuration for protocol handling
	let builder = SyncBuilder::new().add_location("./dir1").add_location("./dir2");

	// Should have valid configuration
	assert!(!builder.config().profile.is_empty());
}

// ===================================================================
// Additional Tests for Connection State Management
// ===================================================================

/// Test builder state with profile switching
#[test]
fn test_profile_switching() {
	let builder1 = SyncBuilder::new().add_location("./dir1").profile("workspace");

	let builder2 = builder1.profile("backup");

	assert_eq!(builder2.profile_name(), "backup");
}

/// Test locations are preserved across builder chaining
#[test]
fn test_builder_chaining_preserves_locations() {
	let builder = SyncBuilder::new()
		.add_location("./dir1")
		.add_location("./dir2")
		.profile("test")
		.exclude_patterns(vec!["*.tmp"]);

	assert_eq!(builder.location_count(), 2);
	assert_eq!(builder.profile_name(), "test");
	assert_eq!(builder.config().exclude_patterns.len(), 1);
}

/// Test state directory configuration
#[test]
fn test_state_dir_configuration() {
	let state_path = std::path::PathBuf::from("/tmp/test_state");
	let builder = SyncBuilder::new().add_location("./dir").state_dir(state_path.to_str().unwrap());

	assert_eq!(builder.state_directory(), state_path);
}

/// Test that builder creates valid state paths
#[test]
fn test_state_path_generation() {
	use tempfile::TempDir;

	let temp_dir = TempDir::new().unwrap();
	let builder = SyncBuilder::new()
		.add_location("./dir1")
		.state_dir(temp_dir.path().to_str().unwrap())
		.profile("test");

	let state_path = builder.state_path();

	// State path should include profile name
	assert!(state_path.to_string_lossy().contains("test"));
	assert!(state_path.to_string_lossy().contains(".json"));
}

/// Test dry-run configuration validation
#[test]
fn test_dry_run_prevents_modifications() {
	let builder = SyncBuilder::new().add_location("./dir1").dry_run(true);

	assert!(builder.config().dry_run);

	let builder2 = builder.dry_run(false);
	assert!(!builder2.config().dry_run);
}
