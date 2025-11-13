use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

// Helper function to create test files
fn create_test_file(dir: &TempDir, name: &str, content: &[u8]) -> PathBuf {
	let file_path = dir.path().join(name);
	let mut file = fs::File::create(&file_path).unwrap();
	file.write_all(content).unwrap();
	file_path
}

#[test]
fn test_create_temp_directory() {
	let temp_dir = TempDir::new().unwrap();
	assert!(temp_dir.path().exists());
	assert!(temp_dir.path().is_dir());
}

#[test]
fn test_create_and_read_file() {
	let temp_dir = TempDir::new().unwrap();
	let content = b"Hello, World!";
	let file_path = create_test_file(&temp_dir, "test.txt", content);

	assert!(file_path.exists());
	let read_content = fs::read(&file_path).unwrap();
	assert_eq!(read_content, content);
}

#[test]
fn test_multiple_files_in_directory() {
	let temp_dir = TempDir::new().unwrap();

	create_test_file(&temp_dir, "file1.txt", b"Content 1");
	create_test_file(&temp_dir, "file2.txt", b"Content 2");
	create_test_file(&temp_dir, "file3.txt", b"Content 3");

	let entries: Vec<_> = fs::read_dir(temp_dir.path()).unwrap().map(|e| e.unwrap()).collect();

	assert_eq!(entries.len(), 3);
}

#[test]
fn test_empty_file() {
	let temp_dir = TempDir::new().unwrap();
	let file_path = create_test_file(&temp_dir, "empty.txt", b"");

	let metadata = fs::metadata(&file_path).unwrap();
	assert_eq!(metadata.len(), 0);
}

#[test]
fn test_large_file() {
	let temp_dir = TempDir::new().unwrap();
	// Create a 1MB file
	let content = vec![0xAB; 1024 * 1024];
	let file_path = create_test_file(&temp_dir, "large.bin", &content);

	let metadata = fs::metadata(&file_path).unwrap();
	assert_eq!(metadata.len(), 1024 * 1024);
}

#[test]
fn test_file_metadata() {
	let temp_dir = TempDir::new().unwrap();
	let file_path = create_test_file(&temp_dir, "meta.txt", b"test data");

	let metadata = fs::metadata(&file_path).unwrap();
	assert!(metadata.is_file());
	assert!(!metadata.is_dir());
	assert_eq!(metadata.len(), 9); // "test data" is 9 bytes
}

#[test]
fn test_directory_creation() {
	let temp_dir = TempDir::new().unwrap();
	let sub_dir = temp_dir.path().join("subdir");

	fs::create_dir(&sub_dir).unwrap();
	assert!(sub_dir.exists());
	assert!(sub_dir.is_dir());
}

#[test]
fn test_nested_directory_structure() {
	let temp_dir = TempDir::new().unwrap();

	let sub_dir1 = temp_dir.path().join("dir1");
	let sub_dir2 = sub_dir1.join("dir2");

	fs::create_dir(&sub_dir1).unwrap();
	fs::create_dir(&sub_dir2).unwrap();

	let file_path = sub_dir2.join("nested.txt");
	let mut file = fs::File::create(&file_path).unwrap();
	file.write_all(b"nested content").unwrap();

	assert!(file_path.exists());
}

#[test]
fn test_binary_file_content() {
	let temp_dir = TempDir::new().unwrap();
	let binary_content: Vec<u8> = vec![0x00, 0xFF, 0xDE, 0xAD, 0xBE, 0xEF];
	let file_path = create_test_file(&temp_dir, "binary.dat", &binary_content);

	let read_content = fs::read(&file_path).unwrap();
	assert_eq!(read_content, binary_content);
}

#[test]
fn test_identical_files_different_locations() {
	let temp_dir = TempDir::new().unwrap();
	let content = b"Identical content for deduplication test";

	let file1 = create_test_file(&temp_dir, "file1.txt", content);
	let file2 = create_test_file(&temp_dir, "file2.txt", content);

	let content1 = fs::read(&file1).unwrap();
	let content2 = fs::read(&file2).unwrap();

	assert_eq!(content1, content2);
	assert_ne!(file1, file2); // Different paths
}

