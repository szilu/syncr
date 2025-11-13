//! File property filters (size, age, type)

use super::ExclusionError;
use std::fs::Metadata;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// Filter files based on size, age, and type
pub struct FileFilter {
	size: Option<SizeFilter>,
	age: Option<AgeFilter>,
	types: Option<TypeFilter>,
}

impl FileFilter {
	/// Create a new file filter
	pub fn new(
		min_size: Option<u64>,
		max_size: Option<u64>,
		min_age: Option<Duration>,
		max_age: Option<Duration>,
		filter_types: Option<&str>,
	) -> Result<Self, ExclusionError> {
		let size = if min_size.is_some() || max_size.is_some() {
			Some(SizeFilter::new(min_size, max_size)?)
		} else {
			None
		};

		let age = if min_age.is_some() || max_age.is_some() {
			Some(AgeFilter::new(min_age, max_age)?)
		} else {
			None
		};

		let types = if let Some(types_str) = filter_types {
			Some(TypeFilter::new(types_str)?)
		} else {
			None
		};

		Ok(Self { size, age, types })
	}

	/// Check if a file matches all configured filters
	///
	/// Returns true if the file passes all filters (should be included).
	/// Returns false if the file fails any filter (should be excluded).
	pub fn matches(&self, path: &Path, metadata: &Metadata) -> bool {
		// Check size filter
		if let Some(ref size_filter) = self.size {
			if !size_filter.matches(metadata) {
				return false;
			}
		}

		// Check age filter
		if let Some(ref age_filter) = self.age {
			if !age_filter.matches(metadata) {
				return false;
			}
		}

		// Check type filter
		if let Some(ref type_filter) = self.types {
			if !type_filter.matches(path, metadata) {
				return false;
			}
		}

		true
	}
}

/// Filter files by size
#[derive(Debug, Clone)]
pub struct SizeFilter {
	min_size: Option<u64>,
	max_size: Option<u64>,
}

impl SizeFilter {
	/// Create a new size filter
	pub fn new(min_size: Option<u64>, max_size: Option<u64>) -> Result<Self, ExclusionError> {
		if let (Some(min), Some(max)) = (min_size, max_size) {
			if min > max {
				return Err(ExclusionError::InvalidFilter(
					"min_size cannot be greater than max_size".to_string(),
				));
			}
		}

		Ok(Self { min_size, max_size })
	}

	/// Check if a file matches the size filter
	pub fn matches(&self, metadata: &Metadata) -> bool {
		let size = metadata.len();

		if let Some(min) = self.min_size {
			if size < min {
				return false;
			}
		}

		if let Some(max) = self.max_size {
			if size > max {
				return false;
			}
		}

		true
	}

	/// Parse a size string (e.g., "10K", "5M", "1G")
	pub fn parse_size(s: &str) -> Result<u64, ExclusionError> {
		let s = s.trim().to_uppercase();

		let (num_str, multiplier) = if s.ends_with('K') {
			(&s[..s.len() - 1], 1024u64)
		} else if s.ends_with('M') {
			(&s[..s.len() - 1], 1024 * 1024)
		} else if s.ends_with('G') {
			(&s[..s.len() - 1], 1024 * 1024 * 1024)
		} else if s.ends_with('T') {
			(&s[..s.len() - 1], 1024 * 1024 * 1024 * 1024)
		} else {
			(s.as_str(), 1)
		};

		let num: u64 = num_str
			.parse()
			.map_err(|_| ExclusionError::InvalidFilter(format!("Invalid size: {}", s)))?;

		Ok(num * multiplier)
	}
}

/// Filter files by age (modification time)
#[derive(Debug, Clone)]
pub struct AgeFilter {
	min_age: Option<Duration>,
	max_age: Option<Duration>,
}

impl AgeFilter {
	/// Create a new age filter
	pub fn new(
		min_age: Option<Duration>,
		max_age: Option<Duration>,
	) -> Result<Self, ExclusionError> {
		if let (Some(min), Some(max)) = (min_age, max_age) {
			if min > max {
				return Err(ExclusionError::InvalidFilter(
					"min_age cannot be greater than max_age".to_string(),
				));
			}
		}

		Ok(Self { min_age, max_age })
	}

