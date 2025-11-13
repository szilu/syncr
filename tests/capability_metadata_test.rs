//! Integration tests for capability-aware metadata reconciliation
//!
//! Tests the Phase F+ Priority 2 feature: Per-Node Capability Integration
//! Validates that metadata comparison rules are adjusted based on detected node capabilities.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;
use tempfile::TempDir;

use syncr::metadata::{MetadataReconciler, MetadataStrategy, NodeCapabilities, ReconciliationMode};

// ============================================================================
// Helper Functions
// ============================================================================

/// Create a test file with specified content
fn create_test_file(dir: &TempDir, name: &str, content: &[u8]) -> PathBuf {
	let file_path = dir.path().join(name);
	if let Some(parent) = file_path.parent() {
		fs::create_dir_all(parent).unwrap();
	}
	let mut file = fs::File::create(&file_path).unwrap();
	file.write_all(content).unwrap();
	file_path
}

/// Set file permissions (Unix only)
#[cfg(unix)]
fn set_file_permissions(path: &PathBuf, mode: u32) {
	use std::fs::Permissions;
	use std::os::unix::fs::PermissionsExt;
	let perms = Permissions::from_mode(mode);
	fs::set_permissions(path, perms).unwrap();
}

/// Get file permissions (Unix only)
#[cfg(unix)]
fn get_file_mode(path: &PathBuf) -> u32 {
	use std::os::unix::fs::MetadataExt;
	fs::metadata(path).unwrap().mode()
}

// ============================================================================
// Part 1: Reconciliation Mode Selection Tests
// ============================================================================

#[test]
fn test_metadata_strategy_to_reconciliation_mode_strict() {
	let strategy = MetadataStrategy::Strict;
	let mode = strategy.to_reconciliation_mode();
	assert_eq!(mode, ReconciliationMode::Lcd);
}

#[test]
fn test_metadata_strategy_to_reconciliation_mode_smart() {
	let strategy = MetadataStrategy::Smart;
	let mode = strategy.to_reconciliation_mode();
	assert_eq!(mode, ReconciliationMode::BestEffort);
}

#[test]
fn test_metadata_strategy_to_reconciliation_mode_relaxed() {
	let strategy = MetadataStrategy::Relaxed;
	let mode = strategy.to_reconciliation_mode();
	assert_eq!(mode, ReconciliationMode::BestEffort);
}

#[test]
fn test_metadata_strategy_to_reconciliation_mode_content_only() {
	let strategy = MetadataStrategy::ContentOnly;
	let mode = strategy.to_reconciliation_mode();
	assert_eq!(mode, ReconciliationMode::Lcd);
}

// ============================================================================
// Part 2: LCD (Least Common Denominator) Mode Tests
// ============================================================================

#[test]
fn test_lcd_all_nodes_capable() {
	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let capabilities =
		vec![NodeCapabilities::all(), NodeCapabilities::all(), NodeCapabilities::all()];

	let comparison = reconciler.compute_comparison(&capabilities);

	// All nodes can preserve all metadata
	assert!(comparison.compare_owner);
	assert!(comparison.compare_permissions);
	assert!(comparison.compare_timestamps);
	assert!(comparison.compare_xattrs);
}

#[test]
fn test_lcd_windows_linux_mixed() {
	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);

	// Linux: full capabilities
	let mut linux_caps = NodeCapabilities::all();
	linux_caps.filesystem_type = "ext4".to_string();

	// Windows: limited capabilities (can't chown/chmod)
	let mut windows_caps = NodeCapabilities::none();
	windows_caps.filesystem_type = "NTFS".to_string();

	let capabilities = vec![linux_caps, windows_caps];
	let comparison = reconciler.compute_comparison(&capabilities);

	// Only timestamp comparison should be enabled (Windows can set timestamps)
	assert!(!comparison.compare_owner, "Windows can't chown");
	assert!(!comparison.compare_permissions, "Windows can't chmod");
	assert!(comparison.compare_timestamps, "Both support timestamps");
	assert!(!comparison.compare_xattrs, "Windows doesn't support xattrs");
}

