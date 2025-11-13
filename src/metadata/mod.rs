//! Metadata handling and reconciliation
//!
//! Provides per-node capability detection and metadata reconciliation strategies
//! for handling asymmetric permissions across sync nodes.

mod capabilities;
mod reconciliation;
mod strategy;

pub use capabilities::NodeCapabilities;
#[allow(unused_imports)]
pub use reconciliation::{MetadataReconciler, ReconciliationMode};
pub use strategy::MetadataComparison;
// MetadataStrategy is now consolidated in crate::strategies module
#[allow(unused_imports)]
pub use crate::strategies::MetadataStrategy;

use crate::types::FileData;
use std::error::Error;

/// Errors that can occur during metadata operations
#[derive(Debug)]
#[allow(dead_code)]
pub enum MetadataError {
	/// Failed to detect capabilities
	DetectionFailed(String),

	/// Invalid reconciliation configuration
	InvalidConfig(String),

	/// Metadata conflict that cannot be auto-resolved
	ConflictUnresolvable(String),
}

impl std::fmt::Display for MetadataError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			MetadataError::DetectionFailed(msg) => {
				write!(f, "Capability detection failed: {}", msg)
			}
			MetadataError::InvalidConfig(msg) => {
				write!(f, "Invalid metadata configuration: {}", msg)
			}
			MetadataError::ConflictUnresolvable(msg) => {
				write!(f, "Unresolvable metadata conflict: {}", msg)
			}
		}
	}
}

impl Error for MetadataError {}

/// Compare metadata between two files considering reconciliation strategy
///
/// Returns true if the metadata differs in a way that matters according to the strategy.
#[allow(dead_code)]
pub fn metadata_differs(
	file1: &FileData,
	file2: &FileData,
	comparison: &MetadataComparison,
) -> bool {
	// Size always matters
	if file1.size != file2.size {
		return true;
	}

	// Type always matters
	if file1.tp != file2.tp {
		return true;
	}

	// Timestamp comparison (with tolerance)
	if comparison.compare_timestamps {
		let time_diff = if file1.mtime > file2.mtime {
			(file1.mtime - file2.mtime) as u64
		} else {
			(file2.mtime - file1.mtime) as u64
		};

		if time_diff > comparison.time_tolerance_secs {
			return true;
		}
	}

	// Permissions comparison
	if comparison.compare_permissions && file1.mode != file2.mode {
		return true;
	}

	// Ownership comparison
	if comparison.compare_owner && (file1.user != file2.user || file1.group != file2.group) {
		return true;
	}

	// Extended attributes comparison
	if comparison.compare_xattrs {
		// For now, we don't have xattrs in FileData, so skip this
		// TODO: Add xattrs to FileData when implementing full xattr support
	}

	false
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::types::FileType;

	#[test]
	fn test_metadata_differs_size() {
		let file1 = create_test_file();
		let mut file2 = create_test_file();

		file2.size = 999;

		let comparison = MetadataComparison::content_only();
		assert!(metadata_differs(&file1, &file2, &comparison));
	}

	#[test]
	fn test_metadata_differs_type() {
		let file1 = create_test_file();
		let mut file2 = create_test_file();

		file2.tp = FileType::Dir;

		let comparison = MetadataComparison::content_only();
		assert!(metadata_differs(&file1, &file2, &comparison));
	}

	#[test]
	fn test_metadata_differs_ownership_ignored() {
		let file1 = create_test_file();
		let mut file2 = create_test_file();

		file2.user = 999;
		file2.group = 999;

		// Content-only mode ignores ownership
		let comparison = MetadataComparison::content_only();
		assert!(!metadata_differs(&file1, &file2, &comparison));

		// Strict mode compares ownership
		let comparison = MetadataComparison::strict();
		assert!(metadata_differs(&file1, &file2, &comparison));
	}

	#[test]
	fn test_metadata_differs_permissions() {
		let file1 = create_test_file();
		let mut file2 = create_test_file();

		file2.mode = 0o644;

		// Relaxed mode ignores permissions
		let comparison = MetadataComparison::relaxed();
		assert!(!metadata_differs(&file1, &file2, &comparison));

		// Strict mode compares permissions
		let comparison = MetadataComparison::strict();
		assert!(metadata_differs(&file1, &file2, &comparison));
	}

	#[test]
	fn test_metadata_differs_time_tolerance() {
		let mut file1 = create_test_file();
		let mut file2 = create_test_file();

		file1.mtime = 1000;
		file2.mtime = 1001; // 1 second difference

		// Smart mode has 1-second tolerance
		let comparison = MetadataComparison::smart();
		assert!(!metadata_differs(&file1, &file2, &comparison));

		// Strict mode has 0-second tolerance
		let comparison = MetadataComparison::strict();
		assert!(metadata_differs(&file1, &file2, &comparison));
	}

	fn create_test_file() -> FileData {
		use std::path::PathBuf;

		FileData::builder(FileType::File, PathBuf::from("test.txt"))
			.mode(0o755)
			.user(1000)
			.group(1000)
			.ctime(1000)
			.mtime(1000)
			.size(100)
			.build()
	}
}
