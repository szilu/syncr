//! Integration tests for heterogeneous environment synchronization
//!
//! Tests multi-node syncing scenarios with different capabilities
//! and validates that metadata is handled correctly based on reconciliation rules.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

use syncr::metadata::{MetadataReconciler, MetadataStrategy, NodeCapabilities, ReconciliationMode};

// ============================================================================
// Helper Functions
// ============================================================================

#[allow(dead_code)]
fn create_test_file(dir: &TempDir, name: &str, content: &[u8]) -> PathBuf {
	let file_path = dir.path().join(name);
	if let Some(parent) = file_path.parent() {
		fs::create_dir_all(parent).unwrap();
	}
	let mut file = fs::File::create(&file_path).unwrap();
	file.write_all(content).unwrap();
	file_path
}

/// Simulate capability detection for nodes
fn detect_node_capabilities(node_index: usize, node_count: usize) -> NodeCapabilities {
	match (node_index, node_count) {
		// Two-node Linux/Windows scenario
		(0, 2) => {
			// Node 0: Linux with full capabilities
			let mut caps = NodeCapabilities::all();
			caps.filesystem_type = "ext4".to_string();
			caps.effective_uid = 1000;
			caps
		}
		(1, 2) => {
			// Node 1: Windows with limited capabilities
			let mut caps = NodeCapabilities::none();
			caps.filesystem_type = "NTFS".to_string();
			caps.case_sensitive = false;
			caps
		}
		// Three-node heterogeneous scenario
		(0, 3) => {
			// Node 0: Linux root
			let mut caps = NodeCapabilities::all();
			caps.filesystem_type = "ext4".to_string();
			caps.effective_uid = 0;
			caps
		}
		(1, 3) => {
			// Node 1: Linux user
			let mut caps = NodeCapabilities::none();
			caps.can_chmod = true;
			caps.filesystem_type = "ext4".to_string();
			caps.effective_uid = 1000;
			caps
		}
		(2, 3) => {
			// Node 2: macOS
			let mut caps = NodeCapabilities::all();
			caps.filesystem_type = "APFS".to_string();
			caps.case_sensitive = true;
			caps.effective_uid = 501;
			caps
		}
		_ => NodeCapabilities::all(),
	}
}

/// Determine if metadata should be compared based on reconciliation rules
fn should_compare_metadata(
	metadata_type: &str,
	strategy: MetadataStrategy,
	node_count: usize,
) -> bool {
	let mut capabilities = Vec::new();
	for i in 0..node_count {
		capabilities.push(detect_node_capabilities(i, node_count));
	}

	let reconciliation_mode = strategy.to_reconciliation_mode();
	let reconciler = MetadataReconciler::new(reconciliation_mode);
	let comparison = reconciler.compute_comparison(&capabilities);

	match metadata_type {
		"owner" => comparison.compare_owner,
		"permissions" => comparison.compare_permissions,
		"timestamps" => comparison.compare_timestamps,
		"xattrs" => comparison.compare_xattrs,
		_ => false,
	}
}

// ============================================================================
// Part 1: Two-Node Linux/Windows Scenarios
// ============================================================================

#[test]
fn test_linux_windows_strict_strategy() {
	// Strict strategy should only compare common metadata
	assert!(!should_compare_metadata("owner", MetadataStrategy::Strict, 2));
	assert!(!should_compare_metadata("permissions", MetadataStrategy::Strict, 2));
	assert!(should_compare_metadata("timestamps", MetadataStrategy::Strict, 2));
	assert!(!should_compare_metadata("xattrs", MetadataStrategy::Strict, 2));
}