#[test]
fn test_file_with_special_chars() {
	let temp_dir = TempDir::new().unwrap();
	let content = b"Special chars: \n\r\t\x00\xFF";
	let file_path = create_test_file(&temp_dir, "special.txt", content);

	let read_content = fs::read(&file_path).unwrap();
	assert_eq!(read_content, content);
}

#[test]
#[cfg(unix)]
fn test_file_permissions() {
	use std::os::unix::fs::PermissionsExt;

	let temp_dir = TempDir::new().unwrap();
	let file_path = create_test_file(&temp_dir, "perms.txt", b"test");

	let mut perms = fs::metadata(&file_path).unwrap().permissions();
	perms.set_mode(0o644);
	fs::set_permissions(&file_path, perms).unwrap();

	let metadata = fs::metadata(&file_path).unwrap();
	let mode = metadata.permissions().mode();
	// Check the lower 9 bits (rwxrwxrwx)
	assert_eq!(mode & 0o777, 0o644);
}

#[test]
fn test_two_directory_setup() {
	let temp_dir1 = TempDir::new().unwrap();
	let temp_dir2 = TempDir::new().unwrap();

	create_test_file(&temp_dir1, "shared.txt", b"shared content");
	create_test_file(&temp_dir2, "shared.txt", b"shared content");

	let file1 = temp_dir1.path().join("shared.txt");
	let file2 = temp_dir2.path().join("shared.txt");

	let content1 = fs::read(&file1).unwrap();
	let content2 = fs::read(&file2).unwrap();

	assert_eq!(content1, content2);
}

#[test]
fn test_file_modification_detection() {
	let temp_dir = TempDir::new().unwrap();
	let file_path = create_test_file(&temp_dir, "modified.txt", b"original");

	let metadata1 = fs::metadata(&file_path).unwrap();
	let mtime1 = metadata1.modified().unwrap();

	// Sleep to ensure different timestamp
	std::thread::sleep(std::time::Duration::from_millis(10));

	// Modify the file
	let mut file = fs::OpenOptions::new().write(true).truncate(true).open(&file_path).unwrap();
	file.write_all(b"modified").unwrap();
	drop(file);

	let metadata2 = fs::metadata(&file_path).unwrap();
	let mtime2 = metadata2.modified().unwrap();

	assert!(mtime2 > mtime1, "Modified time should be later");
}

#[test]
fn test_exclusion_engine_with_patterns() {
	use std::path::Path;

	let temp_dir = TempDir::new().unwrap();

	// Create test files with different extensions
	create_test_file(&temp_dir, "document.txt", b"text");
	create_test_file(&temp_dir, "debug.log", b"log");
	create_test_file(&temp_dir, "cache.db", b"cache");
	create_test_file(&temp_dir, "temp.tmp", b"temp");

	// Create a subdirectory with files
	fs::create_dir(temp_dir.path().join("subdir")).unwrap();
	create_test_file(&temp_dir, "subdir/file.txt", b"sub text");
	create_test_file(&temp_dir, "subdir/debug.log", b"sub log");

	// Test exclusion engine
	let syncr_lib = syncr::exclusion::ExclusionEngine::new(
		&syncr::exclusion::ExcludeConfig {
			patterns: vec!["*.log".to_string(), "*.tmp".to_string()],
			..Default::default()
		},
		temp_dir.path(),
	)
	.expect("Failed to create exclusion engine");

	// Test that log files are excluded
	assert!(syncr_lib.should_exclude(Path::new("debug.log"), None), "debug.log should be excluded");
	assert!(
		syncr_lib.should_exclude(Path::new("subdir/debug.log"), None),
		"subdir/debug.log should be excluded"
	);
	assert!(syncr_lib.should_exclude(Path::new("temp.tmp"), None), "temp.tmp should be excluded");

	// Test that other files are not excluded
	assert!(
		!syncr_lib.should_exclude(Path::new("document.txt"), None),
		"document.txt should not be excluded"
	);
	assert!(
		!syncr_lib.should_exclude(Path::new("cache.db"), None),
		"cache.db should not be excluded"
	);
	assert!(
		!syncr_lib.should_exclude(Path::new("subdir/file.txt"), None),
		"subdir/file.txt should not be excluded"
	);
}

