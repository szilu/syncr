#![allow(dead_code)]

//! Pattern-based file exclusion using glob patterns

use super::ExclusionError;
use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};
use std::path::Path;

/// Represents an exclusion or inclusion pattern
#[derive(Debug, Clone)]
pub struct ExclusionPattern {
	/// Original pattern string
	pub pattern: String,

	/// Whether this is an inclusion pattern (overrides exclusions)
	pub is_include: bool,

	/// Whether this pattern is anchored to the root
	pub anchored: bool,
}

impl ExclusionPattern {
	/// Create a new exclusion pattern
	pub fn exclude(pattern: impl Into<String>) -> Self {
		Self { pattern: pattern.into(), is_include: false, anchored: false }
	}

	/// Create a new inclusion pattern
	pub fn include(pattern: impl Into<String>) -> Self {
		Self { pattern: pattern.into(), is_include: true, anchored: false }
	}

	/// Create an anchored pattern (must match from root)
	pub fn anchored(mut self) -> Self {
		self.anchored = true;
		self
	}
}

/// Pattern matcher using globset for efficient matching
pub struct PatternMatcher {
	/// Compiled exclusion patterns
	exclude_set: GlobSet,

	/// Compiled inclusion patterns (higher priority)
	include_set: Option<GlobSet>,

	/// Always-excluded patterns (built-in)
	always_exclude: GlobSet,
}

impl PatternMatcher {
	/// Create a new pattern matcher
	pub fn new(
		exclude_patterns: &[String],
		anchored_patterns: &[String],
	) -> Result<Self, ExclusionError> {
		// Build always-excluded patterns
		let always_exclude = Self::build_always_excluded()?;

		// Build user exclusion patterns
		let exclude_set = Self::build_glob_set(exclude_patterns, false)?;

		// Build anchored patterns if provided
		let include_set = if !anchored_patterns.is_empty() {
			Some(Self::build_glob_set(anchored_patterns, true)?)
		} else {
			None
		};

		Ok(Self { exclude_set, include_set, always_exclude })
	}

	/// Build the always-excluded patterns
	fn build_always_excluded() -> Result<GlobSet, ExclusionError> {
		let patterns = vec![
			".syncr/**",      // SyncR state directory
			"**/*.SyNcR-TmP", // SyncR temporary files
			".Trash-*/**",    // Linux trash
			"lost+found/**",  // Linux filesystem recovery
			"**/.DS_Store",   // macOS cruft
			"**/Thumbs.db",   // Windows cruft
			"**/desktop.ini", // Windows cruft
			"**/*.swp",       // Vim swap files
			"**/*.swo",       // Vim swap files
			"**/*~",          // Editor backups
			"**/.nfs*",       // NFS temp files
		];

		Self::build_glob_set(&patterns.into_iter().map(String::from).collect::<Vec<_>>(), false)
	}

	/// Build a GlobSet from patterns
	fn build_glob_set(patterns: &[String], anchored: bool) -> Result<GlobSet, ExclusionError> {
		let mut builder = GlobSetBuilder::new();

		for pattern in patterns {
			let glob = if anchored {
				GlobBuilder::new(pattern)
					.literal_separator(true)
					.build()
					.map_err(|e| ExclusionError::InvalidPattern(format!("{}: {}", pattern, e)))?
			} else {
				Glob::new(pattern)
					.map_err(|e| ExclusionError::InvalidPattern(format!("{}: {}", pattern, e)))?
			};

			builder.add(glob);
		}

		builder.build().map_err(|e| {
			ExclusionError::InvalidPattern(format!("Failed to build pattern set: {}", e))
		})
	}

	/// Check if a path is excluded by any pattern
	pub fn is_excluded(&self, path: &Path) -> bool {
		// Always-excluded takes highest priority
		if self.always_exclude.is_match(path) {
			return true;
		}

		// If path matches an include pattern, it's NOT excluded
		if let Some(ref include_set) = self.include_set {
			if include_set.is_match(path) {
				return false;
			}
		}

		// Check user exclusion patterns
		self.exclude_set.is_match(path)
	}

	/// Add a runtime pattern (for dynamic exclusions)
	pub fn add_pattern(&mut self, pattern: ExclusionPattern) -> Result<(), ExclusionError> {
		// Rebuild the appropriate glob set with the new pattern
		// Note: This is inefficient - for many dynamic patterns, consider rebuilding once
		// For now, this is a placeholder for future optimization
		let _ = pattern; // Suppress warning
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_basic_exclusion() {
		let patterns = vec!["*.log".to_string(), "*.tmp".to_string()];
		let matcher = PatternMatcher::new(&patterns, &[]).unwrap();

		assert!(matcher.is_excluded(Path::new("test.log")));
		assert!(matcher.is_excluded(Path::new("foo/bar.tmp")));
		assert!(!matcher.is_excluded(Path::new("test.txt")));
	}

	#[test]
	fn test_glob_patterns() {
		let patterns = vec!["**/*.log".to_string(), "test_*".to_string()];
		let matcher = PatternMatcher::new(&patterns, &[]).unwrap();

		assert!(matcher.is_excluded(Path::new("deep/nested/file.log")));
		assert!(matcher.is_excluded(Path::new("test_foo.txt")));
		assert!(!matcher.is_excluded(Path::new("foo_test.txt")));
	}

	#[test]
	fn test_always_excluded() {
		let matcher = PatternMatcher::new(&[], &[]).unwrap();

		// Built-in exclusions
		assert!(matcher.is_excluded(Path::new(".syncr/state.db")));
		assert!(matcher.is_excluded(Path::new("foo/bar.SyNcR-TmP")));
		assert!(matcher.is_excluded(Path::new(".DS_Store")));
		assert!(matcher.is_excluded(Path::new("foo/Thumbs.db")));
		assert!(matcher.is_excluded(Path::new("file.swp")));
		assert!(matcher.is_excluded(Path::new("backup~")));
	}

	#[test]
	fn test_directory_patterns() {
		let patterns = vec!["node_modules/**".to_string(), "target/**".to_string()];
		let matcher = PatternMatcher::new(&patterns, &[]).unwrap();

		assert!(matcher.is_excluded(Path::new("node_modules/package/file.js")));
		assert!(matcher.is_excluded(Path::new("target/release/binary")));
		assert!(!matcher.is_excluded(Path::new("src/main.rs")));
	}

	#[test]
	fn test_inclusion_override() {
		let exclude_patterns = vec!["*.log".to_string()];
		let include_patterns = vec!["important.log".to_string()];
		let matcher = PatternMatcher::new(&exclude_patterns, &include_patterns).unwrap();

		// Should be excluded
		assert!(matcher.is_excluded(Path::new("test.log")));

		// Should be included (overrides exclusion)
		assert!(!matcher.is_excluded(Path::new("important.log")));
	}

	#[test]
	fn test_trash_exclusion() {
		let matcher = PatternMatcher::new(&[], &[]).unwrap();

		assert!(matcher.is_excluded(Path::new(".Trash-1000/file.txt")));
		assert!(matcher.is_excluded(Path::new("lost+found/orphan")));
	}
}