#[test]
fn test_linux_windows_smart_strategy() {
	// Smart strategy maps to BestEffort mode
	// BestEffort compares what ANY node can preserve (using any() logic)
	assert!(should_compare_metadata("owner", MetadataStrategy::Smart, 2), "Linux can chown");
	assert!(should_compare_metadata("permissions", MetadataStrategy::Smart, 2), "Linux can chmod");
	assert!(
		should_compare_metadata("timestamps", MetadataStrategy::Smart, 2),
		"Both support timestamps"
	);
	// xattrs: Linux CAN set them, so with BestEffort (any), xattrs ARE compared
	assert!(should_compare_metadata("xattrs", MetadataStrategy::Smart, 2), "Linux can set xattrs");
}

#[test]
fn test_linux_windows_relaxed_strategy() {
	// Relaxed strategy maps to BestEffort mode, same as Smart
	// BestEffort based on capabilities using any() logic
	assert!(should_compare_metadata("owner", MetadataStrategy::Relaxed, 2), "Linux can chown");
	assert!(
		should_compare_metadata("permissions", MetadataStrategy::Relaxed, 2),
		"Linux can chmod"
	);
	assert!(
		should_compare_metadata("timestamps", MetadataStrategy::Relaxed, 2),
		"Both support timestamps"
	);
	// xattrs: Linux CAN set them, so with BestEffort, xattrs ARE compared
	assert!(
		should_compare_metadata("xattrs", MetadataStrategy::Relaxed, 2),
		"Linux can set xattrs"
	);
}

#[test]
fn test_linux_windows_content_only_strategy() {
	// ContentOnly maps to LCD mode (most conservative)
	// LCD only compares what ALL nodes can do (only timestamps for Linux/Windows)
	assert!(!should_compare_metadata("owner", MetadataStrategy::ContentOnly, 2));
	assert!(!should_compare_metadata("permissions", MetadataStrategy::ContentOnly, 2));
	assert!(
		should_compare_metadata("timestamps", MetadataStrategy::ContentOnly, 2),
		"Both support timestamps"
	);
	assert!(!should_compare_metadata("xattrs", MetadataStrategy::ContentOnly, 2));
}

// ============================================================================
// Part 2: Three-Node Heterogeneous Scenarios
// ============================================================================

#[test]
fn test_linux_root_user_macos_strict() {
	// Three-node: Linux root, Linux user, macOS
	// Strict (LCD) should only compare what ALL can do

	// Linux root: can chown/chmod
	// Linux user: can only chmod (not chown)
	// macOS root: can chown/chmod
	// LCD: Only chmod (not all can chown)

	assert!(!should_compare_metadata("owner", MetadataStrategy::Strict, 3));
	assert!(should_compare_metadata("permissions", MetadataStrategy::Strict, 3));
	assert!(should_compare_metadata("timestamps", MetadataStrategy::Strict, 3));
}

#[test]
fn test_linux_root_user_macos_smart() {
	// Smart (BestEffort) should try to preserve what any node can

	// Linux root: can chown/chmod/xattrs
	// Linux user: can only chmod
	// macOS: can chown/chmod

	assert!(should_compare_metadata("owner", MetadataStrategy::Smart, 3), "root can chown");
	assert!(should_compare_metadata("permissions", MetadataStrategy::Smart, 3), "all can chmod");
}

// ============================================================================
// Part 3: Cross-Filesystem Scenarios
// ============================================================================

#[test]
fn test_case_sensitivity_detection() {
	// Node 0: Linux (case-sensitive)
	let mut linux = NodeCapabilities::all();
	linux.case_sensitive = true;

	// Node 1: Windows (case-insensitive)
	let mut windows = NodeCapabilities::all();
	windows.case_sensitive = false;

	let capabilities = vec![linux, windows];
	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let comparison = reconciler.compute_comparison(&capabilities);

	// Time tolerance should be 2 seconds for FAT32/case-insensitive
	assert_eq!(comparison.time_tolerance_secs, 2);
}

