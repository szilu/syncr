//! Tests for CAP command fallback behavior when capability detection fails
//!
//! Validates that the sync system gracefully handles cases where capability detection
//! is not available (e.g., old protocol versions, connection failures).

use syncr::metadata::{MetadataReconciler, NodeCapabilities, ReconciliationMode};

// ============================================================================
// Part 1: Fallback Behavior Tests
// ============================================================================

#[test]
fn test_nodecapabilities_none_returns_defaults() {
	let none_caps = NodeCapabilities::none();

	// Verify all capabilities are disabled
	assert!(!none_caps.can_chown);
	assert!(!none_caps.can_chmod);
	assert!(!none_caps.can_set_xattrs);
	assert!(!none_caps.can_create_devices);
	assert!(!none_caps.can_create_fifos);
	assert_eq!(none_caps.filesystem_type, "unknown");
	assert!(none_caps.case_sensitive);
}

#[test]
fn test_fallback_to_none_capabilities_does_not_crash() {
	// This tests the graceful degradation: if CAP command fails,
	// we use NodeCapabilities::none()
	let fallback_caps = NodeCapabilities::none();

	// Should not panic even with disabled capabilities
	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let comparison = reconciler.compute_comparison(&[fallback_caps]);

	// Most conservative: don't compare owner/permissions/xattrs
	assert!(!comparison.compare_owner);
	assert!(!comparison.compare_permissions);
	assert!(!comparison.compare_xattrs);
	assert!(comparison.compare_timestamps);
}

#[test]
fn test_fallback_in_heterogeneous_environment() {
	// Scenario: One node reports capabilities, another fails and uses fallback
	let linux_caps = NodeCapabilities::all();
	let fallback_caps = NodeCapabilities::none(); // Fallback for failed detection

	let capabilities = vec![linux_caps, fallback_caps];

	// With LCD (Strict): only what BOTH can do
	let strict_reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let strict = strict_reconciler.compute_comparison(&capabilities);

	// Fallback node can't do anything special
	assert!(!strict.compare_owner);
	assert!(!strict.compare_permissions);
	assert!(!strict.compare_xattrs);
	assert!(strict.compare_timestamps);

	// With BestEffort (Smart): what ANY can do
	let best_reconciler = MetadataReconciler::new(ReconciliationMode::BestEffort);
	let best = best_reconciler.compute_comparison(&capabilities);

	// Linux node can do these, so they're compared
	assert!(best.compare_owner);
	assert!(best.compare_permissions);
	assert!(best.compare_xattrs);
}

#[test]
fn test_all_nodes_fallback() {
	// Worst case: all capability detections fail
	let cap1 = NodeCapabilities::none();
	let cap2 = NodeCapabilities::none();
	let cap3 = NodeCapabilities::none();

	let capabilities = vec![cap1, cap2, cap3];

	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let comparison = reconciler.compute_comparison(&capabilities);

	// Most conservative: only timestamps
	assert!(!comparison.compare_owner);
	assert!(!comparison.compare_permissions);
	assert!(!comparison.compare_xattrs);
	assert!(comparison.compare_timestamps);
}

// ============================================================================
// Part 2: Graceful Degradation Tests
// ============================================================================

#[test]
fn test_sync_continues_without_capability_detection() {
	// Even if CAP command fails, sync should continue
	// by using NodeCapabilities::none() as fallback

	// Simulate a two-node sync where capability detection fails
	let fallback = NodeCapabilities::none();
	let fallback2 = NodeCapabilities::none();

	let reconciler = MetadataReconciler::new(ReconciliationMode::BestEffort);
	let comparison = reconciler.compute_comparison(&[fallback, fallback2]);

	// Should be safe to use: compares timestamps but not special metadata
	assert!(comparison.compare_timestamps);
	assert!(!comparison.compare_owner);
}

#[test]
fn test_empty_capabilities_vec_safe_default() {
	// If somehow we end up with empty capabilities list
	let empty_vec: Vec<NodeCapabilities> = Vec::new();

	// LCD mode with empty capabilities
	let lcd_reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let lcd_comparison = lcd_reconciler.compute_comparison(&empty_vec);

	// Should fall back to safe defaults
	assert!(!lcd_comparison.compare_owner);
	assert!(!lcd_comparison.compare_permissions);
	assert!(!lcd_comparison.compare_xattrs);

	// BestEffort with empty capabilities
	let best_reconciler = MetadataReconciler::new(ReconciliationMode::BestEffort);
	let best_comparison = best_reconciler.compute_comparison(&empty_vec);

	// Should fall back to smart() which is safe
	assert!(best_comparison.compare_permissions);
	assert!(best_comparison.compare_timestamps);
}

// ============================================================================
// Part 3: Backward Compatibility Tests
// ============================================================================

