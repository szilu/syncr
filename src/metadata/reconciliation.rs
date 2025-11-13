#![allow(dead_code)]

//! Metadata reconciliation strategies
//!
//! Handles metadata reconciliation across nodes with different capabilities.

use super::capabilities::NodeCapabilities;
use super::strategy::MetadataComparison;
use serde::{Deserialize, Serialize};

/// Metadata reconciliation mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ReconciliationMode {
	/// Least Common Denominator - compare only what ALL nodes can preserve
	#[default]
	Lcd,

	/// Best Effort - each node preserves what it can, no conflicts for unsupported features
	BestEffort,

	/// Source Wins - one node is authoritative for metadata
	SourceWins,
}

impl ReconciliationMode {
	/// Parse from string
	/// Note: This uses Option instead of Result to allow easy CLI parsing without error handling
	#[allow(clippy::should_implement_trait)]
	pub fn from_str(s: &str) -> Option<Self> {
		match s.to_lowercase().as_str() {
			"lcd" | "least-common-denominator" => Some(Self::Lcd),
			"best-effort" | "besteffort" => Some(Self::BestEffort),
			"source-wins" | "sourcewins" | "source" => Some(Self::SourceWins),
			_ => None,
		}
	}
}

/// Metadata reconciler that determines how to handle metadata across nodes
pub struct MetadataReconciler {
	mode: ReconciliationMode,
	source_node_index: Option<usize>,
}

impl MetadataReconciler {
	/// Create a new metadata reconciler
	pub fn new(mode: ReconciliationMode) -> Self {
		Self { mode, source_node_index: None }
	}

	/// Create a reconciler with source-wins mode
	pub fn source_wins(source_index: usize) -> Self {
		Self { mode: ReconciliationMode::SourceWins, source_node_index: Some(source_index) }
	}

	/// Compute metadata comparison rules from node capabilities
	///
	/// # Arguments
	/// * `capabilities` - Capabilities of all nodes in the sync
	///
	/// # Returns
	/// A `MetadataComparison` that defines which metadata fields to compare
	pub fn compute_comparison(&self, capabilities: &[NodeCapabilities]) -> MetadataComparison {
		match self.mode {
			ReconciliationMode::Lcd => self.compute_lcd(capabilities),
			ReconciliationMode::BestEffort => self.compute_best_effort(capabilities),
			ReconciliationMode::SourceWins => self.compute_source_wins(capabilities),
		}
	}

	/// Least Common Denominator strategy
	///
	/// Only compare metadata that ALL nodes can preserve.
	/// This prevents false conflicts due to capability differences.
	fn compute_lcd(&self, capabilities: &[NodeCapabilities]) -> MetadataComparison {
		if capabilities.is_empty() {
			return MetadataComparison::content_only();
		}

		// All nodes must support a feature to compare it
		let all_can_chown = capabilities.iter().all(|c| c.can_chown);
		let all_can_chmod = capabilities.iter().all(|c| c.can_chmod);
		let all_can_xattrs = capabilities.iter().all(|c| c.can_set_xattrs);
		let all_case_sensitive = capabilities.iter().all(|c| c.case_sensitive);

		MetadataComparison {
			compare_owner: all_can_chown,
			compare_permissions: all_can_chmod,
			compare_timestamps: true, // Almost always safe
			compare_xattrs: all_can_xattrs,
			time_tolerance_secs: if all_case_sensitive { 1 } else { 2 }, // FAT32 has 2-second resolution
		}
	}

	/// Best Effort strategy
	///
	/// Compare all metadata, but each node only preserves what it can.
	/// This can lead to "stable" states where files differ only in unsupported metadata.
	fn compute_best_effort(&self, capabilities: &[NodeCapabilities]) -> MetadataComparison {
		if capabilities.is_empty() {
			return MetadataComparison::smart();
		}

		// Compare everything that at least one node can preserve
		let any_can_chown = capabilities.iter().any(|c| c.can_chown);
		let any_can_chmod = capabilities.iter().any(|c| c.can_chmod);
		let any_can_xattrs = capabilities.iter().any(|c| c.can_set_xattrs);

		// Use relaxed time tolerance if any filesystem is case-insensitive (likely FAT32)
		let time_tolerance = if capabilities.iter().any(|c| !c.case_sensitive) {
			2 // FAT32 resolution
		} else {
			1 // Normal resolution
		};

		MetadataComparison {
			compare_owner: any_can_chown,
			compare_permissions: any_can_chmod,
			compare_timestamps: true,
			compare_xattrs: any_can_xattrs,
			time_tolerance_secs: time_tolerance,
		}
	}

