//! Conflict resolution logic
#![allow(dead_code)]

use super::{Conflict, ConflictResolutionError};
use crate::strategies::ConflictResolution;

/// Resolves conflicts using configured strategies
pub struct ConflictResolver {
	/// Default strategy to use
	default_strategy: ConflictResolution,
}

impl ConflictResolver {
	/// Create a new conflict resolver with default strategy
	pub fn new(default_strategy: ConflictResolution) -> Self {
		ConflictResolver { default_strategy }
	}

	/// Resolve a conflict using the configured strategy
	///
	/// Returns the index of the winning version, or None if the conflict should be skipped
	pub fn resolve(
		&self,
		conflict: &Conflict,
		strategy: Option<&ConflictResolution>,
	) -> Result<Option<usize>, ConflictResolutionError> {
		let strategy = strategy.unwrap_or(&self.default_strategy);

		if conflict.versions.is_empty() {
			return Err(ConflictResolutionError::NoVersions);
		}

		match strategy {
			ConflictResolution::PreferFirst => Ok(Some(0)),

			ConflictResolution::PreferLast => Ok(Some(conflict.versions.len().saturating_sub(1))),

			ConflictResolution::PreferNewest => conflict
				.newest_version()
				.ok_or_else(|| {
					ConflictResolutionError::StrategyNotApplicable(
						"Cannot determine newest version".to_string(),
					)
				})
				.map(Some),

			ConflictResolution::PreferOldest => conflict
				.oldest_version()
				.ok_or_else(|| {
					ConflictResolutionError::StrategyNotApplicable(
						"Cannot determine oldest version".to_string(),
					)
				})
				.map(Some),

			ConflictResolution::PreferLargest => conflict
				.largest_version()
				.ok_or_else(|| {
					ConflictResolutionError::StrategyNotApplicable(
						"Cannot determine largest version".to_string(),
					)
				})
				.map(Some),

			ConflictResolution::PreferSmallest => conflict
				.smallest_version()
				.ok_or_else(|| {
					ConflictResolutionError::StrategyNotApplicable(
						"Cannot determine smallest version".to_string(),
					)
				})
				.map(Some),

			ConflictResolution::NodeByIndex(idx) => {
				if *idx >= conflict.versions.len() {
					Err(ConflictResolutionError::InvalidVersion(*idx))
				} else {
					Ok(Some(*idx))
				}
			}

			ConflictResolution::NodeByName(name) => conflict
				.version_by_name(name)
				.ok_or_else(|| ConflictResolutionError::NodeNotFound(name.clone()))
				.map(Some),

			ConflictResolution::Skip => Ok(None),

			ConflictResolution::Interactive => Err(ConflictResolutionError::StrategyNotApplicable(
				"Interactive resolution not supported here".to_string(),
			)),

			ConflictResolution::FailOnConflict => {
				Err(ConflictResolutionError::StrategyNotApplicable(
					"Conflict detected with fail-on-conflict strategy".to_string(),
				))
			}
		}
	}

	/// Check if a strategy is automatic (doesn't require user input)
	pub fn is_automatic(strategy: &ConflictResolution) -> bool {
		!matches!(strategy, ConflictResolution::Interactive)
	}

