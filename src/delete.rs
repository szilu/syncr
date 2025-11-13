//! File deletion handling with safety features
#![allow(dead_code)]

use crate::strategies::DeleteMode;
use std::path::{Path, PathBuf};

/// Delete protection configuration
#[derive(Debug, Clone, PartialEq)]
pub struct DeleteProtection {
	/// Enable delete protection
	pub enabled: bool,

	/// Maximum number of files to delete in one sync (None = unlimited)
	pub max_delete_count: Option<usize>,

	/// Maximum percentage of files to delete (0-100, None = unlimited)
	pub max_delete_percent: Option<u8>,

	/// Backup deleted files to this directory before removing
	pub backup_dir: Option<PathBuf>,

	/// Suffix for backed up deleted files
	pub backup_suffix: String,

	/// Trash directory (for DeleteMode::Trash)
	pub trash_dir: PathBuf,
}

impl DeleteProtection {
	/// Create a new delete protection config with defaults
	pub fn new() -> Self {
		Self {
			enabled: true,
			max_delete_count: Some(1000),
			max_delete_percent: Some(50),
			backup_dir: None,
			backup_suffix: ".syncr-deleted".to_string(),
			trash_dir: Self::default_trash_dir(),
		}
	}

	/// Create a disabled protection config (no limits)
	pub fn disabled() -> Self {
		Self {
			enabled: false,
			max_delete_count: None,
			max_delete_percent: None,
			backup_dir: None,
			backup_suffix: ".syncr-deleted".to_string(),
			trash_dir: Self::default_trash_dir(),
		}
	}

	/// Check if deletion should be allowed
	///
	/// # Arguments
	/// * `delete_count` - Number of files to delete
	/// * `total_files` - Total number of files
	///
	/// # Returns
	/// Ok(()) if deletion is allowed, Err with reason if not
	pub fn check_allowed(&self, delete_count: usize, total_files: usize) -> Result<(), String> {
		if !self.enabled {
			return Ok(());
		}

		// Check absolute count limit
		if let Some(max_count) = self.max_delete_count {
			if delete_count > max_count {
				return Err(format!(
					"Deletion limit exceeded: {} files to delete, but max is {}",
					delete_count, max_count
				));
			}
		}

		// Check percentage limit
		if let Some(max_percent) = self.max_delete_percent {
			if total_files > 0 {
				let delete_percent = (delete_count * 100) / total_files;
				if delete_percent > max_percent as usize {
					return Err(format!(
						"Deletion percentage limit exceeded: {}% of files to delete, but max is {}%",
						delete_percent, max_percent
					));
				}
			}
		}

		Ok(())
	}

	/// Get the default trash directory (XDG-compliant on Linux)
	fn default_trash_dir() -> PathBuf {
		#[cfg(target_os = "linux")]
		{
			// XDG trash specification
			if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
				PathBuf::from(data_home).join("Trash/files")
			} else if let Ok(home) = std::env::var("HOME") {
				PathBuf::from(home).join(".local/share/Trash/files")
			} else {
				PathBuf::from("/tmp/syncr-trash")
			}
		}

		#[cfg(target_os = "macos")]
		{
			if let Ok(home) = std::env::var("HOME") {
				PathBuf::from(home).join(".Trash")
			} else {
				PathBuf::from("/tmp/syncr-trash")
			}
		}

		#[cfg(target_os = "windows")]
		{
			// Windows Recycle Bin is complex, use temp dir
			PathBuf::from(std::env::temp_dir()).join("syncr-trash")
		}

		#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
		{
			PathBuf::from("/tmp/syncr-trash")
		}
	}
}

impl Default for DeleteProtection {
	fn default() -> Self {
		Self::new()
	}
}

/// Handles file deletions with safety checks
pub struct DeleteHandler {
	/// Delete mode
	mode: DeleteMode,

	/// Delete protection
	protection: DeleteProtection,
}

impl DeleteHandler {
	/// Create a new delete handler
	pub fn new(mode: DeleteMode, protection: DeleteProtection) -> Self {
		DeleteHandler { mode, protection }
	}

	/// Get the delete mode
	pub fn mode(&self) -> DeleteMode {
		self.mode
	}

	/// Get the protection config
	pub fn protection(&self) -> &DeleteProtection {
		&self.protection
	}

	/// Check if a deletion operation is allowed
	///
	/// # Arguments
	/// * `delete_count` - Number of files to delete
	/// * `total_files` - Total number of files
	pub fn check_delete_allowed(
		&self,
		delete_count: usize,
		total_files: usize,
	) -> Result<(), String> {
		// Check mode
		if !self.mode.allows_deletion() {
			return Err("Deletion not allowed in current mode".to_string());
		}

		// Check protection limits
		self.protection.check_allowed(delete_count, total_files)
	}

	/// Get the backup path for a deleted file
	///
	/// Returns None if backup is not configured
	pub fn backup_path_for(&self, original: &Path) -> Option<PathBuf> {
		self.protection.backup_dir.as_ref().map(|backup_dir| {
			let filename = original.file_name().unwrap_or_default();
			let mut backup_name = filename.to_os_string();
			backup_name.push(&self.protection.backup_suffix);
			backup_dir.join(backup_name)
		})
	}