#[test]
fn test_lcd_root_and_nonroot() {
	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);

	// Root node: full capabilities
	let root_caps = NodeCapabilities::all();

	// Non-root node: limited capabilities
	let user_caps = NodeCapabilities::none();

	let capabilities = vec![root_caps, user_caps];
	let comparison = reconciler.compute_comparison(&capabilities);

	// Non-root can't do ownership, permissions, or xattrs
	assert!(!comparison.compare_owner);
	assert!(!comparison.compare_permissions);
	assert!(!comparison.compare_xattrs);
	assert!(comparison.compare_timestamps);
}

#[test]
fn test_lcd_fat32_time_tolerance() {
	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);

	// Case-sensitive filesystem (ext4, NTFS with case sensitivity)
	let mut case_sensitive = NodeCapabilities::all();
	case_sensitive.case_sensitive = true;
	case_sensitive.filesystem_type = "ext4".to_string();

	// Case-insensitive filesystem (FAT32, Windows NTFS default)
	let mut case_insensitive = NodeCapabilities::all();
	case_insensitive.case_sensitive = false;
	case_insensitive.filesystem_type = "FAT32".to_string();

	let capabilities = vec![case_sensitive, case_insensitive];
	let comparison = reconciler.compute_comparison(&capabilities);

	// Should use 2-second tolerance for FAT32
	assert_eq!(comparison.time_tolerance_secs, 2);
}

// ============================================================================
// Part 3: BestEffort Mode Tests
// ============================================================================

#[test]
fn test_best_effort_windows_linux_mixed() {
	let reconciler = MetadataReconciler::new(ReconciliationMode::BestEffort);

	let linux_caps = NodeCapabilities::all();
	let windows_caps = NodeCapabilities::none();

	let capabilities = vec![linux_caps, windows_caps];
	let comparison = reconciler.compute_comparison(&capabilities);

	// BestEffort compares what ANY node can do
	assert!(comparison.compare_owner, "Linux can chown");
	assert!(comparison.compare_permissions, "Linux can chmod");
	assert!(comparison.compare_xattrs, "Linux supports xattrs");
}

#[test]
fn test_best_effort_tries_to_preserve_all() {
	let reconciler = MetadataReconciler::new(ReconciliationMode::BestEffort);

	// Three nodes with mixed capabilities
	let node1 = NodeCapabilities::all();
	let mut node2 = NodeCapabilities::none();
	node2.can_chmod = true; // Only supports chmod

	let mut node3 = NodeCapabilities::none();
	node3.can_chown = true; // Only supports chown

	let capabilities = vec![node1, node2, node3];
	let comparison = reconciler.compute_comparison(&capabilities);

	// Each capability is preserved by at least one node
	assert!(comparison.compare_owner, "Node3 can chown");
	assert!(comparison.compare_permissions, "Node2 can chmod");
}

// ============================================================================
// Part 4: Source Wins Mode Tests
// ============================================================================

#[test]
fn test_source_wins_root_source() {
	let root_caps = NodeCapabilities::all();
	let user_caps = NodeCapabilities::none();

	let capabilities = vec![root_caps, user_caps];

	// Node 0 (root) is source
	let reconciler = MetadataReconciler::source_wins(0);
	let comparison = reconciler.compute_comparison(&capabilities);

	// Should use root's capabilities
	assert!(comparison.compare_owner);
	assert!(comparison.compare_permissions);
	assert!(comparison.compare_xattrs);
}

#[test]
fn test_source_wins_user_source() {
	let root_caps = NodeCapabilities::all();
	let user_caps = NodeCapabilities::none();

	let capabilities = vec![root_caps, user_caps];

	// Node 1 (user) is source
	let reconciler = MetadataReconciler::source_wins(1);
	let comparison = reconciler.compute_comparison(&capabilities);

	// Should use user's (limited) capabilities
	assert!(!comparison.compare_owner);
	assert!(!comparison.compare_permissions);
	assert!(!comparison.compare_xattrs);
}

