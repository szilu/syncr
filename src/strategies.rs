//! Consolidated strategy and mode enums
//!
//! Central location for all strategy and mode enums used in synchronization,
//! conflict resolution, metadata handling, and file operations.
//!
//! Each enum includes:
//! - FromStr implementation for CLI and config parsing
//! - Conversion methods to/from config module types

use serde::{Deserialize, Serialize};
use std::str::FromStr;

// Import metadata types needed for conversion methods
// Using a conditional import to avoid circular dependencies during compilation
// These are only used in impl blocks after MetadataStrategy is defined
use crate::metadata::{MetadataComparison, ReconciliationMode};

// ============================================================================
// METADATA STRATEGY
// ============================================================================

/// Metadata comparison strategy during synchronization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum MetadataStrategy {
	/// Strict - all metadata must match (like rsync -a)
	Strict,

	/// Smart - auto-detect capabilities, preserve what we can (default)
	#[default]
	Smart,

	/// Relaxed - only compare content and basic metadata
	Relaxed,

	/// Content-only - pure content-based sync, ignore all metadata
	ContentOnly,
}

impl FromStr for MetadataStrategy {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s.to_lowercase().as_str() {
			"strict" => Ok(Self::Strict),
			"smart" | "auto" => Ok(Self::Smart),
			"relaxed" | "loose" => Ok(Self::Relaxed),
			"content-only" | "content" => Ok(Self::ContentOnly),
			_ => Err(format!(
				"Unknown metadata strategy: {}. Valid options: strict, smart, relaxed, content-only",
				s
			)),
		}
	}
}

impl std::fmt::Display for MetadataStrategy {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Strict => write!(f, "strict"),
			Self::Smart => write!(f, "smart"),
			Self::Relaxed => write!(f, "relaxed"),
			Self::ContentOnly => write!(f, "content-only"),
		}
	}
}

impl MetadataStrategy {
	/// Convert to a MetadataComparison
	#[allow(dead_code)]
	pub fn to_comparison(self) -> MetadataComparison {
		match self {
			Self::Strict => MetadataComparison::strict(),
			Self::Smart => MetadataComparison::smart(),
			Self::Relaxed => MetadataComparison::relaxed(),
			Self::ContentOnly => MetadataComparison::content_only(),
		}
	}

	/// Convert to a ReconciliationMode for capability-aware comparison
	#[allow(dead_code)]
	pub fn to_reconciliation_mode(self) -> ReconciliationMode {
		match self {
			// Strict: Only compare what ALL nodes can preserve (LCD)
			Self::Strict => ReconciliationMode::Lcd,
			// Smart: Compare what any node can preserve (BestEffort)
			Self::Smart => ReconciliationMode::BestEffort,
			// Relaxed: Less strict about metadata (BestEffort)
			Self::Relaxed => ReconciliationMode::BestEffort,
			// ContentOnly: Doesn't matter for reconciliation, use LCD (most conservative)
			Self::ContentOnly => ReconciliationMode::Lcd,
		}
	}
}

// ============================================================================
// CONFLICT RESOLUTION
// ============================================================================

/// Strategy for automatic conflict resolution
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictResolution {
	/// Always choose first location's version
	PreferFirst,

	/// Always choose last location's version
	PreferLast,

	/// Choose newest modification time
	PreferNewest,

	/// Choose oldest modification time
	PreferOldest,

	/// Choose largest file
	PreferLargest,

	/// Choose smallest file
	PreferSmallest,

	/// Prompt user interactively (CLI only)
	Interactive,

	/// Fail on any conflict
	FailOnConflict,

	/// Skip conflicted files (leave unchanged on all nodes)
	Skip,

	/// Specific node wins (by index)
	NodeByIndex(usize),

	/// Specific node wins (by name/location)
	NodeByName(String),
}

impl FromStr for ConflictResolution {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s.to_lowercase().as_str() {
			"first" | "prefer-first" => Ok(Self::PreferFirst),
			"last" | "prefer-last" => Ok(Self::PreferLast),
			"newest" | "prefer-newest" => Ok(Self::PreferNewest),
			"oldest" | "prefer-oldest" => Ok(Self::PreferOldest),
			"largest" | "prefer-largest" => Ok(Self::PreferLargest),
			"smallest" | "prefer-smallest" => Ok(Self::PreferSmallest),
			"interactive" | "ask" => Ok(Self::Interactive),
			"fail" | "error" => Ok(Self::FailOnConflict),
			"skip" => Ok(Self::Skip),
			s if s.starts_with("node:") => {
				let node_id = s.strip_prefix("node:").unwrap().to_string();
				if let Ok(index) = node_id.parse::<usize>() {
					Ok(Self::NodeByIndex(index))
				} else {
					Ok(Self::NodeByName(node_id))
				}
			}
			_ => Err(format!(
				"Unknown conflict resolution: {}. Valid options: first, last, newest, oldest, largest, smallest, interactive, fail, skip, node:<name|index>",
				s
			)),
		}
	}
}

impl std::fmt::Display for ConflictResolution {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::PreferFirst => write!(f, "prefer-first"),
			Self::PreferLast => write!(f, "prefer-last"),
			Self::PreferNewest => write!(f, "prefer-newest"),
			Self::PreferOldest => write!(f, "prefer-oldest"),
			Self::PreferLargest => write!(f, "prefer-largest"),
			Self::PreferSmallest => write!(f, "prefer-smallest"),
			Self::Interactive => write!(f, "interactive"),
			Self::FailOnConflict => write!(f, "fail"),
			Self::Skip => write!(f, "skip"),
			Self::NodeByIndex(idx) => write!(f, "node:{}", idx),
			Self::NodeByName(name) => write!(f, "node:{}", name),
		}
	}
}