#[test]
fn test_exclusion_engine_with_includes() {
	use std::path::Path;

	let temp_dir = TempDir::new().unwrap();

	// Create test files
	create_test_file(&temp_dir, "important.log", b"important");
	create_test_file(&temp_dir, "debug.log", b"debug");
	create_test_file(&temp_dir, "app.log", b"app");

	// Test exclusion engine with includes
	let syncr_lib = syncr::exclusion::ExclusionEngine::new_with_includes(
		&syncr::exclusion::ExcludeConfig {
			patterns: vec!["*.log".to_string()],
			..Default::default()
		},
		temp_dir.path(),
		&["important.log".to_string()], // Include important.log even though it matches exclusion
	)
	.expect("Failed to create exclusion engine");

	// Test that included file is NOT excluded
	assert!(
		!syncr_lib.should_exclude(Path::new("important.log"), None),
		"important.log should NOT be excluded (included)"
	);

	// Test that other matching files are still excluded
	assert!(syncr_lib.should_exclude(Path::new("debug.log"), None), "debug.log should be excluded");
	assert!(syncr_lib.should_exclude(Path::new("app.log"), None), "app.log should be excluded");
}

#[test]
fn test_delete_mode_parsing() {
	use std::str::FromStr;
	use syncr::strategies::DeleteMode;

	// Test parsing all delete modes
	assert_eq!(DeleteMode::from_str("sync").ok(), Some(DeleteMode::Sync));
	assert_eq!(DeleteMode::from_str("no-delete").ok(), Some(DeleteMode::NoDelete));
	assert_eq!(DeleteMode::from_str("delete-after").ok(), Some(DeleteMode::DeleteAfter));
	assert_eq!(DeleteMode::from_str("delete-excluded").ok(), Some(DeleteMode::DeleteExcluded));
	assert_eq!(DeleteMode::from_str("trash").ok(), Some(DeleteMode::Trash));

	// Test invalid mode
	assert!(DeleteMode::from_str("invalid").is_err());
	assert!(DeleteMode::from_str("DELETE").is_err()); // Case sensitive
}

#[test]
fn test_delete_handler_allows_deletion() {
	use syncr::delete::{DeleteHandler, DeleteProtection};
	use syncr::strategies::DeleteMode;

	// Sync mode allows deletion
	let handler = DeleteHandler::new(DeleteMode::Sync, DeleteProtection::disabled());
	assert!(handler.check_delete_allowed(10, 100).is_ok());

	// NoDelete mode blocks deletion
	let handler = DeleteHandler::new(DeleteMode::NoDelete, DeleteProtection::disabled());
	assert!(handler.check_delete_allowed(10, 100).is_err());

	// DeleteAfter mode allows deletion (queued for later)
	let handler = DeleteHandler::new(DeleteMode::DeleteAfter, DeleteProtection::disabled());
	assert!(handler.check_delete_allowed(10, 100).is_ok());

	// DeleteExcluded mode allows deletion (filtered by exclusion patterns)
	let handler = DeleteHandler::new(DeleteMode::DeleteExcluded, DeleteProtection::disabled());
	assert!(handler.check_delete_allowed(10, 100).is_ok());

	// Trash mode allows deletion (moves to trash)
	let handler = DeleteHandler::new(DeleteMode::Trash, DeleteProtection::disabled());
	assert!(handler.check_delete_allowed(10, 100).is_ok());
}