	/// Source Wins strategy
	///
	/// Use the source node's capabilities to determine what to compare.
	/// Other nodes follow the source's metadata.
	fn compute_source_wins(&self, capabilities: &[NodeCapabilities]) -> MetadataComparison {
		let source_caps = match self.source_node_index {
			Some(idx) if idx < capabilities.len() => &capabilities[idx],
			_ => {
				// No valid source specified, fall back to LCD
				return self.compute_lcd(capabilities);
			}
		};

		MetadataComparison {
			compare_owner: source_caps.can_chown,
			compare_permissions: source_caps.can_chmod,
			compare_timestamps: true,
			compare_xattrs: source_caps.can_set_xattrs,
			time_tolerance_secs: if source_caps.case_sensitive { 1 } else { 2 },
		}
	}

	/// Get the reconciliation mode
	pub fn mode(&self) -> ReconciliationMode {
		self.mode
	}
}

impl Default for MetadataReconciler {
	fn default() -> Self {
		Self::new(ReconciliationMode::default())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_lcd_all_capable() {
		let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);

		// All nodes are root with full capabilities
		let caps = vec![NodeCapabilities::all(), NodeCapabilities::all(), NodeCapabilities::all()];

		let comparison = reconciler.compute_comparison(&caps);

		// All metadata should be compared
		assert!(comparison.compare_owner);
		assert!(comparison.compare_permissions);
		assert!(comparison.compare_timestamps);
		assert!(comparison.compare_xattrs);
	}

	#[test]
	fn test_lcd_mixed_capabilities() {
		let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);

		// Mixed: root and user nodes
		let caps = vec![
			NodeCapabilities::all(),  // Root node
			NodeCapabilities::none(), // User node
		];

		let comparison = reconciler.compute_comparison(&caps);

		// Only common capabilities should be compared
		assert!(!comparison.compare_owner); // User can't chown
		assert!(!comparison.compare_permissions); // User can't chmod (in our test)
		assert!(comparison.compare_timestamps); // Both can do timestamps
		assert!(!comparison.compare_xattrs); // User node doesn't support xattrs
	}

	#[test]
	fn test_best_effort_mixed() {
		let reconciler = MetadataReconciler::new(ReconciliationMode::BestEffort);

		let caps = vec![
			NodeCapabilities::all(),  // Root node
			NodeCapabilities::none(), // User node
		];

		let comparison = reconciler.compute_comparison(&caps);

		// Best effort compares what ANY node can do
		assert!(comparison.compare_owner); // At least one can chown
		assert!(comparison.compare_xattrs); // At least one supports xattrs
	}

	#[test]
	fn test_source_wins() {
		let mut root_caps = NodeCapabilities::all();
		root_caps.effective_uid = 0;

		let mut user_caps = NodeCapabilities::none();
		user_caps.effective_uid = 1000;

		let caps = vec![root_caps, user_caps];

		// Source is node 0 (root)
		let reconciler = MetadataReconciler::source_wins(0);
		let comparison = reconciler.compute_comparison(&caps);

		// Should use source (root) capabilities
		assert!(comparison.compare_owner);
		assert!(comparison.compare_permissions);
		assert!(comparison.compare_xattrs);

		// Source is node 1 (user)
		let reconciler = MetadataReconciler::source_wins(1);
		let comparison = reconciler.compute_comparison(&caps);

		// Should use source (user) capabilities
		assert!(!comparison.compare_owner);
		assert!(!comparison.compare_permissions);
		assert!(!comparison.compare_xattrs);
	}

	#[test]
	fn test_fat32_time_tolerance() {
		let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);

		let mut caps1 = NodeCapabilities::all();
		caps1.case_sensitive = true; // ext4

		let mut caps2 = NodeCapabilities::all();
		caps2.case_sensitive = false; // FAT32

		let caps = vec![caps1, caps2];
		let comparison = reconciler.compute_comparison(&caps);

		// Should use 2-second tolerance for FAT32
		assert_eq!(comparison.time_tolerance_secs, 2);
	}

	#[test]
	fn test_reconciliation_mode_from_str() {
		assert_eq!(ReconciliationMode::from_str("lcd"), Some(ReconciliationMode::Lcd));
		assert_eq!(
			ReconciliationMode::from_str("best-effort"),
			Some(ReconciliationMode::BestEffort)
		);
		assert_eq!(
			ReconciliationMode::from_str("source-wins"),
			Some(ReconciliationMode::SourceWins)
		);
		assert_eq!(ReconciliationMode::from_str("invalid"), None);
	}

	#[test]
	fn test_empty_capabilities() {
		let reconciler = MetadataReconciler::new(ReconciliationMode::Lcd);
		let comparison = reconciler.compute_comparison(&[]);

		// Should fall back to content-only for empty capabilities
		assert!(!comparison.compare_owner);
		assert!(!comparison.compare_permissions);
	}
}