// ============================================================================
// DELETE MODE
// ============================================================================

/// Delete propagation mode during synchronization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum DeleteMode {
	/// Propagate deletes across nodes (default)
	#[default]
	Sync,

	/// Never delete files
	NoDelete,

	/// Delete after successful sync
	DeleteAfter,

	/// Delete files matching exclusion patterns
	DeleteExcluded,

	/// Move to trash instead of deleting
	Trash,
}

impl FromStr for DeleteMode {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s.to_lowercase().as_str() {
			"sync" => Ok(Self::Sync),
			"no-delete" | "nodelete" => Ok(Self::NoDelete),
			"delete-after" | "deleteafter" => Ok(Self::DeleteAfter),
			"delete-excluded" | "deleteexcluded" => Ok(Self::DeleteExcluded),
			"trash" => Ok(Self::Trash),
			_ => Err(format!(
				"Unknown delete mode: {}. Valid options: sync, no-delete, delete-after, delete-excluded, trash",
				s
			)),
		}
	}
}

impl std::fmt::Display for DeleteMode {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Sync => write!(f, "sync"),
			Self::NoDelete => write!(f, "no-delete"),
			Self::DeleteAfter => write!(f, "delete-after"),
			Self::DeleteExcluded => write!(f, "delete-excluded"),
			Self::Trash => write!(f, "trash"),
		}
	}
}

impl DeleteMode {
	/// Check if deletions are allowed in this mode
	pub fn allows_deletion(&self) -> bool {
		!matches!(self, DeleteMode::NoDelete)
	}
}

// ============================================================================
// SYMLINK MODE
// ============================================================================

/// Symlink handling mode during synchronization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SymlinkMode {
	/// Keep as symlinks (default)
	#[default]
	Preserve,

	/// Follow and sync target
	Follow,

	/// Skip symlinks
	Ignore,

	/// Convert absolute to relative
	Relative,
}

impl FromStr for SymlinkMode {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s.to_lowercase().as_str() {
			"preserve" | "keep" => Ok(Self::Preserve),
			"follow" => Ok(Self::Follow),
			"ignore" | "skip" => Ok(Self::Ignore),
			"relative" => Ok(Self::Relative),
			_ => Err(format!(
				"Unknown symlink mode: {}. Valid options: preserve, follow, ignore, relative",
				s
			)),
		}
	}
}

impl std::fmt::Display for SymlinkMode {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Preserve => write!(f, "preserve"),
			Self::Follow => write!(f, "follow"),
			Self::Ignore => write!(f, "ignore"),
			Self::Relative => write!(f, "relative"),
		}
	}
}

// ============================================================================
// CONFIG CONVERSIONS
// ============================================================================
// Note: These conversions are defined in config module to avoid circular
// dependencies, since strategies is declared before config in lib.rs

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_metadata_strategy_from_str() {
		assert_eq!(MetadataStrategy::from_str("strict").unwrap(), MetadataStrategy::Strict);
		assert_eq!(MetadataStrategy::from_str("smart").unwrap(), MetadataStrategy::Smart);
		assert_eq!(MetadataStrategy::from_str("auto").unwrap(), MetadataStrategy::Smart);
		assert_eq!(MetadataStrategy::from_str("relaxed").unwrap(), MetadataStrategy::Relaxed);
		assert_eq!(
			MetadataStrategy::from_str("content-only").unwrap(),
			MetadataStrategy::ContentOnly
		);
		assert!(MetadataStrategy::from_str("invalid").is_err());
	}

	#[test]
	fn test_conflict_resolution_from_str() {
		assert_eq!(ConflictResolution::from_str("first").unwrap(), ConflictResolution::PreferFirst);
		assert_eq!(
			ConflictResolution::from_str("newest").unwrap(),
			ConflictResolution::PreferNewest
		);
		assert_eq!(ConflictResolution::from_str("skip").unwrap(), ConflictResolution::Skip);
		assert_eq!(
			ConflictResolution::from_str("interactive").unwrap(),
			ConflictResolution::Interactive
		);
	}

	#[test]
	fn test_delete_mode_from_str() {
		assert_eq!(DeleteMode::from_str("sync").unwrap(), DeleteMode::Sync);
		assert_eq!(DeleteMode::from_str("no-delete").unwrap(), DeleteMode::NoDelete);
		assert_eq!(DeleteMode::from_str("trash").unwrap(), DeleteMode::Trash);
		assert!(DeleteMode::from_str("invalid").is_err());
	}

	#[test]
	fn test_symlink_mode_from_str() {
		assert_eq!(SymlinkMode::from_str("preserve").unwrap(), SymlinkMode::Preserve);
		assert_eq!(SymlinkMode::from_str("follow").unwrap(), SymlinkMode::Follow);
		assert_eq!(SymlinkMode::from_str("ignore").unwrap(), SymlinkMode::Ignore);
	}

	#[test]
	fn test_delete_mode_allows_deletion() {
		assert!(DeleteMode::Sync.allows_deletion());
		assert!(!DeleteMode::NoDelete.allows_deletion());
		assert!(DeleteMode::Trash.allows_deletion());
	}
}

// vim: ts=4