#[test]
fn test_delete_protection_count_limit() {
	use std::path::PathBuf;
	use syncr::delete::{DeleteHandler, DeleteProtection};
	use syncr::strategies::DeleteMode;

	let protection = DeleteProtection {
		enabled: true,
		max_delete_count: Some(5),
		max_delete_percent: None,
		backup_dir: None,
		backup_suffix: ".bak".to_string(),
		trash_dir: PathBuf::from("/tmp/trash"),
	};

	let handler = DeleteHandler::new(DeleteMode::Sync, protection);

	// Within limit
	assert!(handler.check_delete_allowed(3, 100).is_ok());

	// At limit
	assert!(handler.check_delete_allowed(5, 100).is_ok());

	// Exceeds limit
	assert!(handler.check_delete_allowed(6, 100).is_err());
}

#[test]
fn test_delete_protection_percent_limit() {
	use std::path::PathBuf;
	use syncr::delete::{DeleteHandler, DeleteProtection};
	use syncr::strategies::DeleteMode;

	let protection = DeleteProtection {
		enabled: true,
		max_delete_count: None,
		max_delete_percent: Some(25),
		backup_dir: None,
		backup_suffix: ".bak".to_string(),
		trash_dir: PathBuf::from("/tmp/trash"),
	};

	let handler = DeleteHandler::new(DeleteMode::Sync, protection);

	// 10% of 100 - within limit
	assert!(handler.check_delete_allowed(10, 100).is_ok());

	// 25% of 100 - at limit
	assert!(handler.check_delete_allowed(25, 100).is_ok());

	// 26% of 100 - exceeds limit
	assert!(handler.check_delete_allowed(26, 100).is_err());
}

#[test]
fn test_delete_protection_combined_limits() {
	use std::path::PathBuf;
	use syncr::delete::{DeleteHandler, DeleteProtection};
	use syncr::strategies::DeleteMode;

	let protection = DeleteProtection {
		enabled: true,
		max_delete_count: Some(50),
		max_delete_percent: Some(10),
		backup_dir: None,
		backup_suffix: ".bak".to_string(),
		trash_dir: PathBuf::from("/tmp/trash"),
	};

	let handler = DeleteHandler::new(DeleteMode::Sync, protection);

	// 5 out of 1000 files = 0.5%, within both limits
	assert!(handler.check_delete_allowed(5, 1000).is_ok());

	// 51 out of 1000 files = 5.1%, exceeds count limit
	assert!(handler.check_delete_allowed(51, 1000).is_err());

	// 100 out of 500 files = 20%, exceeds percent limit
	assert!(handler.check_delete_allowed(100, 500).is_err());

	// 50 out of 500 files = 10%, at both limits
	assert!(handler.check_delete_allowed(50, 500).is_ok());
}

#[test]
fn test_delete_protection_disabled() {
	use syncr::delete::{DeleteHandler, DeleteProtection};
	use syncr::strategies::DeleteMode;

	let protection = DeleteProtection::disabled();
	let handler = DeleteHandler::new(DeleteMode::Sync, protection);

	// Should allow even extremely high numbers when disabled
	assert!(handler.check_delete_allowed(10000, 10).is_ok());
}

#[test]
fn test_delete_mode_default() {
	use syncr::delete::DeleteHandler;
	use syncr::strategies::DeleteMode;

	let handler = DeleteHandler::default();
	assert_eq!(handler.mode(), DeleteMode::Sync);
}

#[test]
fn test_delete_trash_path() {
	use std::path::PathBuf;
	use syncr::delete::DeleteHandler;

	let handler = DeleteHandler::default();
	let original = PathBuf::from("/data/documents/file.txt");
	let trash = handler.trash_path_for(&original);

	// Should end with the filename
	assert!(trash.ends_with("file.txt"));
}

// ===== CONFLICT RESOLUTION TESTS =====

#[test]
fn test_conflict_resolver_instantiation() {
	use syncr::conflict::ConflictResolver;
	use syncr::strategies::ConflictResolution;

	// Test creating resolver with each strategy
	let _resolver = ConflictResolver::new(ConflictResolution::PreferNewest);
	assert!(!ConflictResolver::is_automatic(&ConflictResolution::Interactive));
	assert!(ConflictResolver::is_automatic(&ConflictResolution::PreferNewest));
}

