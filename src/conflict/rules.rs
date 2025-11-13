//! Per-pattern conflict resolution rules
#![allow(dead_code)]

use crate::strategies::ConflictResolution;
use globset::{Glob, GlobMatcher};
use std::path::Path;

/// A conflict resolution rule for specific file patterns
#[derive(Debug, Clone)]
pub struct ConflictRule {
	/// Glob pattern to match files
	pattern: String,

	/// Compiled glob matcher
	matcher: GlobMatcher,

	/// Strategy to use for matching files
	strategy: ConflictResolution,
}

impl ConflictRule {
	/// Create a new conflict rule
	///
	/// # Arguments
	/// * `pattern` - Glob pattern (e.g., "*.log", "**/*.db")
	/// * `strategy` - Conflict resolution strategy for matching files
	///
	/// # Errors
	/// Returns error if pattern is invalid
	pub fn new(pattern: &str, strategy: ConflictResolution) -> Result<Self, String> {
		let glob = Glob::new(pattern).map_err(|e| format!("Invalid glob pattern: {}", e))?;

		Ok(ConflictRule { pattern: pattern.to_string(), matcher: glob.compile_matcher(), strategy })
	}

	/// Check if this rule matches the given path
	pub fn matches(&self, path: &Path) -> bool {
		self.matcher.is_match(path)
	}

	/// Get the strategy for this rule
	pub fn strategy(&self) -> &ConflictResolution {
		&self.strategy
	}

	/// Get the pattern for this rule
	pub fn pattern(&self) -> &str {
		&self.pattern
	}
}

/// A set of conflict resolution rules
#[derive(Debug, Clone)]
pub struct ConflictRuleSet {
	/// Rules in priority order (first match wins)
	rules: Vec<ConflictRule>,

	/// Default strategy if no rules match
	default_strategy: ConflictResolution,
}

impl ConflictRuleSet {
	/// Create a new rule set with default strategy
	pub fn new(default_strategy: ConflictResolution) -> Self {
		ConflictRuleSet { rules: Vec::new(), default_strategy }
	}

	/// Add a rule to the set
	///
	/// Rules are evaluated in the order they're added (first match wins)
	pub fn add_rule(&mut self, rule: ConflictRule) {
		self.rules.push(rule);
	}

	/// Find the appropriate strategy for a file path
	///
	/// Returns the strategy from the first matching rule, or default if no rules match
	pub fn strategy_for_path(&self, path: &Path) -> &ConflictResolution {
		for rule in &self.rules {
			if rule.matches(path) {
				return rule.strategy();
			}
		}
		&self.default_strategy
	}

	/// Get the number of rules
	pub fn rule_count(&self) -> usize {
		self.rules.len()
	}

	/// Get the default strategy
	pub fn default_strategy(&self) -> &ConflictResolution {
		&self.default_strategy
	}
}

