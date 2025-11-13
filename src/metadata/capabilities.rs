//! Per-node capability detection
//!
//! Detects what metadata operations are supported on each sync node.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Capabilities of a sync node
///
/// Describes what metadata operations this node can perform.
/// Used for metadata reconciliation to avoid false conflicts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct NodeCapabilities {
	/// Can change file ownership (chown) - usually root-only
	#[serde(rename = "canChown")]
	pub can_chown: bool,

	/// Can change file permissions (chmod) - usually always true on Unix
	#[serde(rename = "canChmod")]
	pub can_chmod: bool,

	/// Filesystem supports extended attributes
	#[serde(rename = "canSetXattrs")]
	pub can_set_xattrs: bool,

	/// Can create device nodes (mknod) - root-only
	#[serde(rename = "canCreateDevices")]
	pub can_create_devices: bool,

	/// Can create FIFOs/named pipes - usually always true
	#[serde(rename = "canCreateFifos")]
	pub can_create_fifos: bool,

	/// Current effective user ID
	#[serde(rename = "effectiveUid")]
	pub effective_uid: u32,

	/// Current effective group ID
	#[serde(rename = "effectiveGid")]
	pub effective_gid: u32,

	/// Filesystem type (e.g., "ext4", "btrfs", "vfat")
	#[serde(rename = "filesystemType")]
	pub filesystem_type: String,

	/// Filesystem is case-sensitive
	#[serde(rename = "caseSensitive")]
	pub case_sensitive: bool,
}

impl Default for NodeCapabilities {
	fn default() -> Self {
		Self {
			can_chown: false,
			can_chmod: true,
			can_set_xattrs: false,
			can_create_devices: false,
			can_create_fifos: true,
			effective_uid: 1000,
			effective_gid: 1000,
			filesystem_type: "unknown".to_string(),
			case_sensitive: true,
		}
	}
}

impl NodeCapabilities {
	/// Detect capabilities for the current process and filesystem
	///
	/// # Arguments
	/// * `base_path` - Path to test for filesystem capabilities (optional)
	pub fn detect(base_path: Option<&Path>) -> Self {
		let effective_uid = crate::util::get_effective_uid();
		let effective_gid = crate::util::get_effective_gid();
		let is_root = effective_uid == 0;

		Self {
			can_chown: is_root,
			can_chmod: true, // Almost always true on Unix
			can_set_xattrs: base_path.map(test_xattr_support).unwrap_or(false),
			can_create_devices: is_root,
			can_create_fifos: true, // Almost always true on Unix
			effective_uid,
			effective_gid,
			filesystem_type: base_path
				.and_then(detect_filesystem_type)
				.unwrap_or_else(|| "unknown".to_string()),
			case_sensitive: base_path.map(test_case_sensitivity).unwrap_or(true), // Default to true for Unix
		}
	}

	/// Create a capabilities struct with all features disabled (for testing)
	pub fn none() -> Self {
		Self {
			can_chown: false,
			can_chmod: false,
			can_set_xattrs: false,
			can_create_devices: false,
			can_create_fifos: false,
			effective_uid: 1000,
			effective_gid: 1000,
			filesystem_type: "unknown".to_string(),
			case_sensitive: true,
		}
	}

	/// Create a capabilities struct with all features enabled (for testing)
	pub fn all() -> Self {
		Self {
			can_chown: true,
			can_chmod: true,
			can_set_xattrs: true,
			can_create_devices: true,
			can_create_fifos: true,
			effective_uid: 0,
			effective_gid: 0,
			filesystem_type: "ext4".to_string(),
			case_sensitive: true,
		}
	}

	/// Check if this node is running as root
	pub fn is_root(&self) -> bool {
		self.effective_uid == 0
	}
}

/// Convenience function to detect capabilities
#[allow(dead_code)]
pub fn detect_capabilities(base_path: Option<&Path>) -> NodeCapabilities {
	NodeCapabilities::detect(base_path)
}

// get_effective_uid and get_effective_gid are used via crate::util:: directly below

/// Test if the filesystem supports extended attributes
fn test_xattr_support(_path: &Path) -> bool {
	// TODO: Implement xattr support test
	// For now, assume no xattr support to avoid false positives
	// In a full implementation:
	// 1. Try to set a test xattr on a temp file
	// 2. Try to read it back
	// 3. Clean up
	false
}

/// Detect the filesystem type
fn detect_filesystem_type(_path: &Path) -> Option<String> {
	// TODO: Implement filesystem type detection
	// On Linux: parse /proc/mounts or use statfs()
	// On macOS: use statfs()
	// On Windows: use GetVolumeInformation()
	//
	// For now, return None to indicate unknown
	None
}

/// Test if the filesystem is case-sensitive
fn test_case_sensitivity(path: &Path) -> bool {
	// TODO: Implement case sensitivity test
	// Create a test file "TEST.tmp", try to access it as "test.tmp"
	// If we can access it, filesystem is case-insensitive
	// Clean up test file
	//
	// For now, default to true (case-sensitive, which is safer)
	let _ = path; // Suppress unused warning
	true
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_detect_capabilities() {
		let caps = NodeCapabilities::detect(None);

		// We can't test exact values since they depend on the environment,
		// but we can verify the struct is populated
		// effective_uid and effective_gid are unsigned, so >= 0 is always true
		// Just verify the struct is populated by checking a property that depends on it
		let _ = caps.is_root(); // This depends on the uid values

		// Root detection should be consistent
		assert_eq!(caps.is_root(), caps.effective_uid == 0);
		assert_eq!(caps.can_chown, caps.effective_uid == 0);
	}

	#[test]
	fn test_capabilities_none() {
		let caps = NodeCapabilities::none();

		assert!(!caps.can_chown);
		assert!(!caps.can_chmod);
		assert!(!caps.can_set_xattrs);
		assert!(!caps.can_create_devices);
		assert!(!caps.is_root());
	}

	#[test]
	fn test_capabilities_all() {
		let caps = NodeCapabilities::all();

		assert!(caps.can_chown);
		assert!(caps.can_chmod);
		assert!(caps.can_set_xattrs);
		assert!(caps.can_create_devices);
		assert!(caps.is_root());
	}

	#[test]
	fn test_is_root() {
		let mut caps = NodeCapabilities::none();
		caps.effective_uid = 0;

		assert!(caps.is_root());

		caps.effective_uid = 1000;
		assert!(!caps.is_root());
	}

	#[test]
	fn test_serialization() {
		let caps = NodeCapabilities::all();

		// Test that it can be serialized (needed for protocol communication)
		let json = serde_json::to_string(&caps).unwrap();
		let deserialized: NodeCapabilities = serde_json::from_str(&json).unwrap();

		assert_eq!(caps, deserialized);
	}
}