	/// Check if a file matches the age filter
	pub fn matches(&self, metadata: &Metadata) -> bool {
		let modified = match metadata.modified() {
			Ok(time) => time,
			Err(_) => return true, // If we can't get mtime, don't filter it out
		};

		let now = SystemTime::now();
		let age = match now.duration_since(modified) {
			Ok(duration) => duration,
			Err(_) => return true, // File is in the future? Don't filter it out
		};

		if let Some(min) = self.min_age {
			if age < min {
				return false;
			}
		}

		if let Some(max) = self.max_age {
			if age > max {
				return false;
			}
		}

		true
	}

	/// Parse a duration string (e.g., "7d", "2h", "30m", "90s")
	pub fn parse_duration(s: &str) -> Result<Duration, ExclusionError> {
		let s = s.trim().to_lowercase();

		let (num_str, multiplier) = if s.ends_with('d') {
			(&s[..s.len() - 1], 24 * 60 * 60)
		} else if s.ends_with('h') {
			(&s[..s.len() - 1], 60 * 60)
		} else if s.ends_with('m') {
			(&s[..s.len() - 1], 60)
		} else if s.ends_with('s') {
			(&s[..s.len() - 1], 1)
		} else if s.ends_with("ms") {
			return Err(ExclusionError::InvalidFilter(
				"Milliseconds not supported for age filter".to_string(),
			));
		} else {
			// Default to seconds if no unit specified
			(s.as_str(), 1)
		};

		let num: u64 = num_str
			.parse()
			.map_err(|_| ExclusionError::InvalidFilter(format!("Invalid duration: {}", s)))?;

		Ok(Duration::from_secs(num * multiplier))
	}
}

/// Filter files by type (file, dir, symlink)
#[derive(Debug, Clone)]
pub struct TypeFilter {
	allow_files: bool,
	allow_dirs: bool,
	allow_symlinks: bool,
}

impl TypeFilter {
	/// Create a new type filter from a comma-separated string
	///
	/// Examples: "file", "dir", "file,symlink"
	pub fn new(types: &str) -> Result<Self, ExclusionError> {
		let mut allow_files = false;
		let mut allow_dirs = false;
		let mut allow_symlinks = false;

		for t in types.split(',').map(|s| s.trim().to_lowercase()) {
			match t.as_str() {
				"file" | "files" | "f" => allow_files = true,
				"dir" | "directory" | "directories" | "d" => allow_dirs = true,
				"symlink" | "link" | "l" => allow_symlinks = true,
				_ => {
					return Err(ExclusionError::InvalidFilter(format!(
						"Invalid file type: {}. Valid types: file, dir, symlink",
						t
					)))
				}
			}
		}

		if !allow_files && !allow_dirs && !allow_symlinks {
			return Err(ExclusionError::InvalidFilter(
				"At least one file type must be specified".to_string(),
			));
		}

		Ok(Self { allow_files, allow_dirs, allow_symlinks })
	}