#[test]
fn test_source_wins_invalid_source_falls_back_to_lcd() {
	let capabilities = vec![NodeCapabilities::all(), NodeCapabilities::none()];

	// Non-existent source node index
	let reconciler = MetadataReconciler::source_wins(99);
	let comparison = reconciler.compute_comparison(&capabilities);

	// Should fall back to LCD mode (most conservative)
	assert!(!comparison.compare_owner);
	assert!(!comparison.compare_permissions);
}

// ============================================================================
// Part 5: Integration Scenario Tests
// ============================================================================

#[test]
fn test_scenario_heterogeneous_three_node_sync() {
	// Scenario: Syncing Linux (root), Linux (user), Windows
	let linux_root = NodeCapabilities::all();

	let mut linux_user = NodeCapabilities::none();
	linux_user.can_chmod = true; // Can only chmod, not chown
	linux_user.filesystem_type = "ext4".to_string();

	let mut windows = NodeCapabilities::none();
	windows.filesystem_type = "NTFS".to_string();
	windows.case_sensitive = false;

	let capabilities = vec![linux_root, linux_user, windows];

	// With Strict strategy (LCD): only what ALL can do
	let strict_reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let strict_comparison = strict_reconciler.compute_comparison(&capabilities);
	assert!(!strict_comparison.compare_owner, "Not all can chown");
	assert!(!strict_comparison.compare_permissions, "Not all can chmod");
	assert!(strict_comparison.compare_timestamps);

	// With Smart strategy (BestEffort): what ANY can do
	let smart_reconciler = MetadataReconciler::new(ReconciliationMode::BestEffort);
	let smart_comparison = smart_reconciler.compute_comparison(&capabilities);
	assert!(smart_comparison.compare_owner, "Linux root can chown");
	assert!(smart_comparison.compare_permissions, "Linux user can chmod");
}

#[test]
fn test_scenario_arm_device_sync() {
	// Scenario: Syncing from x86 Linux to ARM Linux (e.g., Raspberry Pi)
	let mut x86_linux = NodeCapabilities::all();
	x86_linux.filesystem_type = "ext4".to_string();

	let mut arm_linux = NodeCapabilities::all();
	arm_linux.filesystem_type = "ext4".to_string();

	let capabilities = vec![x86_linux, arm_linux];

	let reconciler = MetadataReconciler::new(ReconciliationMode::BestEffort);
	let comparison = reconciler.compute_comparison(&capabilities);

	// Both are identical and capable: full sync
	assert!(comparison.compare_owner);
	assert!(comparison.compare_permissions);
	assert!(comparison.compare_timestamps);
	assert!(comparison.compare_xattrs);
}

#[test]
fn test_scenario_nfs_share_sync() {
	// Scenario: Local filesystem + NFS mount (may not preserve all metadata)
	let mut local_fs = NodeCapabilities::all();
	local_fs.filesystem_type = "ext4".to_string();

	// NFS might have limitations depending on mount options
	let mut nfs_share = NodeCapabilities::all();
	nfs_share.filesystem_type = "nfs".to_string();
	nfs_share.can_set_xattrs = false; // NFS often doesn't support xattrs

	let capabilities = vec![local_fs, nfs_share];

	// With LCD: only common capabilities
	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let comparison = reconciler.compute_comparison(&capabilities);
	assert!(comparison.compare_owner);
	assert!(comparison.compare_permissions);
	assert!(!comparison.compare_xattrs, "NFS doesn't support xattrs");
}

// ============================================================================
// Part 6: Edge Cases and Error Handling
// ============================================================================

#[test]
fn test_empty_capabilities_list_lcd() {
	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let comparison = reconciler.compute_comparison(&[]);

	// Should fall back to content-only (most conservative)
	assert!(!comparison.compare_owner);
	assert!(!comparison.compare_permissions);
	assert!(!comparison.compare_xattrs);
}

