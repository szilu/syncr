//! Path validation functions

use std::path::{Component, Path};

use super::ValidationError;

/// Check if a path is safe (no parent directory references)
///
/// This is the extracted validation from original protocol/file_operations.rs:331
/// Ensures paths cannot escape the sync root directory using ".." references.
///
/// # Arguments
/// * `path` - Path to validate
///
/// # Returns
/// `true` if path is safe, `false` if it contains parent directory references
pub fn is_path_safe(path: &Path) -> bool {
	!path.components().any(|c| matches!(c, Component::ParentDir))
}

/// Validate a path is safe
///
/// # Arguments
/// * `path` - Path to validate
///
/// # Returns
/// `Ok(())` if valid, `Err(ValidationError)` if path contains dangerous components
pub fn validate_path_safe(path: &Path) -> Result<(), ValidationError> {
	if !is_path_safe(path) {
		return Err(ValidationError::PathError(
			"Path contains parent directory reference (..)".to_string(),
		));
	}
	Ok(())
}

/// Check if path is within a root directory
///
/// # Arguments
/// * `path` - Path to check
/// * `root` - Root directory that path should be within
///
/// # Returns
/// `true` if path is within root, `false` otherwise
pub fn is_path_within_root(path: &Path, root: &Path) -> bool {
	path.starts_with(root)
}

/// Validate that path is within root directory
///
/// # Arguments
/// * `path` - Path to validate
/// * `root` - Root directory
///
/// # Returns
/// `Ok(())` if valid, `Err(ValidationError)` if path is outside root
pub fn validate_path_within_root(path: &Path, root: &Path) -> Result<(), ValidationError> {
	if !is_path_within_root(path, root) {
		return Err(ValidationError::PathError(format!(
			"Path {:?} is outside root directory {:?}",
			path, root
		)));
	}
	Ok(())
}

/// Check if path has no absolute components
pub fn is_path_relative(path: &Path) -> bool {
	!path.is_absolute()
}

/// Validate that path is relative (not absolute)
pub fn validate_path_relative(path: &Path) -> Result<(), ValidationError> {
	if path.is_absolute() {
		return Err(ValidationError::PathError(format!(
			"Path must be relative, got absolute path: {:?}",
			path
		)));
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_is_path_safe_normal() {
		assert!(is_path_safe(Path::new("file.txt")));
		assert!(is_path_safe(Path::new("dir/file.txt")));
		assert!(is_path_safe(Path::new("a/b/c/file.txt")));
	}

	#[test]
	fn test_is_path_safe_with_parent() {
		assert!(!is_path_safe(Path::new("../file.txt")));
		assert!(!is_path_safe(Path::new("dir/../file.txt")));
		assert!(!is_path_safe(Path::new("a/b/../../file.txt")));
	}

	#[test]
	fn test_validate_path_safe_ok() {
		assert!(validate_path_safe(Path::new("file.txt")).is_ok());
		assert!(validate_path_safe(Path::new("dir/subdir/file.txt")).is_ok());
	}

	#[test]
	fn test_validate_path_safe_err() {
		let result = validate_path_safe(Path::new("../etc/passwd"));
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("parent directory"));
	}

	#[test]
	fn test_is_path_within_root_true() {
		let root = Path::new("/home/user/sync");
		assert!(is_path_within_root(Path::new("/home/user/sync/file.txt"), root));
		assert!(is_path_within_root(Path::new("/home/user/sync/dir/file.txt"), root));
	}

	#[test]
	fn test_is_path_within_root_false() {
		let root = Path::new("/home/user/sync");
		assert!(!is_path_within_root(Path::new("/home/user/other/file.txt"), root));
		assert!(!is_path_within_root(Path::new("/etc/passwd"), root));
	}

	#[test]
	fn test_is_path_relative_true() {
		assert!(is_path_relative(Path::new("file.txt")));
		assert!(is_path_relative(Path::new("dir/file.txt")));
	}

	#[test]
	fn test_is_path_relative_false() {
		assert!(!is_path_relative(Path::new("/file.txt")));
		assert!(!is_path_relative(Path::new("/dir/file.txt")));
	}

	#[test]
	fn test_validate_path_relative_ok() {
		assert!(validate_path_relative(Path::new("file.txt")).is_ok());
		assert!(validate_path_relative(Path::new("dir/file.txt")).is_ok());
	}

	#[test]
	fn test_validate_path_relative_err() {
		let result = validate_path_relative(Path::new("/absolute/path"));
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("must be relative"));
	}
}