	/// Get the trash path for a deleted file
	pub fn trash_path_for(&self, original: &Path) -> PathBuf {
		let filename = original.file_name().unwrap_or_default();
		self.protection.trash_dir.join(filename)
	}

	/// Should files be backed up before deletion?
	pub fn should_backup(&self) -> bool {
		self.protection.backup_dir.is_some()
	}
}

impl Default for DeleteHandler {
	fn default() -> Self {
		Self::new(DeleteMode::default(), DeleteProtection::default())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_protection_check_count_limit() {
		let protection = DeleteProtection {
			enabled: true,
			max_delete_count: Some(100),
			max_delete_percent: None,
			backup_dir: None,
			backup_suffix: ".deleted".to_string(),
			trash_dir: PathBuf::from("/tmp/trash"),
		};

		// Within limit
		assert!(protection.check_allowed(50, 1000).is_ok());

		// At limit
		assert!(protection.check_allowed(100, 1000).is_ok());

		// Exceeds limit
		assert!(protection.check_allowed(101, 1000).is_err());
	}

	#[test]
	fn test_protection_check_percent_limit() {
		let protection = DeleteProtection {
			enabled: true,
			max_delete_count: None,
			max_delete_percent: Some(50),
			backup_dir: None,
			backup_suffix: ".deleted".to_string(),
			trash_dir: PathBuf::from("/tmp/trash"),
		};

		// 25% - within limit
		assert!(protection.check_allowed(25, 100).is_ok());

		// 50% - at limit
		assert!(protection.check_allowed(50, 100).is_ok());

		// 51% - exceeds limit
		assert!(protection.check_allowed(51, 100).is_err());

		// 100% - exceeds limit
		assert!(protection.check_allowed(100, 100).is_err());
	}

	#[test]
	fn test_protection_disabled() {
		let protection = DeleteProtection::disabled();

		// Any amount should be allowed
		assert!(protection.check_allowed(10000, 100).is_ok());
	}

	#[test]
	fn test_delete_handler_mode_check() {
		let handler = DeleteHandler::new(DeleteMode::NoDelete, DeleteProtection::disabled());

		let result = handler.check_delete_allowed(10, 100);
		assert!(result.is_err());
		assert!(result.unwrap_err().contains("not allowed"));
	}

	#[test]
	fn test_delete_handler_allows_deletion() {
		let handler = DeleteHandler::new(DeleteMode::Sync, DeleteProtection::disabled());

		let result = handler.check_delete_allowed(10, 100);
		assert!(result.is_ok());
	}

	#[test]
	fn test_delete_handler_protection_limits() {
		let protection = DeleteProtection {
			enabled: true,
			max_delete_count: Some(10),
			max_delete_percent: None,
			backup_dir: None,
			backup_suffix: ".deleted".to_string(),
			trash_dir: PathBuf::from("/tmp/trash"),
		};

		let handler = DeleteHandler::new(DeleteMode::Sync, protection);

		assert!(handler.check_delete_allowed(5, 100).is_ok());
		assert!(handler.check_delete_allowed(15, 100).is_err());
	}

	#[test]
	fn test_backup_path() {
		let protection = DeleteProtection {
			enabled: true,
			max_delete_count: None,
			max_delete_percent: None,
			backup_dir: Some(PathBuf::from("/backup")),
			backup_suffix: ".bak".to_string(),
			trash_dir: PathBuf::from("/tmp/trash"),
		};

		let handler = DeleteHandler::new(DeleteMode::Sync, protection);

		let original = PathBuf::from("/data/file.txt");
		let backup = handler.backup_path_for(&original).unwrap();

		assert_eq!(backup, PathBuf::from("/backup/file.txt.bak"));
	}

	#[test]
	fn test_backup_path_none() {
		let handler = DeleteHandler::default();

		let original = PathBuf::from("/data/file.txt");
		let backup = handler.backup_path_for(&original);

		assert!(backup.is_none());
	}

	#[test]
	fn test_trash_path() {
		let handler = DeleteHandler::default();

		let original = PathBuf::from("/data/dir/file.txt");
		let trash = handler.trash_path_for(&original);

		assert!(trash.ends_with("file.txt"));
	}

	#[test]
	fn test_should_backup() {
		let mut protection = DeleteProtection::default();
		let handler = DeleteHandler::new(DeleteMode::Sync, protection.clone());

		assert!(!handler.should_backup());

		protection.backup_dir = Some(PathBuf::from("/backup"));
		let handler = DeleteHandler::new(DeleteMode::Sync, protection);

		assert!(handler.should_backup());
	}

	#[test]
	fn test_combined_limits() {
		let protection = DeleteProtection {
			enabled: true,
			max_delete_count: Some(100),
			max_delete_percent: Some(10),
			backup_dir: None,
			backup_suffix: ".deleted".to_string(),
			trash_dir: PathBuf::from("/tmp/trash"),
		};

		// 50 out of 1000 files = 5%, within both limits
		assert!(protection.check_allowed(50, 1000).is_ok());

		// 101 out of 1000 files = 10.1%, exceeds count limit
		assert!(protection.check_allowed(101, 1000).is_err());

		// 100 out of 500 files = 20%, exceeds percent limit
		assert!(protection.check_allowed(100, 500).is_err());
	}
}

// vim: ts=4