#[test]
fn test_conflict_prefer_newest() {
	use std::path::PathBuf;
	use syncr::conflict::{Conflict, ConflictResolver, ConflictType, FileVersion};
	use syncr::strategies::ConflictResolution;
	use syncr::types::{FileData, FileType};

	let file1 = FileData {
		tp: FileType::File,
		path: PathBuf::from("test.txt"),
		mode: 0o644,
		user: 1000,
		group: 1000,
		ctime: 0,
		mtime: 100, // Older
		size: 100,
		chunks: vec![],
		target: None,
	};

	let file2 = FileData {
		tp: FileType::File,
		path: PathBuf::from("test.txt"),
		mode: 0o644,
		user: 1000,
		group: 1000,
		ctime: 0,
		mtime: 200, // Newer
		size: 200,
		chunks: vec![],
		target: None,
	};

	let versions = vec![
		FileVersion { node_index: 0, node_location: "node1".to_string(), file_data: file1 },
		FileVersion { node_index: 1, node_location: "node2".to_string(), file_data: file2 },
	];

	let conflict =
		Conflict::new(1, PathBuf::from("test.txt"), ConflictType::ModifyModify, versions);
	let resolver = ConflictResolver::new(ConflictResolution::PreferNewest);

	let result = resolver.resolve(&conflict, None).unwrap();
	assert_eq!(result, Some(1)); // node2 is newer
}

#[test]
fn test_conflict_prefer_oldest() {
	use std::path::PathBuf;
	use syncr::conflict::{Conflict, ConflictResolver, ConflictType, FileVersion};
	use syncr::strategies::ConflictResolution;
	use syncr::types::{FileData, FileType};

	let file1 = FileData {
		tp: FileType::File,
		path: PathBuf::from("test.txt"),
		mode: 0o644,
		user: 1000,
		group: 1000,
		ctime: 0,
		mtime: 100, // Older
		size: 100,
		chunks: vec![],
		target: None,
	};

	let file2 = FileData {
		tp: FileType::File,
		path: PathBuf::from("test.txt"),
		mode: 0o644,
		user: 1000,
		group: 1000,
		ctime: 0,
		mtime: 200, // Newer
		size: 200,
		chunks: vec![],
		target: None,
	};

	let versions = vec![
		FileVersion { node_index: 0, node_location: "node1".to_string(), file_data: file1 },
		FileVersion { node_index: 1, node_location: "node2".to_string(), file_data: file2 },
	];

	let conflict =
		Conflict::new(2, PathBuf::from("test.txt"), ConflictType::ModifyModify, versions);
	let resolver = ConflictResolver::new(ConflictResolution::PreferOldest);

	let result = resolver.resolve(&conflict, None).unwrap();
	assert_eq!(result, Some(0)); // node1 is older
}

#[test]
fn test_conflict_prefer_largest() {
	use std::path::PathBuf;
	use syncr::conflict::{Conflict, ConflictResolver, ConflictType, FileVersion};
	use syncr::strategies::ConflictResolution;
	use syncr::types::{FileData, FileType};

	let file1 = FileData {
		tp: FileType::File,
		path: PathBuf::from("test.txt"),
		mode: 0o644,
		user: 1000,
		group: 1000,
		ctime: 0,
		mtime: 100,
		size: 100, // Smaller
		chunks: vec![],
		target: None,
	};

	let file2 = FileData {
		tp: FileType::File,
		path: PathBuf::from("test.txt"),
		mode: 0o644,
		user: 1000,
		group: 1000,
		ctime: 0,
		mtime: 200,
		size: 500, // Larger
		chunks: vec![],
		target: None,
	};

	let versions = vec![
		FileVersion { node_index: 0, node_location: "node1".to_string(), file_data: file1 },
		FileVersion { node_index: 1, node_location: "node2".to_string(), file_data: file2 },
	];

	let conflict =
		Conflict::new(3, PathBuf::from("test.txt"), ConflictType::ModifyModify, versions);
	let resolver = ConflictResolver::new(ConflictResolution::PreferLargest);

	let result = resolver.resolve(&conflict, None).unwrap();
	assert_eq!(result, Some(1)); // node2 is larger
}

