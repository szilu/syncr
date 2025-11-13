//! Exclusion and filtering system
//!
//! Provides pattern-based exclusion, .gitignore support, and file property filters.
#![allow(dead_code)]

mod filters;
mod ignore;
mod patterns;

pub use filters::FileFilter;
pub use ignore::IgnoreFileMatcher;
pub use patterns::PatternMatcher;

use std::path::Path;

/// Exclusion configuration (local copy for backward compatibility)
/// TODO: Integrate with config::Config when config loading is complete
#[derive(Debug, Clone, Default)]
pub struct ExcludeConfig {
	pub patterns: Vec<String>,
	pub respect_ignore_files: Vec<String>,
	pub custom_ignore_files: Vec<std::path::PathBuf>,
	pub min_size: Option<String>,
	pub max_size: Option<String>,
	pub min_age: Option<String>,
	pub max_age: Option<String>,
	pub exclude_types: Vec<String>,
	pub case_insensitive: bool,
}

/// Combined exclusion engine that applies all configured filters
pub struct ExclusionEngine {
	pattern_matcher: PatternMatcher,
	ignore_matcher: Option<IgnoreFileMatcher>,
	file_filter: FileFilter,
}

impl ExclusionEngine {
	/// Create a new exclusion engine from configuration
	pub fn new(config: &ExcludeConfig, base_path: &Path) -> Result<Self, ExclusionError> {
		Self::new_with_includes(config, base_path, &[])
	}

	/// Create a new exclusion engine with both exclude and include patterns
	///
	/// Include patterns (anchored_patterns) override exclusions.
	pub fn new_with_includes(
		config: &ExcludeConfig,
		base_path: &Path,
		include_patterns: &[String],
	) -> Result<Self, ExclusionError> {
		// Pattern matcher: excludes first param, includes as "anchored_patterns"
		let pattern_matcher = PatternMatcher::new(&config.patterns, include_patterns)?;

		let ignore_matcher = if !config.respect_ignore_files.is_empty() {
			Some(IgnoreFileMatcher::new(base_path, &config.respect_ignore_files)?)
		} else {
			None
		};

		// Parse size strings to u64
		let min_size = if let Some(ref s) = config.min_size {
			Some(filters::SizeFilter::parse_size(s)?)
		} else {
			None
		};

		let max_size = if let Some(ref s) = config.max_size {
			Some(filters::SizeFilter::parse_size(s)?)
		} else {
			None
		};

		// Parse age strings to Duration
		let min_age = if let Some(ref s) = config.min_age {
			Some(filters::AgeFilter::parse_duration(s)?)
		} else {
			None
		};

		let max_age = if let Some(ref s) = config.max_age {
			Some(filters::AgeFilter::parse_duration(s)?)
		} else {
			None
		};

		// Build type filter string from exclude_types
		let filter_types_str = if !config.exclude_types.is_empty() {
			Some(config.exclude_types.join(","))
		} else {
			None
		};

		let file_filter =
			FileFilter::new(min_size, max_size, min_age, max_age, filter_types_str.as_deref())?;

		Ok(Self { pattern_matcher, ignore_matcher, file_filter })
	}

	/// Check if a path should be excluded
	///
	/// Returns true if the path should be excluded from sync.
	/// Considers: patterns, ignore files, and file property filters.
	pub fn should_exclude(&self, path: &Path, metadata: Option<&std::fs::Metadata>) -> bool {
		// Check pattern matcher first (fastest)
		if self.pattern_matcher.is_excluded(path) {
			return true;
		}

		// Check ignore files
		if let Some(ref ignore_matcher) = self.ignore_matcher {
			if ignore_matcher.is_ignored(path) {
				return true;
			}
		}

		// Check file property filters (requires metadata)
		if let Some(meta) = metadata {
			if !self.file_filter.matches(path, meta) {
				return true;
			}
		}

		false
	}

	/// Check if a directory should be excluded
	///
	/// Optimized for directory checking during traversal.
	pub fn should_exclude_dir(&self, path: &Path) -> bool {
		// Directories are always excluded by pattern matcher if they match
		if self.pattern_matcher.is_excluded(path) {
			return true;
		}

		// Check ignore files
		if let Some(ref ignore_matcher) = self.ignore_matcher {
			if ignore_matcher.is_ignored_dir(path) {
				return true;
			}
		}

		false
	}
}

/// Errors that can occur during exclusion processing
#[derive(Debug)]
pub enum ExclusionError {
	/// Failed to parse a glob pattern
	InvalidPattern(String),

	/// Failed to read or parse an ignore file
	IgnoreFileError(String),

	/// Invalid filter configuration
	InvalidFilter(String),
}

impl std::fmt::Display for ExclusionError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			ExclusionError::InvalidPattern(msg) => {
				write!(f, "Invalid exclusion pattern: {}", msg)
			}
			ExclusionError::IgnoreFileError(msg) => {
				write!(f, "Ignore file error: {}", msg)
			}
			ExclusionError::InvalidFilter(msg) => {
				write!(f, "Invalid filter: {}", msg)
			}
		}
	}
}

impl std::error::Error for ExclusionError {}

#[cfg(test)]
mod tests {
	use super::*;
	use tempfile::TempDir;

	#[test]
	fn test_exclusion_engine_basic() {
		let temp_dir = TempDir::new().unwrap();
		let config = ExcludeConfig {
			patterns: vec!["*.log".to_string(), "*.tmp".to_string()],
			..Default::default()
		};

		let engine = ExclusionEngine::new(&config, temp_dir.path()).unwrap();

		// Should exclude .log files
		assert!(engine.should_exclude(Path::new("test.log"), None));

		// Should not exclude other files
		assert!(!engine.should_exclude(Path::new("test.txt"), None));
	}

	#[test]
	fn test_always_excluded() {
		let temp_dir = TempDir::new().unwrap();
		let config = ExcludeConfig::default();

		let engine = ExclusionEngine::new(&config, temp_dir.path()).unwrap();

		// Built-in exclusions should always work
		assert!(engine.should_exclude(Path::new(".syncr/state.json"), None));
		assert!(engine.should_exclude(Path::new("test.SyNcR-TmP"), None));
	}
}
