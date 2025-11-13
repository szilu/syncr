#![allow(dead_code)]

//! Metadata comparison definitions
//!
//! Defines the MetadataComparison struct that controls how file metadata is compared.
//! The MetadataStrategy enum has been consolidated to crate::strategies module.

/// Metadata comparison rules
///
/// Defines which metadata fields to compare during sync.
#[derive(Debug, Clone, PartialEq)]
pub struct MetadataComparison {
	/// Compare file ownership (uid/gid)
	pub compare_owner: bool,

	/// Compare file permissions (mode)
	pub compare_permissions: bool,

	/// Compare file timestamps (mtime)
	pub compare_timestamps: bool,

	/// Compare extended attributes
	pub compare_xattrs: bool,

	/// Tolerance for timestamp comparison (in seconds)
	///
	/// Files with timestamp differences within this tolerance are considered equal.
	/// Useful for filesystems with coarse timestamp resolution (e.g., FAT32 has 2-second resolution).
	pub time_tolerance_secs: u64,
}

impl MetadataComparison {
	/// Strict comparison - all metadata must match
	///
	/// Like rsync -a: owner, group, permissions, timestamps must all match.
	/// Best for: backups, system migration, when you have full control.
	pub fn strict() -> Self {
		Self {
			compare_owner: true,
			compare_permissions: true,
			compare_timestamps: true,
			compare_xattrs: true,
			time_tolerance_secs: 0, // Exact match
		}
	}

	/// Smart comparison - auto-detect and adapt
	///
	/// Compares permissions and timestamps, but not ownership (unless root).
	/// Best for: general use, cross-machine sync.
	pub fn smart() -> Self {
		let is_root = crate::util::get_effective_uid() == 0;

		Self {
			compare_owner: is_root,
			compare_permissions: true,
			compare_timestamps: true,
			compare_xattrs: false,  // Conservative default
			time_tolerance_secs: 1, // 1-second tolerance for minor clock skew
		}
	}

	/// Relaxed comparison - content and timestamps only
	///
	/// Ignores ownership and permissions, only compares file content and modification time.
	/// Best for: cross-platform sync, development environments.
	pub fn relaxed() -> Self {
		Self {
			compare_owner: false,
			compare_permissions: false,
			compare_timestamps: true,
			compare_xattrs: false,
			time_tolerance_secs: 2, // 2-second tolerance for FAT32
		}
	}

	/// Content-only comparison - pure content-based
	///
	/// Only compares file content (size + chunks), ignores all metadata.
	/// Best for: build outputs, generated files, when you only care about content.
	pub fn content_only() -> Self {
		Self {
			compare_owner: false,
			compare_permissions: false,
			compare_timestamps: false,
			compare_xattrs: false,
			time_tolerance_secs: u64::MAX, // Infinite tolerance (timestamps ignored)
		}
	}

	/// Create a custom comparison with specific settings
	pub fn custom(
		owner: bool,
		permissions: bool,
		timestamps: bool,
		xattrs: bool,
		time_tolerance: u64,
	) -> Self {
		Self {
			compare_owner: owner,
			compare_permissions: permissions,
			compare_timestamps: timestamps,
			compare_xattrs: xattrs,
			time_tolerance_secs: time_tolerance,
		}
	}
}

impl Default for MetadataComparison {
	fn default() -> Self {
		Self::smart()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::strategies::MetadataStrategy;

	#[test]
	fn test_strict_comparison() {
		let cmp = MetadataComparison::strict();

		assert!(cmp.compare_owner);
		assert!(cmp.compare_permissions);
		assert!(cmp.compare_timestamps);
		assert!(cmp.compare_xattrs);
		assert_eq!(cmp.time_tolerance_secs, 0);
	}

	#[test]
	fn test_smart_comparison() {
		let cmp = MetadataComparison::smart();

		// Owner comparison depends on whether we're root
		let is_root = crate::util::get_effective_uid() == 0;
		assert_eq!(cmp.compare_owner, is_root);

		assert!(cmp.compare_permissions);
		assert!(cmp.compare_timestamps);
		assert!(!cmp.compare_xattrs); // Conservative default
		assert_eq!(cmp.time_tolerance_secs, 1);
	}

	#[test]
	fn test_relaxed_comparison() {
		let cmp = MetadataComparison::relaxed();

		assert!(!cmp.compare_owner);
		assert!(!cmp.compare_permissions);
		assert!(cmp.compare_timestamps);
		assert!(!cmp.compare_xattrs);
		assert_eq!(cmp.time_tolerance_secs, 2);
	}

	#[test]
	fn test_content_only_comparison() {
		let cmp = MetadataComparison::content_only();

		assert!(!cmp.compare_owner);
		assert!(!cmp.compare_permissions);
		assert!(!cmp.compare_timestamps);
		assert!(!cmp.compare_xattrs);
	}

	#[test]
	fn test_custom_comparison() {
		let cmp = MetadataComparison::custom(true, false, true, false, 5);

		assert!(cmp.compare_owner);
		assert!(!cmp.compare_permissions);
		assert!(cmp.compare_timestamps);
		assert!(!cmp.compare_xattrs);
		assert_eq!(cmp.time_tolerance_secs, 5);
	}

	#[test]
	fn test_strategy_to_comparison() {
		let strict_cmp = MetadataStrategy::Strict.to_comparison();
		assert!(strict_cmp.compare_owner);
		assert_eq!(strict_cmp.time_tolerance_secs, 0);

		let content_cmp = MetadataStrategy::ContentOnly.to_comparison();
		assert!(!content_cmp.compare_owner);
		assert!(!content_cmp.compare_timestamps);
	}

	#[test]
	fn test_default_strategy() {
		let strategy = MetadataStrategy::default();
		assert_eq!(strategy, MetadataStrategy::Smart);
	}

	#[test]
	fn test_default_comparison() {
		let comparison = MetadataComparison::default();
		let smart = MetadataComparison::smart();

		assert_eq!(comparison.compare_permissions, smart.compare_permissions);
		assert_eq!(comparison.compare_timestamps, smart.compare_timestamps);
	}
}