#[test]
fn test_conflict_prefer_smallest() {
	use std::path::PathBuf;
	use syncr::conflict::{Conflict, ConflictResolver, ConflictType, FileVersion};
	use syncr::strategies::ConflictResolution;
	use syncr::types::{FileData, FileType};

	let file1 = FileData {
		tp: FileType::File,
		path: PathBuf::from("test.txt"),
		mode: 0o644,
		user: 1000,
		group: 1000,
		ctime: 0,
		mtime: 100,
		size: 100, // Smaller
		chunks: vec![],
		target: None,
	};

	let file2 = FileData {
		tp: FileType::File,
		path: PathBuf::from("test.txt"),
		mode: 0o644,
		user: 1000,
		group: 1000,
		ctime: 0,
		mtime: 200,
		size: 500, // Larger
		chunks: vec![],
		target: None,
	};

	let versions = vec![
		FileVersion { node_index: 0, node_location: "node1".to_string(), file_data: file1 },
		FileVersion { node_index: 1, node_location: "node2".to_string(), file_data: file2 },
	];

	let conflict =
		Conflict::new(4, PathBuf::from("test.txt"), ConflictType::ModifyModify, versions);
	let resolver = ConflictResolver::new(ConflictResolution::PreferSmallest);

	let result = resolver.resolve(&conflict, None).unwrap();
	assert_eq!(result, Some(0)); // node1 is smaller
}

#[test]
fn test_conflict_prefer_first() {
	use std::path::PathBuf;
	use syncr::conflict::{Conflict, ConflictResolver, ConflictType, FileVersion};
	use syncr::strategies::ConflictResolution;
	use syncr::types::{FileData, FileType};

	let file1 = FileData {
		tp: FileType::File,
		path: PathBuf::from("test.txt"),
		mode: 0o644,
		user: 1000,
		group: 1000,
		ctime: 0,
		mtime: 100,
		size: 100,
		chunks: vec![],
		target: None,
	};

	let file2 = FileData {
		tp: FileType::File,
		path: PathBuf::from("test.txt"),
		mode: 0o644,
		user: 1000,
		group: 1000,
		ctime: 0,
		mtime: 200,
		size: 200,
		chunks: vec![],
		target: None,
	};

	let versions = vec![
		FileVersion { node_index: 0, node_location: "node1".to_string(), file_data: file1 },
		FileVersion { node_index: 1, node_location: "node2".to_string(), file_data: file2 },
	];

	let conflict =
		Conflict::new(5, PathBuf::from("test.txt"), ConflictType::ModifyModify, versions);
	let resolver = ConflictResolver::new(ConflictResolution::PreferFirst);

	let result = resolver.resolve(&conflict, None).unwrap();
	assert_eq!(result, Some(0)); // First node
}

#[test]
fn test_conflict_prefer_last() {
	use std::path::PathBuf;
	use syncr::conflict::{Conflict, ConflictResolver, ConflictType, FileVersion};
	use syncr::strategies::ConflictResolution;
	use syncr::types::{FileData, FileType};

	let file1 = FileData {
		tp: FileType::File,
		path: PathBuf::from("test.txt"),
		mode: 0o644,
		user: 1000,
		group: 1000,
		ctime: 0,
		mtime: 100,
		size: 100,
		chunks: vec![],
		target: None,
	};

	let file2 = FileData {
		tp: FileType::File,
		path: PathBuf::from("test.txt"),
		mode: 0o644,
		user: 1000,
		group: 1000,
		ctime: 0,
		mtime: 200,
		size: 200,
		chunks: vec![],
		target: None,
	};

	let versions = vec![
		FileVersion { node_index: 0, node_location: "node1".to_string(), file_data: file1 },
		FileVersion { node_index: 1, node_location: "node2".to_string(), file_data: file2 },
	];

	let conflict =
		Conflict::new(6, PathBuf::from("test.txt"), ConflictType::ModifyModify, versions);
	let resolver = ConflictResolver::new(ConflictResolution::PreferLast);

	let result = resolver.resolve(&conflict, None).unwrap();
	assert_eq!(result, Some(1)); // Last node
}