	/// Get a human-readable description of the strategy
	pub fn strategy_description(strategy: &ConflictResolution) -> &'static str {
		match strategy {
			ConflictResolution::PreferFirst => "Always choose first location",
			ConflictResolution::PreferLast => "Always choose last location",
			ConflictResolution::PreferNewest => "Choose newest modification time",
			ConflictResolution::PreferOldest => "Choose oldest modification time",
			ConflictResolution::PreferLargest => "Choose largest file",
			ConflictResolution::PreferSmallest => "Choose smallest file",
			ConflictResolution::Interactive => "Prompt user interactively",
			ConflictResolution::FailOnConflict => "Fail on any conflict",
			ConflictResolution::Skip => "Skip conflicted files",
			ConflictResolution::NodeByIndex(_) => "Prefer specific node by index",
			ConflictResolution::NodeByName(_) => "Prefer specific node by name",
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::conflict::{Conflict, ConflictType, FileVersion};
	use crate::types::{FileData, FileType};
	use std::path::PathBuf;

	fn create_test_file(mtime: u32, size: u64) -> FileData {
		FileData::builder(FileType::File, PathBuf::from("test.txt"))
			.mode(0o644)
			.user(1000)
			.group(1000)
			.ctime(0)
			.mtime(mtime)
			.size(size)
			.build()
	}

	fn create_test_conflict() -> Conflict {
		let v1 = FileVersion {
			node_index: 0,
			node_location: "node1".to_string(),
			file_data: create_test_file(100, 1024),
		};
		let v2 = FileVersion {
			node_index: 1,
			node_location: "node2".to_string(),
			file_data: create_test_file(200, 2048),
		};

		Conflict::new(1, PathBuf::from("test.txt"), ConflictType::ModifyModify, vec![v1, v2])
	}

	#[test]
	fn test_prefer_first() {
		let resolver = ConflictResolver::new(ConflictResolution::PreferFirst);
		let conflict = create_test_conflict();

		let result = resolver.resolve(&conflict, None).unwrap();
		assert_eq!(result, Some(0));
	}

	#[test]
	fn test_prefer_last() {
		let resolver = ConflictResolver::new(ConflictResolution::PreferLast);
		let conflict = create_test_conflict();

		let result = resolver.resolve(&conflict, None).unwrap();
		assert_eq!(result, Some(1));
	}

	#[test]
	fn test_prefer_newest() {
		let resolver = ConflictResolver::new(ConflictResolution::PreferNewest);
		let conflict = create_test_conflict();

		let result = resolver.resolve(&conflict, None).unwrap();
		assert_eq!(result, Some(1)); // node2 has mtime 200
	}

	#[test]
	fn test_prefer_oldest() {
		let resolver = ConflictResolver::new(ConflictResolution::PreferOldest);
		let conflict = create_test_conflict();

		let result = resolver.resolve(&conflict, None).unwrap();
		assert_eq!(result, Some(0)); // node1 has mtime 100
	}

	#[test]
	fn test_prefer_largest() {
		let resolver = ConflictResolver::new(ConflictResolution::PreferLargest);
		let conflict = create_test_conflict();

		let result = resolver.resolve(&conflict, None).unwrap();
		assert_eq!(result, Some(1)); // node2 has size 2048
	}

	#[test]
	fn test_prefer_smallest() {
		let resolver = ConflictResolver::new(ConflictResolution::PreferSmallest);
		let conflict = create_test_conflict();

		let result = resolver.resolve(&conflict, None).unwrap();
		assert_eq!(result, Some(0)); // node1 has size 1024
	}

	#[test]
	fn test_skip() {
		let resolver = ConflictResolver::new(ConflictResolution::Skip);
		let conflict = create_test_conflict();

		let result = resolver.resolve(&conflict, None).unwrap();
		assert_eq!(result, None); // Skip means no winner
	}

	#[test]
	fn test_node_by_index() {
		let resolver = ConflictResolver::new(ConflictResolution::NodeByIndex(1));
		let conflict = create_test_conflict();

		let result = resolver.resolve(&conflict, None).unwrap();
		assert_eq!(result, Some(1));
	}

	#[test]
	fn test_node_by_index_invalid() {
		let resolver = ConflictResolver::new(ConflictResolution::NodeByIndex(10));
		let conflict = create_test_conflict();

		let result = resolver.resolve(&conflict, None);
		assert!(result.is_err());
	}

	#[test]
	fn test_node_by_name() {
		let resolver = ConflictResolver::new(ConflictResolution::NodeByName("node2".to_string()));
		let conflict = create_test_conflict();

		let result = resolver.resolve(&conflict, None).unwrap();
		assert_eq!(result, Some(1));
	}

	#[test]
	fn test_node_by_name_not_found() {
		let resolver =
			ConflictResolver::new(ConflictResolution::NodeByName("nonexistent".to_string()));
		let conflict = create_test_conflict();

		let result = resolver.resolve(&conflict, None);
		assert!(result.is_err());
	}

	#[test]
	fn test_override_strategy() {
		let resolver = ConflictResolver::new(ConflictResolution::PreferFirst);
		let conflict = create_test_conflict();

		// Override with different strategy
		let result = resolver.resolve(&conflict, Some(&ConflictResolution::PreferLast)).unwrap();
		assert_eq!(result, Some(1));
	}

	#[test]
	fn test_is_automatic() {
		assert!(ConflictResolver::is_automatic(&ConflictResolution::PreferFirst));
		assert!(ConflictResolver::is_automatic(&ConflictResolution::Skip));
		assert!(!ConflictResolver::is_automatic(&ConflictResolution::Interactive));
	}
}

// vim: ts=4
