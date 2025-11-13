/// Configuration and routing tests for conflict resolution strategies
///
/// Tests verify that:
/// 1. The SyncBuilder API correctly configures conflict resolution strategies
/// 2. The routing logic properly identifies automatic vs interactive strategies
/// 3. ConflictResolver correctly classifies strategies
/// 4. Configuration flows correctly through the system
///
/// These tests catch bugs like the one where --conflict skip was ignored
/// because routing only checked --skip-conflicts flag, not the strategy itself.
use std::str::FromStr;
use syncr::conflict::ConflictResolver;
use syncr::strategies::ConflictResolution;
use syncr::sync::SyncBuilder;

#[test]
fn test_is_automatic_for_skip() {
	// Skip should be automatic (no user interaction)
	assert!(ConflictResolver::is_automatic(&ConflictResolution::Skip));
}

#[test]
fn test_is_automatic_for_newest() {
	// PreferNewest should be automatic
	assert!(ConflictResolver::is_automatic(&ConflictResolution::PreferNewest));
}

#[test]
fn test_is_automatic_for_oldest() {
	// PreferOldest should be automatic
	assert!(ConflictResolver::is_automatic(&ConflictResolution::PreferOldest));
}

#[test]
fn test_is_automatic_for_largest() {
	// PreferLargest should be automatic
	assert!(ConflictResolver::is_automatic(&ConflictResolution::PreferLargest));
}

#[test]
fn test_is_automatic_for_smallest() {
	// PreferSmallest should be automatic
	assert!(ConflictResolver::is_automatic(&ConflictResolution::PreferSmallest));
}

#[test]
fn test_is_automatic_for_first() {
	// PreferFirst should be automatic
	assert!(ConflictResolver::is_automatic(&ConflictResolution::PreferFirst));
}

#[test]
fn test_is_automatic_for_last() {
	// PreferLast should be automatic
	assert!(ConflictResolver::is_automatic(&ConflictResolution::PreferLast));
}

#[test]
fn test_is_automatic_for_fail() {
	// FailOnConflict should be automatic (doesn't require user input, just fails)
	assert!(ConflictResolver::is_automatic(&ConflictResolution::FailOnConflict));
}

#[test]
fn test_is_not_automatic_for_interactive() {
	// Interactive should NOT be automatic
	assert!(!ConflictResolver::is_automatic(&ConflictResolution::Interactive));
}

#[test]
fn test_builder_sets_conflict_resolution_skip() {
	// Verify SyncBuilder correctly stores Skip strategy
	let builder = SyncBuilder::new().conflict_resolution(ConflictResolution::Skip);

	let config = builder.config();
	assert!(matches!(config.conflict_resolution, ConflictResolution::Skip));
}

#[test]
fn test_builder_sets_conflict_resolution_newest() {
	// Verify SyncBuilder correctly stores PreferNewest strategy
	let builder = SyncBuilder::new().conflict_resolution(ConflictResolution::PreferNewest);

	let config = builder.config();
	assert!(matches!(config.conflict_resolution, ConflictResolution::PreferNewest));
}

#[test]
fn test_builder_sets_conflict_resolution_oldest() {
	// Verify SyncBuilder correctly stores PreferOldest strategy
	let builder = SyncBuilder::new().conflict_resolution(ConflictResolution::PreferOldest);

	let config = builder.config();
	assert!(matches!(config.conflict_resolution, ConflictResolution::PreferOldest));
}

#[test]
fn test_builder_sets_conflict_resolution_interactive() {
	// Verify SyncBuilder correctly stores Interactive strategy
	let builder = SyncBuilder::new().conflict_resolution(ConflictResolution::Interactive);

	let config = builder.config();
	assert!(matches!(config.conflict_resolution, ConflictResolution::Interactive));
}

#[test]
fn test_builder_defaults_to_interactive() {
	// Verify SyncBuilder defaults to Interactive
	let builder = SyncBuilder::new();

	let config = builder.config();
	assert!(matches!(config.conflict_resolution, ConflictResolution::Interactive));
}

#[test]
fn test_conflict_resolution_from_str_skip() {
	// Verify parsing "skip" from CLI
	let strategy = ConflictResolution::from_str("skip");
	assert!(strategy.is_ok());
	assert!(matches!(strategy.unwrap(), ConflictResolution::Skip));
}

#[test]
fn test_conflict_resolution_from_str_newest() {
	// Verify parsing "newest" from CLI
	let strategy = ConflictResolution::from_str("newest");
	assert!(strategy.is_ok());
	assert!(matches!(strategy.unwrap(), ConflictResolution::PreferNewest));
}