#[test]
fn test_conflict_skip() {
	use std::path::PathBuf;
	use syncr::conflict::{Conflict, ConflictResolver, ConflictType, FileVersion};
	use syncr::strategies::ConflictResolution;
	use syncr::types::{FileData, FileType};

	let file1 = FileData {
		tp: FileType::File,
		path: PathBuf::from("test.txt"),
		mode: 0o644,
		user: 1000,
		group: 1000,
		ctime: 0,
		mtime: 100,
		size: 100,
		chunks: vec![],
		target: None,
	};

	let file2 = FileData {
		tp: FileType::File,
		path: PathBuf::from("test.txt"),
		mode: 0o644,
		user: 1000,
		group: 1000,
		ctime: 0,
		mtime: 200,
		size: 200,
		chunks: vec![],
		target: None,
	};

	let versions = vec![
		FileVersion { node_index: 0, node_location: "node1".to_string(), file_data: file1 },
		FileVersion { node_index: 1, node_location: "node2".to_string(), file_data: file2 },
	];

	let conflict =
		Conflict::new(7, PathBuf::from("test.txt"), ConflictType::ModifyModify, versions);
	let resolver = ConflictResolver::new(ConflictResolution::Skip);

	let result = resolver.resolve(&conflict, None).unwrap();
	assert_eq!(result, None); // Skip returns None (no winner)
}

#[test]
fn test_conflict_fail_on_conflict() {
	use std::path::PathBuf;
	use syncr::conflict::{Conflict, ConflictResolver, ConflictType, FileVersion};
	use syncr::strategies::ConflictResolution;
	use syncr::types::{FileData, FileType};

	let file1 = FileData {
		tp: FileType::File,
		path: PathBuf::from("test.txt"),
		mode: 0o644,
		user: 1000,
		group: 1000,
		ctime: 0,
		mtime: 100,
		size: 100,
		chunks: vec![],
		target: None,
	};

	let file2 = FileData {
		tp: FileType::File,
		path: PathBuf::from("test.txt"),
		mode: 0o644,
		user: 1000,
		group: 1000,
		ctime: 0,
		mtime: 200,
		size: 200,
		chunks: vec![],
		target: None,
	};

	let versions = vec![
		FileVersion { node_index: 0, node_location: "node1".to_string(), file_data: file1 },
		FileVersion { node_index: 1, node_location: "node2".to_string(), file_data: file2 },
	];

	let conflict =
		Conflict::new(8, PathBuf::from("test.txt"), ConflictType::ModifyModify, versions);
	let resolver = ConflictResolver::new(ConflictResolution::FailOnConflict);

	let result = resolver.resolve(&conflict, None);
	assert!(result.is_err()); // Should return error
}

#[test]
fn test_conflict_interactive() {
	use std::path::PathBuf;
	use syncr::conflict::{Conflict, ConflictResolver, ConflictType, FileVersion};
	use syncr::strategies::ConflictResolution;
	use syncr::types::{FileData, FileType};

	let file1 = FileData {
		tp: FileType::File,
		path: PathBuf::from("test.txt"),
		mode: 0o644,
		user: 1000,
		group: 1000,
		ctime: 0,
		mtime: 100,
		size: 100,
		chunks: vec![],
		target: None,
	};

	let file2 = FileData {
		tp: FileType::File,
		path: PathBuf::from("test.txt"),
		mode: 0o644,
		user: 1000,
		group: 1000,
		ctime: 0,
		mtime: 200,
		size: 200,
		chunks: vec![],
		target: None,
	};

	let versions = vec![
		FileVersion { node_index: 0, node_location: "node1".to_string(), file_data: file1 },
		FileVersion { node_index: 1, node_location: "node2".to_string(), file_data: file2 },
	];

	let conflict =
		Conflict::new(9, PathBuf::from("test.txt"), ConflictType::ModifyModify, versions);
	let resolver = ConflictResolver::new(ConflictResolution::Interactive);

	let result = resolver.resolve(&conflict, None);
	assert!(result.is_err()); // Interactive returns error (not supported in tests)
}