#[test]
fn test_empty_capabilities_list_best_effort() {
	let reconciler = MetadataReconciler::new(ReconciliationMode::BestEffort);
	let comparison = reconciler.compute_comparison(&[]);

	// Should fall back to smart() which depends on whether running as root
	// At minimum, permissions and timestamps should be compared
	assert!(comparison.compare_permissions);
	assert!(comparison.compare_timestamps);
	// compare_owner and compare_xattrs depend on whether running as root
}

#[test]
fn test_single_node_lcd() {
	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let capabilities = vec![NodeCapabilities::all()];

	let comparison = reconciler.compute_comparison(&capabilities);

	// Single node: use its capabilities
	assert!(comparison.compare_owner);
	assert!(comparison.compare_permissions);
	assert!(comparison.compare_xattrs);
}

#[test]
fn test_single_limited_node_lcd() {
	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let capabilities = vec![NodeCapabilities::none()];

	let comparison = reconciler.compute_comparison(&capabilities);

	// Single limited node: use its (limited) capabilities
	assert!(!comparison.compare_owner);
	assert!(!comparison.compare_permissions);
	assert!(!comparison.compare_xattrs);
}

#[test]
fn test_all_none_capabilities() {
	let reconciler = MetadataReconciler::new(ReconciliationMode::BestEffort);
	let capabilities =
		vec![NodeCapabilities::none(), NodeCapabilities::none(), NodeCapabilities::none()];

	let comparison = reconciler.compute_comparison(&capabilities);

	// All nodes have no capabilities
	assert!(!comparison.compare_owner);
	assert!(!comparison.compare_permissions);
	assert!(!comparison.compare_xattrs);
	assert!(comparison.compare_timestamps); // Timestamps usually always work
}

// ============================================================================
// Part 7: File Metadata Handling Tests (if Unix)
// ============================================================================

#[test]
#[cfg(unix)]
fn test_file_permissions_preservation() {
	let temp_dir = TempDir::new().unwrap();
	let file_path = create_test_file(&temp_dir, "test.txt", b"content");

	// Set specific permissions
	set_file_permissions(&file_path, 0o644);

	let mode = get_file_mode(&file_path);
	assert_eq!(mode & 0o777, 0o644);
}

#[test]
#[cfg(unix)]
fn test_different_permissions() {
	let temp_dir = TempDir::new().unwrap();

	let file1 = create_test_file(&temp_dir, "file1.txt", b"content");
	let file2 = create_test_file(&temp_dir, "file2.txt", b"content");

	set_file_permissions(&file1, 0o644);
	set_file_permissions(&file2, 0o755);

	let mode1 = get_file_mode(&file1);
	let mode2 = get_file_mode(&file2);

	assert_ne!(mode1 & 0o777, mode2 & 0o777);
}

// ============================================================================
// Part 8: Strategy Validation Tests
// ============================================================================

#[test]
fn test_metadata_strategy_parsing() {
	assert_eq!(MetadataStrategy::from_str("strict").ok(), Some(MetadataStrategy::Strict));
	assert_eq!(MetadataStrategy::from_str("smart").ok(), Some(MetadataStrategy::Smart));
	assert_eq!(MetadataStrategy::from_str("relaxed").ok(), Some(MetadataStrategy::Relaxed));
	assert_eq!(
		MetadataStrategy::from_str("content-only").ok(),
		Some(MetadataStrategy::ContentOnly)
	);
	assert!(MetadataStrategy::from_str("invalid").is_err());
}

#[test]
fn test_metadata_strategy_default() {
	let default = MetadataStrategy::default();
	assert_eq!(default, MetadataStrategy::Smart);
}

#[test]
fn test_reconciliation_mode_default() {
	let default = ReconciliationMode::default();
	assert_eq!(default, ReconciliationMode::Lcd);
}
