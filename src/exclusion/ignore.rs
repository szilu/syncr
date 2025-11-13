#![allow(dead_code)]

//! .gitignore and .syncignore file parsing and matching
//!
//! Uses the `ignore` crate (same as ripgrep) for robust gitignore-style pattern handling.

use super::ExclusionError;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::{Path, PathBuf};

/// Parses and applies ignore files (.gitignore, .syncignore, etc.)
pub struct IgnoreFileMatcher {
	/// Base directory for relative path resolution
	base_path: PathBuf,

	/// Compiled gitignore matcher
	gitignore: Gitignore,
}

impl IgnoreFileMatcher {
	/// Create a new ignore file matcher
	///
	/// # Arguments
	/// * `base_path` - Base directory to search for ignore files
	/// * `respect_files` - List of ignore file types to respect (e.g., ["gitignore", "syncignore"])
	pub fn new(base_path: &Path, respect_files: &[String]) -> Result<Self, ExclusionError> {
		let mut builder = GitignoreBuilder::new(base_path);

		// Add .syncignore if requested (highest priority)
		if respect_files.contains(&"syncignore".to_string()) {
			let syncignore = base_path.join(".syncignore");
			if syncignore.exists() {
				// add() returns Option<Error>, None on success
				if let Some(err) = builder.add(syncignore) {
					return Err(ExclusionError::IgnoreFileError(format!(
						"Failed to add .syncignore: {}",
						err
					)));
				}
			}
		}

		// Add .gitignore if requested
		if respect_files.contains(&"gitignore".to_string()) {
			let gitignore = base_path.join(".gitignore");
			if gitignore.exists() {
				if let Some(err) = builder.add(gitignore) {
					return Err(ExclusionError::IgnoreFileError(format!(
						"Failed to add .gitignore: {}",
						err
					)));
				}
			}
		}

		// Add .dockerignore if requested
		if respect_files.contains(&"dockerignore".to_string()) {
			let dockerignore = base_path.join(".dockerignore");
			if dockerignore.exists() {
				if let Some(err) = builder.add(dockerignore) {
					return Err(ExclusionError::IgnoreFileError(format!(
						"Failed to add .dockerignore: {}",
						err
					)));
				}
			}
		}

		// Add .npmignore if requested
		if respect_files.contains(&"npmignore".to_string()) {
			let npmignore = base_path.join(".npmignore");
			if npmignore.exists() {
				if let Some(err) = builder.add(npmignore) {
					return Err(ExclusionError::IgnoreFileError(format!(
						"Failed to add .npmignore: {}",
						err
					)));
				}
			}
		}

		// Add .rgignore if requested
		if respect_files.contains(&"rgignore".to_string()) {
			let rgignore = base_path.join(".rgignore");
			if rgignore.exists() {
				if let Some(err) = builder.add(rgignore) {
					return Err(ExclusionError::IgnoreFileError(format!(
						"Failed to add .rgignore: {}",
						err
					)));
				}
			}
		}

		let gitignore =
			builder.build().map_err(|e| ExclusionError::IgnoreFileError(e.to_string()))?;

		Ok(Self { base_path: base_path.to_path_buf(), gitignore })
	}

	/// Check if a path is ignored
	pub fn is_ignored(&self, path: &Path) -> bool {
		// Make path relative to base if it's absolute
		let relative_path = if path.is_absolute() {
			path.strip_prefix(&self.base_path).unwrap_or(path)
		} else {
			path
		};

		// Check if the file itself is ignored
		if self.gitignore.matched(relative_path, false).is_ignore() {
			return true;
		}

		// Also check if any parent directory is ignored
		// This handles cases like "node_modules/" matching "node_modules/file.js"
		for ancestor in relative_path.ancestors().skip(1) {
			if ancestor == Path::new("") || ancestor == Path::new(".") {
				break;
			}
			if self.gitignore.matched(ancestor, true).is_ignore() {
				return true;
			}
		}

		false
	}

	/// Check if a directory is ignored
	///
	/// This is optimized for directory traversal - if a directory is ignored,
	/// we can skip traversing into it entirely.
	pub fn is_ignored_dir(&self, path: &Path) -> bool {
		// Make path relative to base if it's absolute
		let relative_path = if path.is_absolute() {
			path.strip_prefix(&self.base_path).unwrap_or(path)
		} else {
			path
		};

		self.gitignore.matched(relative_path, true).is_ignore()
	}
}

/// Parser for individual ignore files
pub struct IgnoreFileParser;