#[test]
fn test_conflict_resolution_from_str_oldest() {
	// Verify parsing "oldest" from CLI
	let strategy = ConflictResolution::from_str("oldest");
	assert!(strategy.is_ok());
	assert!(matches!(strategy.unwrap(), ConflictResolution::PreferOldest));
}

#[test]
fn test_conflict_resolution_from_str_largest() {
	// Verify parsing "largest" from CLI
	let strategy = ConflictResolution::from_str("largest");
	assert!(strategy.is_ok());
	assert!(matches!(strategy.unwrap(), ConflictResolution::PreferLargest));
}

#[test]
fn test_conflict_resolution_from_str_smallest() {
	// Verify parsing "smallest" from CLI
	let strategy = ConflictResolution::from_str("smallest");
	assert!(strategy.is_ok());
	assert!(matches!(strategy.unwrap(), ConflictResolution::PreferSmallest));
}

#[test]
fn test_conflict_resolution_from_str_first() {
	// Verify parsing "first" from CLI
	let strategy = ConflictResolution::from_str("first");
	assert!(strategy.is_ok());
	assert!(matches!(strategy.unwrap(), ConflictResolution::PreferFirst));
}

#[test]
fn test_conflict_resolution_from_str_last() {
	// Verify parsing "last" from CLI
	let strategy = ConflictResolution::from_str("last");
	assert!(strategy.is_ok());
	assert!(matches!(strategy.unwrap(), ConflictResolution::PreferLast));
}

#[test]
fn test_conflict_resolution_from_str_ask() {
	// Verify parsing "ask" from CLI
	let strategy = ConflictResolution::from_str("ask");
	assert!(strategy.is_ok());
	assert!(matches!(strategy.unwrap(), ConflictResolution::Interactive));
}

#[test]
fn test_conflict_resolution_from_str_fail() {
	// Verify parsing "fail" from CLI
	let strategy = ConflictResolution::from_str("fail");
	assert!(strategy.is_ok());
	assert!(matches!(strategy.unwrap(), ConflictResolution::FailOnConflict));
}

#[test]
fn test_conflict_resolution_from_str_invalid() {
	// Verify invalid input returns None
	let strategy = ConflictResolution::from_str("invalid");
	assert!(strategy.is_err());
}

#[test]
fn test_routing_logic_automatic_strategies() {
	// This test documents the expected routing behavior for automatic strategies
	// When conflict_resolution is Skip/Newest/Oldest/etc., is_automatic should be true

	let automatic_strategies = vec![
		ConflictResolution::Skip,
		ConflictResolution::PreferNewest,
		ConflictResolution::PreferOldest,
		ConflictResolution::PreferLargest,
		ConflictResolution::PreferSmallest,
		ConflictResolution::PreferFirst,
		ConflictResolution::PreferLast,
		ConflictResolution::FailOnConflict,
	];

	for strategy in automatic_strategies {
		assert!(ConflictResolver::is_automatic(&strategy), "{:?} should be automatic", strategy);
	}
}

#[test]
fn test_routing_logic_interactive_strategy() {
	// This test documents the expected routing behavior for Interactive strategy
	// When conflict_resolution is Interactive, is_automatic should be false

	assert!(
		!ConflictResolver::is_automatic(&ConflictResolution::Interactive),
		"Interactive should NOT be automatic"
	);
}

/// Integration test: Verify the bug fix for --conflict skip being ignored
/// This test verifies that when a user specifies --conflict skip (or any automatic strategy),
/// the system correctly routes to non-interactive sync mode.
#[test]
fn test_bug_fix_conflict_option_routing() {
	// Before the fix, the routing logic only checked --skip-conflicts flag
	// and ignored the --conflict option value. This test verifies the fix.

	// Simulate what happens when user runs: syncr sync dir1 dir2 --conflict skip
	let cli_conflict_strategy = ConflictResolution::from_str("skip").unwrap();

	// The routing logic should identify this as automatic
	let is_automatic = ConflictResolver::is_automatic(&cli_conflict_strategy);

	// This should be true, meaning no interactive prompts
	assert!(is_automatic, "Skip strategy should be identified as automatic");

	// Verify all automatic strategies
	for strategy_name in
		&["skip", "newest", "oldest", "largest", "smallest", "first", "last", "fail"]
	{
		let strategy = ConflictResolution::from_str(strategy_name).unwrap();
		assert!(ConflictResolver::is_automatic(&strategy), "{} should be automatic", strategy_name);
	}

	// Interactive ("ask") should NOT be automatic
	let interactive = ConflictResolution::from_str("ask").unwrap();
	assert!(
		!ConflictResolver::is_automatic(&interactive),
		"ask/interactive should NOT be automatic"
	);
}