#[test]
fn test_old_protocol_no_cap_command() {
	// Simulates connecting to an old node that doesn't support CAP command
	// Such nodes would be treated as NodeCapabilities::none()

	let new_node = NodeCapabilities::all();
	let old_node = NodeCapabilities::none(); // Can't detect capabilities on old node

	let capabilities = vec![new_node, old_node];

	// Should still work, just with reduced metadata comparison
	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let comparison = reconciler.compute_comparison(&capabilities);

	assert!(!comparison.compare_owner);
	assert!(!comparison.compare_permissions);
}

#[test]
fn test_network_failure_fallback() {
	// Network failure during CAP command: fallback to none
	let node1 = NodeCapabilities::all();
	let node2_failed = NodeCapabilities::none(); // Fallback due to network error

	let capabilities = vec![node1, node2_failed];

	let reconciler = MetadataReconciler::new(ReconciliationMode::BestEffort);
	let comparison = reconciler.compute_comparison(&capabilities);

	// First node's capabilities are still used
	assert!(comparison.compare_owner);
	assert!(comparison.compare_permissions);
}

// ============================================================================
// Part 4: Error Recovery Tests
// ============================================================================

#[test]
fn test_partial_capability_detection() {
	// Some nodes report capabilities, some fail
	let capabilities = vec![
		NodeCapabilities::all(),  // Successfully detected
		NodeCapabilities::none(), // Detection failed, fallback
		NodeCapabilities::all(),  // Successfully detected
		NodeCapabilities::none(), // Detection failed, fallback
	];

	// With this mixed setup, should use LCD (most conservative)
	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let comparison = reconciler.compute_comparison(&capabilities);

	// Only timestamps are safe
	assert!(!comparison.compare_owner);
	assert!(!comparison.compare_permissions);
	assert!(!comparison.compare_xattrs);
	assert!(comparison.compare_timestamps);
}

#[test]
fn test_recoverable_sync_after_fallback() {
	// After falling back to NodeCapabilities::none(),
	// the sync should still complete successfully

	let fallback_caps = vec![NodeCapabilities::none(), NodeCapabilities::none()];

	// Should not panic
	let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
	let _comparison = reconciler.compute_comparison(&fallback_caps);

	// Sync would continue with safe defaults (content-only mostly)
}

// ============================================================================
// Part 5: Consistency Under Fallback
// ============================================================================

#[test]
fn test_fallback_gives_consistent_rules() {
	// Fallback behavior should be deterministic
	let fallback = NodeCapabilities::none();

	let reconciler1 = MetadataReconciler::new(ReconciliationMode::Lcd);
	let comparison1 = reconciler1.compute_comparison(std::slice::from_ref(&fallback));

	let reconciler2 = MetadataReconciler::new(ReconciliationMode::Lcd);
	let comparison2 = reconciler2.compute_comparison(&[fallback]);

	// Same rules should be computed
	assert_eq!(comparison1.compare_owner, comparison2.compare_owner);
	assert_eq!(comparison1.compare_permissions, comparison2.compare_permissions);
	assert_eq!(comparison1.compare_xattrs, comparison2.compare_xattrs);
	assert_eq!(comparison1.compare_timestamps, comparison2.compare_timestamps);
}

#[test]
fn test_fallback_preserves_sync_correctness() {
	// Key invariant: fallback should not cause sync to miss real changes
	// It's better to compare less than to compare incorrectly

	let fallback_caps = NodeCapabilities::none();

	let reconciler = MetadataReconciler::new(ReconciliationMode::BestEffort);
	let comparison = reconciler.compute_comparison(&[fallback_caps]);

	// At minimum, should always compare timestamps
	// (changes based on file modification time)
	assert!(comparison.compare_timestamps, "Must always compare timestamps for correctness");
}

// ============================================================================
// Part 6: Operator Awareness Tests
// ============================================================================

#[test]
fn test_none_capabilities_clear_indicator() {
	// NodeCapabilities::none() should clearly indicate limited capabilities
	let none = NodeCapabilities::none();

	assert!(!none.can_chown, "none() means can't chown");
	assert!(!none.can_chmod, "none() means can't chmod");
	assert!(!none.can_set_xattrs, "none() means can't set xattrs");

	// This makes it obvious that metadata handling will be limited
}

#[test]
fn test_all_capabilities_clear_indicator() {
	// NodeCapabilities::all() should clearly indicate full capabilities
	let all = NodeCapabilities::all();

	assert!(all.can_chown, "all() means can chown");
	assert!(all.can_chmod, "all() means can chmod");
	assert!(all.can_set_xattrs, "all() means can set xattrs");

	// This makes it obvious that metadata handling will be comprehensive
}