	/// Check if a file matches the type filter
	pub fn matches(&self, _path: &Path, metadata: &Metadata) -> bool {
		let file_type = metadata.file_type();

		if file_type.is_file() {
			self.allow_files
		} else if file_type.is_dir() {
			self.allow_dirs
		} else if file_type.is_symlink() {
			self.allow_symlinks
		} else {
			// Unknown file type - allow by default
			true
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs;
	use tempfile::TempDir;

	#[test]
	fn test_size_filter_basic() {
		let filter = SizeFilter::new(Some(100), Some(1000)).unwrap();

		// Create mock metadata
		let temp_dir = TempDir::new().unwrap();
		let small_file = temp_dir.path().join("small.txt");
		let medium_file = temp_dir.path().join("medium.txt");
		let large_file = temp_dir.path().join("large.txt");

		fs::write(&small_file, "x").unwrap(); // 1 byte
		fs::write(&medium_file, "x".repeat(500)).unwrap(); // 500 bytes
		fs::write(&large_file, "x".repeat(2000)).unwrap(); // 2000 bytes

		assert!(!filter.matches(&fs::metadata(&small_file).unwrap()));
		assert!(filter.matches(&fs::metadata(&medium_file).unwrap()));
		assert!(!filter.matches(&fs::metadata(&large_file).unwrap()));
	}

	#[test]
	fn test_parse_size() {
		assert_eq!(SizeFilter::parse_size("100").unwrap(), 100);
		assert_eq!(SizeFilter::parse_size("10K").unwrap(), 10 * 1024);
		assert_eq!(SizeFilter::parse_size("5M").unwrap(), 5 * 1024 * 1024);
		assert_eq!(SizeFilter::parse_size("2G").unwrap(), 2 * 1024 * 1024 * 1024);

		// Case insensitive
		assert_eq!(SizeFilter::parse_size("10k").unwrap(), 10 * 1024);
		assert_eq!(SizeFilter::parse_size("5m").unwrap(), 5 * 1024 * 1024);
	}

	#[test]
	fn test_age_filter() {
		let filter = AgeFilter::new(Some(Duration::from_secs(60)), None).unwrap();

		let temp_dir = TempDir::new().unwrap();
		let file_path = temp_dir.path().join("test.txt");
		fs::write(&file_path, "test").unwrap();

		let metadata = fs::metadata(&file_path).unwrap();

		// File is new (just created), so it should NOT match (age < 60 seconds)
		assert!(!filter.matches(&metadata));

		// We can't easily test old files in a unit test without mocking time
	}

	#[test]
	fn test_parse_duration() {
		assert_eq!(AgeFilter::parse_duration("60s").unwrap(), Duration::from_secs(60));
		assert_eq!(AgeFilter::parse_duration("5m").unwrap(), Duration::from_secs(5 * 60));
		assert_eq!(AgeFilter::parse_duration("2h").unwrap(), Duration::from_secs(2 * 60 * 60));
		assert_eq!(AgeFilter::parse_duration("7d").unwrap(), Duration::from_secs(7 * 24 * 60 * 60));

		// No unit defaults to seconds
		assert_eq!(AgeFilter::parse_duration("120").unwrap(), Duration::from_secs(120));
	}

	#[test]
	fn test_type_filter() {
		let file_filter = TypeFilter::new("file").unwrap();
		let dir_filter = TypeFilter::new("dir").unwrap();
		let multi_filter = TypeFilter::new("file,symlink").unwrap();

		let temp_dir = TempDir::new().unwrap();

		// Create a file
		let file_path = temp_dir.path().join("test.txt");
		fs::write(&file_path, "test").unwrap();
		let file_meta = fs::metadata(&file_path).unwrap();

		// Create a directory
		let dir_path = temp_dir.path().join("subdir");
		fs::create_dir(&dir_path).unwrap();
		let dir_meta = fs::metadata(&dir_path).unwrap();

		// Test file filter
		assert!(file_filter.matches(&file_path, &file_meta));
		assert!(!file_filter.matches(&dir_path, &dir_meta));

		// Test dir filter
		assert!(!dir_filter.matches(&file_path, &file_meta));
		assert!(dir_filter.matches(&dir_path, &dir_meta));

		// Test multi filter
		assert!(multi_filter.matches(&file_path, &file_meta));
		assert!(!multi_filter.matches(&dir_path, &dir_meta));
	}

	#[test]
	fn test_type_filter_aliases() {
		// Test various aliases
		assert!(TypeFilter::new("f").is_ok());
		assert!(TypeFilter::new("d").is_ok());
		assert!(TypeFilter::new("l").is_ok());
		assert!(TypeFilter::new("files").is_ok());
		assert!(TypeFilter::new("directories").is_ok());
		assert!(TypeFilter::new("link").is_ok());

		// Invalid type should error
		assert!(TypeFilter::new("invalid").is_err());
	}

	#[test]
	fn test_file_filter_combined() {
		let temp_dir = TempDir::new().unwrap();

		// Create test files with different sizes
		let small_file = temp_dir.path().join("small.txt");
		fs::write(&small_file, "x".repeat(50)).unwrap();

		let medium_file = temp_dir.path().join("medium.txt");
		fs::write(&medium_file, "x".repeat(500)).unwrap();

		let large_file = temp_dir.path().join("large.txt");
		fs::write(&large_file, "x".repeat(2000)).unwrap();

		// Filter: size between 100-1000 bytes, type=file
		let filter = FileFilter::new(Some(100), Some(1000), None, None, Some("file")).unwrap();

		// Small file should not match (size < 100)
		let small_meta = fs::metadata(&small_file).unwrap();
		assert!(!filter.matches(&small_file, &small_meta));

		// Medium file should match (100 <= size <= 1000)
		let medium_meta = fs::metadata(&medium_file).unwrap();
		assert!(filter.matches(&medium_file, &medium_meta));

		// Large file should not match (size > 1000)
		let large_meta = fs::metadata(&large_file).unwrap();
		assert!(!filter.matches(&large_file, &large_meta));
	}
}