#[test]
fn test_all_case_sensitive_filesystems() {
	// Both nodes are case-sensitive
	let mut fs1 = NodeCapabilities::all();
	fs1.case_sensitive = true;

	let mut fs2 = NodeCapabilities::all();
	fs2.case_sensitive = true;

	let capabilities = vec![fs1, fs2];
	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let comparison = reconciler.compute_comparison(&capabilities);

	// Time tolerance should be 1 second for modern filesystems
	assert_eq!(comparison.time_tolerance_secs, 1);
}

// ============================================================================
// Part 4: Privilege Level Scenarios
// ============================================================================

#[test]
fn test_running_as_root() {
	let mut root_caps = NodeCapabilities::all();
	root_caps.effective_uid = 0;

	let mut user_caps = NodeCapabilities::none();
	user_caps.effective_uid = 1000;

	let capabilities = vec![root_caps, user_caps];
	let reconciler = MetadataReconciler::new(ReconciliationMode::BestEffort);
	let comparison = reconciler.compute_comparison(&capabilities);

	// Root can chown, so should be compared in BestEffort
	assert!(comparison.compare_owner);
}

#[test]
fn test_running_as_non_root() {
	let mut user_caps1 = NodeCapabilities::none();
	user_caps1.effective_uid = 1000;

	let mut user_caps2 = NodeCapabilities::none();
	user_caps2.effective_uid = 2000;

	let capabilities = vec![user_caps1, user_caps2];
	let reconciler = MetadataReconciler::new(ReconciliationMode::BestEffort);
	let comparison = reconciler.compute_comparison(&capabilities);

	// Neither user can chown
	assert!(!comparison.compare_owner);
}

// ============================================================================
// Part 5: Real-World Scenario Tests
// ============================================================================

#[test]
fn test_scenario_cloud_backup() {
	// Scenario: Syncing local machine to cloud storage
	// Local: Linux with full capabilities
	// Remote: Cloud service (limited capability provider)

	let mut local = NodeCapabilities::all();
	local.filesystem_type = "ext4".to_string();

	let mut cloud = NodeCapabilities::none();
	cloud.filesystem_type = "object_storage".to_string();
	cloud.can_chmod = false; // Cloud doesn't support permission changes

	let capabilities = vec![local, cloud];

	// Strict: most conservative (only common)
	let strict_reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let strict = strict_reconciler.compute_comparison(&capabilities);
	assert!(!strict.compare_owner);
	assert!(!strict.compare_permissions);

	// Smart: try to preserve what we can
	let smart_reconciler = MetadataReconciler::new(ReconciliationMode::BestEffort);
	let smart = smart_reconciler.compute_comparison(&capabilities);
	// Local can preserve these, but cloud can't, so in smart mode it depends on 'any' logic
	assert!(smart.compare_timestamps); // Both support timestamps
}

#[test]
fn test_scenario_docker_container_sync() {
	// Scenario: Syncing data between Docker containers and host
	// Host: Full capabilities
	// Container: Limited capabilities (often non-root in container)

	let mut host = NodeCapabilities::all();
	host.effective_uid = 0;
	host.filesystem_type = "ext4".to_string();

	let mut container = NodeCapabilities::none();
	container.effective_uid = 1000; // Often runs as non-root
	container.filesystem_type = "overlay2".to_string();

	let capabilities = vec![host, container];

	// In Strict (LCD) mode, should not compare owner/chmod
	let strict_reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let strict = strict_reconciler.compute_comparison(&capabilities);
	assert!(!strict.compare_owner);
	assert!(!strict.compare_permissions);
}

#[test]
fn test_scenario_mobile_app_sync() {
	// Scenario: Syncing files from desktop to mobile device
	// Desktop: Unix-like with full capabilities
	// Mobile: Different filesystem, limited metadata support

	let mut desktop = NodeCapabilities::all();
	desktop.filesystem_type = "ext4".to_string();
	desktop.can_set_xattrs = true;

	let mut mobile = NodeCapabilities::none();
	mobile.filesystem_type = "FAT32".to_string(); // Many mobile devices use FAT32
	mobile.can_set_xattrs = false;
	mobile.case_sensitive = false;

	let capabilities = vec![desktop, mobile];

	// Time tolerance should be 2 for FAT32
	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let comparison = reconciler.compute_comparison(&capabilities);
	assert_eq!(comparison.time_tolerance_secs, 2);
	assert!(!comparison.compare_xattrs);
}