impl Default for ConflictRuleSet {
	fn default() -> Self {
		Self::new(ConflictResolution::Interactive)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::path::PathBuf;

	#[test]
	fn test_rule_creation() {
		let rule = ConflictRule::new("*.log", ConflictResolution::PreferNewest).unwrap();

		assert_eq!(rule.pattern(), "*.log");
		assert!(rule.matches(&PathBuf::from("test.log")));
		assert!(!rule.matches(&PathBuf::from("test.txt")));
	}

	#[test]
	fn test_rule_invalid_pattern() {
		let result = ConflictRule::new("[invalid", ConflictResolution::PreferNewest);
		assert!(result.is_err());
	}

	#[test]
	fn test_rule_wildcard_patterns() {
		let rule = ConflictRule::new("**/*.db", ConflictResolution::Skip).unwrap();

		assert!(rule.matches(&PathBuf::from("foo.db")));
		assert!(rule.matches(&PathBuf::from("dir/foo.db")));
		assert!(rule.matches(&PathBuf::from("deep/nested/dir/foo.db")));
		assert!(!rule.matches(&PathBuf::from("foo.txt")));
	}

	#[test]
	fn test_ruleset_first_match_wins() {
		let mut ruleset = ConflictRuleSet::new(ConflictResolution::Interactive);

		// Add rules in order
		ruleset.add_rule(ConflictRule::new("*.log", ConflictResolution::PreferNewest).unwrap());
		ruleset.add_rule(ConflictRule::new("*.txt", ConflictResolution::PreferOldest).unwrap());
		ruleset.add_rule(ConflictRule::new("*", ConflictResolution::Skip).unwrap()); // Catch-all

		// Check that first matching rule wins
		assert!(matches!(
			ruleset.strategy_for_path(&PathBuf::from("test.log")),
			ConflictResolution::PreferNewest
		));
		assert!(matches!(
			ruleset.strategy_for_path(&PathBuf::from("test.txt")),
			ConflictResolution::PreferOldest
		));
		assert!(matches!(
			ruleset.strategy_for_path(&PathBuf::from("test.db")),
			ConflictResolution::Skip
		));
	}

	#[test]
	fn test_ruleset_default_strategy() {
		let ruleset = ConflictRuleSet::new(ConflictResolution::PreferFirst);

		// No rules added, should use default
		assert!(matches!(
			ruleset.strategy_for_path(&PathBuf::from("anything.txt")),
			ConflictResolution::PreferFirst
		));
	}

	#[test]
	fn test_ruleset_no_match_uses_default() {
		let mut ruleset = ConflictRuleSet::new(ConflictResolution::FailOnConflict);
		ruleset.add_rule(ConflictRule::new("*.log", ConflictResolution::PreferNewest).unwrap());

		// .txt doesn't match any rule, should use default
		assert!(matches!(
			ruleset.strategy_for_path(&PathBuf::from("test.txt")),
			ConflictResolution::FailOnConflict
		));
	}

	#[test]
	fn test_complex_patterns() {
		let mut ruleset = ConflictRuleSet::new(ConflictResolution::Interactive);

		// Database files: skip
		ruleset.add_rule(ConflictRule::new("**/*.db", ConflictResolution::Skip).unwrap());

		// Cache files: prefer largest
		ruleset
			.add_rule(ConflictRule::new("**/*.cache", ConflictResolution::PreferLargest).unwrap());

		// Config files: fail on conflict (require manual resolution)
		ruleset.add_rule(
			ConflictRule::new("**/config.*", ConflictResolution::FailOnConflict).unwrap(),
		);

		// Test files: prefer newest
		ruleset.add_rule(
			ConflictRule::new("**/test_*.txt", ConflictResolution::PreferNewest).unwrap(),
		);

		// Verify matching
		assert!(matches!(
			ruleset.strategy_for_path(&PathBuf::from("data/users.db")),
			ConflictResolution::Skip
		));
		assert!(matches!(
			ruleset.strategy_for_path(&PathBuf::from("cache/items.cache")),
			ConflictResolution::PreferLargest
		));
		assert!(matches!(
			ruleset.strategy_for_path(&PathBuf::from("etc/config.toml")),
			ConflictResolution::FailOnConflict
		));
		assert!(matches!(
			ruleset.strategy_for_path(&PathBuf::from("tests/test_foo.txt")),
			ConflictResolution::PreferNewest
		));
		assert!(matches!(
			ruleset.strategy_for_path(&PathBuf::from("random.txt")),
			ConflictResolution::Interactive
		));
	}

	#[test]
	fn test_rule_count() {
		let mut ruleset = ConflictRuleSet::new(ConflictResolution::Interactive);
		assert_eq!(ruleset.rule_count(), 0);

		ruleset.add_rule(ConflictRule::new("*.log", ConflictResolution::PreferNewest).unwrap());
		assert_eq!(ruleset.rule_count(), 1);

		ruleset.add_rule(ConflictRule::new("*.txt", ConflictResolution::Skip).unwrap());
		assert_eq!(ruleset.rule_count(), 2);
	}
}

// vim: ts=4