impl IgnoreFileParser {
	/// Parse a single ignore file and return patterns
	pub fn parse_file(path: &Path) -> Result<Vec<String>, ExclusionError> {
		let contents = std::fs::read_to_string(path).map_err(|e| {
			ExclusionError::IgnoreFileError(format!("Failed to read {}: {}", path.display(), e))
		})?;

		Ok(Self::parse_contents(&contents))
	}

	/// Parse ignore file contents
	pub fn parse_contents(contents: &str) -> Vec<String> {
		contents
			.lines()
			.filter_map(|line| {
				let line = line.trim();

				// Skip empty lines and comments
				if line.is_empty() || line.starts_with('#') {
					return None;
				}

				Some(line.to_string())
			})
			.collect()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs;
	use tempfile::TempDir;

	#[test]
	fn test_gitignore_basic() {
		let temp_dir = TempDir::new().unwrap();
		let gitignore_path = temp_dir.path().join(".gitignore");

		fs::write(
			&gitignore_path,
			r#"
# Comment
*.log
node_modules/
target/

# Another comment
!important.log
"#,
		)
		.unwrap();

		let matcher = IgnoreFileMatcher::new(temp_dir.path(), &["gitignore".to_string()]).unwrap();

		// Should ignore .log files
		assert!(matcher.is_ignored(Path::new("test.log")));
		assert!(matcher.is_ignored(Path::new("foo/bar.log")));

		// Should ignore directories
		assert!(matcher.is_ignored_dir(Path::new("node_modules")));
		assert!(matcher.is_ignored_dir(Path::new("target")));

		// Should not ignore other files
		assert!(!matcher.is_ignored(Path::new("test.txt")));
	}

	#[test]
	fn test_syncignore_priority() {
		let temp_dir = TempDir::new().unwrap();

		// Create .gitignore
		fs::write(temp_dir.path().join(".gitignore"), "*.log\n").unwrap();

		// Create .syncignore with different patterns
		fs::write(temp_dir.path().join(".syncignore"), "*.tmp\n").unwrap();

		let matcher = IgnoreFileMatcher::new(
			temp_dir.path(),
			&["syncignore".to_string(), "gitignore".to_string()],
		)
		.unwrap();

		// Both should be ignored
		assert!(matcher.is_ignored(Path::new("test.log")));
		assert!(matcher.is_ignored(Path::new("test.tmp")));
		assert!(!matcher.is_ignored(Path::new("test.txt")));
	}

	#[test]
	fn test_no_ignore_files() {
		let temp_dir = TempDir::new().unwrap();

		// No ignore files created
		let matcher = IgnoreFileMatcher::new(temp_dir.path(), &["gitignore".to_string()]).unwrap();

		// Nothing should be ignored
		assert!(!matcher.is_ignored(Path::new("test.log")));
		assert!(!matcher.is_ignored(Path::new("anything.txt")));
	}

	#[test]
	fn test_parse_ignore_file() {
		let contents = r#"
# This is a comment
*.log
node_modules/

# Another comment
*.tmp

# Blank lines should be ignored

target/
"#;

		let patterns = IgnoreFileParser::parse_contents(contents);

		assert_eq!(patterns.len(), 4);
		assert!(patterns.contains(&"*.log".to_string()));
		assert!(patterns.contains(&"node_modules/".to_string()));
		assert!(patterns.contains(&"*.tmp".to_string()));
		assert!(patterns.contains(&"target/".to_string()));
	}

	#[test]
	fn test_directory_patterns() {
		let temp_dir = TempDir::new().unwrap();

		fs::write(temp_dir.path().join(".gitignore"), "node_modules/\ntarget/\n.git/\n").unwrap();

		let matcher = IgnoreFileMatcher::new(temp_dir.path(), &["gitignore".to_string()]).unwrap();

		// Directories should be ignored
		assert!(matcher.is_ignored_dir(Path::new("node_modules")));
		assert!(matcher.is_ignored_dir(Path::new("target")));
		assert!(matcher.is_ignored_dir(Path::new(".git")));

		// Files in those directories should also be ignored
		assert!(matcher.is_ignored(Path::new("node_modules/package.json")));
		assert!(matcher.is_ignored(Path::new("target/debug/app")));
	}

	#[test]
	fn test_negation_patterns() {
		let temp_dir = TempDir::new().unwrap();

		fs::write(temp_dir.path().join(".gitignore"), "*.log\n!important.log\n").unwrap();

		let matcher = IgnoreFileMatcher::new(temp_dir.path(), &["gitignore".to_string()]).unwrap();

		// Most .log files should be ignored
		assert!(matcher.is_ignored(Path::new("test.log")));
		assert!(matcher.is_ignored(Path::new("foo/debug.log")));

		// But important.log should NOT be ignored (negation pattern)
		assert!(!matcher.is_ignored(Path::new("important.log")));
	}
}