#[test]
fn test_scenario_network_share_sync() {
	// Scenario: Syncing to a network-mounted share
	// Local: Full filesystem capabilities
	// Remote: NFS mount with restrictions

	let mut local = NodeCapabilities::all();
	local.filesystem_type = "ext4".to_string();

	let mut nfs = NodeCapabilities::all();
	nfs.filesystem_type = "nfs".to_string();
	nfs.can_chown = false; // NFS often disables chown for security
	nfs.can_set_xattrs = false;

	let capabilities = vec![local, nfs];

	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let comparison = reconciler.compute_comparison(&capabilities);

	assert!(!comparison.compare_owner);
	assert!(comparison.compare_permissions);
	assert!(!comparison.compare_xattrs);
}

// ============================================================================
// Part 6: Edge Case Scenarios
// ============================================================================

#[test]
fn test_identical_nodes_should_compare_all() {
	let node1 = NodeCapabilities::all();
	let node2 = NodeCapabilities::all();

	let capabilities = vec![node1, node2];
	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let comparison = reconciler.compute_comparison(&capabilities);

	// Identical capable nodes should compare everything
	assert!(comparison.compare_owner);
	assert!(comparison.compare_permissions);
	assert!(comparison.compare_timestamps);
	assert!(comparison.compare_xattrs);
}

#[test]
fn test_many_nodes_mixed_capabilities() {
	let mut capabilities = Vec::new();

	// 5 Linux nodes with varying capabilities
	for i in 0..3 {
		let mut caps = NodeCapabilities::all();
		caps.effective_uid = 1000 + i;
		capabilities.push(caps);
	}

	// 2 Windows nodes with limited capabilities
	for _i in 0..2 {
		capabilities.push(NodeCapabilities::none());
	}

	// With Strict (LCD), should only compare what ALL can do
	let strict_reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let strict = strict_reconciler.compute_comparison(&capabilities);

	// Windows can't chown/chmod/xattrs
	assert!(!strict.compare_owner);
	assert!(!strict.compare_permissions);
	assert!(!strict.compare_xattrs);
	assert!(strict.compare_timestamps);

	// With Smart (BestEffort), should compare what ANY can do
	let smart_reconciler = MetadataReconciler::new(ReconciliationMode::BestEffort);
	let smart = smart_reconciler.compute_comparison(&capabilities);

	// Linux nodes can do everything
	assert!(smart.compare_owner);
	assert!(smart.compare_permissions);
	assert!(smart.compare_xattrs);
	assert!(smart.compare_timestamps);
}

// ============================================================================
// Part 7: Validation Tests
// ============================================================================

#[test]
fn test_time_tolerance_boundaries() {
	// FAT32-like: 2 seconds
	let mut caps = NodeCapabilities::all();
	caps.case_sensitive = false;

	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let comparison = reconciler.compute_comparison(&[caps]);

	assert!(comparison.time_tolerance_secs == 2);
}

#[test]
fn test_comparison_consistency() {
	let capabilities = vec![NodeCapabilities::all(), NodeCapabilities::none()];

	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let comparison1 = reconciler.compute_comparison(&capabilities);

	let comparison2 = reconciler.compute_comparison(&capabilities);

	// Same capabilities should produce same rules
	assert_eq!(comparison1.compare_owner, comparison2.compare_owner);
	assert_eq!(comparison1.compare_permissions, comparison2.compare_permissions);
	assert_eq!(comparison1.compare_xattrs, comparison2.compare_xattrs);
	assert_eq!(comparison1.time_tolerance_secs, comparison2.time_tolerance_secs);
}
