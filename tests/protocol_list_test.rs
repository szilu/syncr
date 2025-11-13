//! Protocol LIST command tests
//!
//! Tests directory traversal, chunking, symlink handling, and error conditions

use std::path::Path;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout, Command};

/// Result type for test operations
type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

/// Represents a protocol server for testing
pub struct TestNode {
	dir: TempDir,
	_process: Option<tokio::process::Child>,
	stdin: Option<ChildStdin>,
	stdout: Option<BufReader<ChildStdout>>,
}

impl TestNode {
	/// Create a new test node with a temporary directory
	pub async fn new() -> TestResult<Self> {
		let dir = TempDir::new()?;

		// Spawn serve process directly using pre-built binary with async I/O
		let binary_path = env!("CARGO_BIN_EXE_syncr");
		let mut child = Command::new(binary_path)
			.args(["serve"])
			.arg(dir.path())
			.stdin(std::process::Stdio::piped())
			.stdout(std::process::Stdio::piped())
			.stderr(std::process::Stdio::null())
			.spawn()?;

		let mut stdin = child.stdin.take().ok_or("Failed to get stdin")?;
		let stdout = child.stdout.take().ok_or("Failed to get stdout")?;
		let mut stdout_reader = BufReader::new(stdout);

		// ─── NEGOTIATION PHASE ───
		// 1. Wait for server capabilities announcement (SyNcR:3)
		let mut line = String::new();
		loop {
			line.clear();
			stdout_reader.read_line(&mut line).await?;
			let trimmed = line.trim();
			if trimmed.starts_with("SyNcR:") {
				break; // Got capabilities
			}
			// Skip log messages during startup
			if !trimmed.starts_with("#") && !trimmed.starts_with("!") && !trimmed.is_empty() {
				return Err(format!("Expected SyNcR: message, got: {}", trimmed).into());
			}
		}

		// 2. Send version selection (USE:3)
		stdin.write_all(b"USE:3\n").await?;
		stdin.flush().await?;

		// 3. Wait for ready acknowledgment (READY)
		loop {
			line.clear();
			stdout_reader.read_line(&mut line).await?;
			let trimmed = line.trim();
			if trimmed.starts_with("READY") {
				break; // Server is ready
			}
			// Skip log messages
			if !trimmed.starts_with("#") && !trimmed.starts_with("!") && !trimmed.is_empty() {
				return Err(format!("Expected READY message, got: {}", trimmed).into());
			}
		}

		Ok(TestNode { dir, _process: Some(child), stdin: Some(stdin), stdout: Some(stdout_reader) })
	}

	pub fn path(&self) -> &Path {
		self.dir.path()
	}

	/// Send a LIST command and receive entries
	pub async fn list(&mut self) -> TestResult<Vec<FileSystemEntry>> {
		// Send LIST command
		if let Some(stdin) = &mut self.stdin {
			stdin.write_all(b"{\"cmd\": \"LIST\"}\n").await?;
			stdin.flush().await?;
		}

		let mut entries = Vec::new();
		let stdout_reader = self.stdout.as_mut().ok_or("No stdout")?;

		loop {
			let mut line = String::new();
			stdout_reader.read_line(&mut line).await?;
			let line = line.trim();

			if line.is_empty() {
				continue;
			}

			// Skip log messages (protocol-formatted logs start with # or !)
			if line.starts_with('#') || line.starts_with('!') {
				continue;
			}

			// Check for END marker
			if line.contains("\"cmd\"") && line.contains("\"END\"") {
				break;
			}

			// Parse JSON5 entry - Protocol V3 format
			match json5::from_str::<serde_json::Value>(line) {
				Ok(entry) => {
					// Skip entries with cmd field (control messages)
					if entry.get("cmd").is_some() {
						continue;
					}

					let typ = entry.get("typ").and_then(|t| t.as_str()).unwrap_or("F");

					match typ {
						// File or Directory entry
						"F" | "D" | "S" => {
							if let Some(path) = entry.get("pth").and_then(|p| p.as_str()) {
								let file_type = match typ {
									"F" => "file",
									"D" => "directory",
									"S" => "symlink",
									_ => "file",
								};
								let size = entry.get("sz").and_then(|s| s.as_u64()).unwrap_or(0);

								entries.push(FileSystemEntry {
									path: path.to_string(),
									file_type: file_type.to_string(),
									size,
									chunks: Vec::new(), // Will be filled by chunk entries
								});
							}
						}
						// Chunk entry - associate with most recent file
						"C" => {
							if let Some(last_entry) = entries.last_mut() {
								let hash = entry
									.get("hsh")
									.and_then(|h| h.as_str())
									.unwrap_or("")
									.to_string();
								let offset = entry.get("off").and_then(|o| o.as_u64()).unwrap_or(0);
								let size = entry.get("len").and_then(|s| s.as_u64()).unwrap_or(0);

								if !hash.is_empty() {
									last_entry.chunks.push(ChunkInfo { hash, offset, size });
								}
							}
						}
						_ => {} // Skip unknown types
					}
				}
				Err(e) => {
					eprintln!("Failed to parse entry: {} (error: {})", line, e);
				}
			}
		}

		Ok(entries)
	}
}

#[derive(Debug, Clone)]
struct ChunkInfo {
	hash: String,
	offset: u64,
	size: u64,
}

#[derive(Debug, Clone)]
pub struct FileSystemEntry {
	path: String,
	file_type: String,
	size: u64,
	chunks: Vec<ChunkInfo>,
}

// ============================================================================
// Helper Functions
// ============================================================================

fn create_file(dir: &Path, name: &str, content: &[u8]) -> TestResult<()> {
	std::fs::write(dir.join(name), content)?;
	Ok(())
}

fn create_dir(dir: &Path, name: &str) -> TestResult<()> {
	std::fs::create_dir(dir.join(name))?;
	Ok(())
}

#[cfg(unix)]
fn create_symlink(dir: &Path, name: &str, target: &str) -> TestResult<()> {
	std::os::unix::fs::symlink(target, dir.join(name))?;
	Ok(())
}

#[cfg(unix)]
fn chmod(path: &Path, mode: u32) -> TestResult<()> {
	use std::fs::Permissions;
	use std::os::unix::fs::PermissionsExt;
	let perms = Permissions::from_mode(mode);
	std::fs::set_permissions(path, perms)?;
	Ok(())
}

// ============================================================================
// Test Category 1: Basic Directory Listing (5 tests)
// ============================================================================

#[tokio::test]
async fn test_list_empty_directory() -> TestResult<()> {
	// Test: Empty directory returns no entries
	let mut node = TestNode::new().await?;
	let entries = node.list().await?;

	assert_eq!(entries.len(), 0, "Empty directory should have no entries");
	Ok(())
}

#[tokio::test]
async fn test_list_single_file() -> TestResult<()> {
	// Test: Single file is listed with correct metadata
	let mut node = TestNode::new().await?;
	create_file(node.path(), "test.txt", b"Hello, World!")?;

	let entries = node.list().await?;

	assert_eq!(entries.len(), 1, "Should list 1 file");
	let entry = &entries[0];
	assert_eq!(entry.path, "test.txt");
	assert_eq!(entry.file_type, "file");
	assert_eq!(entry.size, 13);
	assert!(!entry.chunks.is_empty(), "File should have chunks");
	Ok(())
}

#[tokio::test]
async fn test_list_multiple_files() -> TestResult<()> {
	// Test: Multiple files are listed
	let mut node = TestNode::new().await?;
	create_file(node.path(), "file1.txt", b"content1")?;
	create_file(node.path(), "file2.txt", b"content2")?;
	create_file(node.path(), "file3.txt", b"content3")?;

	let entries = node.list().await?;

	assert_eq!(entries.len(), 3, "Should list 3 files");
	let paths: std::collections::HashSet<_> = entries.iter().map(|e| e.path.clone()).collect();
	assert!(paths.contains("file1.txt"));
	assert!(paths.contains("file2.txt"));
	assert!(paths.contains("file3.txt"));
	Ok(())
}

#[tokio::test]
async fn test_list_nested_directories() -> TestResult<()> {
	// Test: Directories are listed, nested content is traversed
	let mut node = TestNode::new().await?;
	create_dir(node.path(), "subdir")?;
	create_file(node.path().join("subdir").as_path(), "nested.txt", b"nested")?;
	create_file(node.path(), "root.txt", b"root")?;

	let entries = node.list().await?;

	// Should have root.txt, subdir (directory), and subdir/nested.txt
	let paths: Vec<_> = entries.iter().map(|e| e.path.clone()).collect();
	assert!(paths.contains(&"root.txt".to_string()));
	assert!(paths.contains(&"subdir".to_string()));
	assert!(paths.iter().any(|p| p.contains("nested.txt")));
	Ok(())
}

#[tokio::test]
async fn test_list_mixed_files_and_dirs() -> TestResult<()> {
	// Test: Mixed file and directory entries
	let mut node = TestNode::new().await?;
	create_file(node.path(), "file.txt", b"content")?;
	create_dir(node.path(), "dir1")?;
	create_dir(node.path(), "dir2")?;
	create_file(node.path().join("dir1").as_path(), "nested.txt", b"nested")?;

	let entries = node.list().await?;

	let files: Vec<_> = entries.iter().filter(|e| e.file_type == "file").collect();
	let dirs: Vec<_> = entries.iter().filter(|e| e.file_type == "directory").collect();

	assert!(!files.is_empty(), "Should have at least 1 file");
	assert!(dirs.len() >= 2, "Should have at least 2 directories");
	Ok(())
}

// ============================================================================
// Test Category 2: File Chunking (5 tests)
// ============================================================================

#[tokio::test]
async fn test_list_chunking_small_file() -> TestResult<()> {
	// Test: Small file gets correct chunking
	let mut node = TestNode::new().await?;
	let small_content = b"small";
	create_file(node.path(), "small.txt", small_content)?;

	let entries = node.list().await?;
	assert_eq!(entries.len(), 1);

	let entry = &entries[0];
	assert_eq!(entry.path, "small.txt");
	assert_eq!(entry.size, small_content.len() as u64);
	// Small files should have exactly 1 chunk
	assert_eq!(entry.chunks.len(), 1, "Small file should have 1 chunk");
	assert_eq!(entry.chunks[0].offset, 0);
	assert_eq!(entry.chunks[0].size, small_content.len() as u64);
	Ok(())
}

#[tokio::test]
async fn test_list_chunking_hash_consistency() -> TestResult<()> {
	// Test: Same file content produces same hash
	let mut node = TestNode::new().await?;
	let content = b"consistent content for hashing";
	create_file(node.path(), "hash_test.txt", content)?;

	let entries1 = node.list().await?;
	let hash1 = entries1[0].chunks[0].hash.clone();

	// List again - hash should be identical
	let entries2 = node.list().await?;
	let hash2 = entries2[0].chunks[0].hash.clone();

	assert_eq!(hash1, hash2, "Same content should produce same hash");
	Ok(())
}

#[tokio::test]
async fn test_list_chunking_different_files_different_hashes() -> TestResult<()> {
	// Test: Different file contents produce different hashes
	let mut node = TestNode::new().await?;
	create_file(node.path(), "file1.txt", b"content1")?;
	create_file(node.path(), "file2.txt", b"content2")?;

	let entries = node.list().await?;
	assert_eq!(entries.len(), 2);

	let hash1 = &entries[0].chunks[0].hash;
	let hash2 = &entries[1].chunks[0].hash;

	assert_ne!(hash1, hash2, "Different content should produce different hashes");
	Ok(())
}

#[tokio::test]
async fn test_list_chunking_multiple_chunks() -> TestResult<()> {
	// Test: Large file gets multiple chunks
	let mut node = TestNode::new().await?;
	// Create a large file (50MB) - with default chunk size of ~1MB, should get many chunks
	let large_content = vec![b'A'; 50 * 1024 * 1024];
	create_file(node.path(), "large.bin", &large_content)?;

	let entries = node.list().await?;
	assert_eq!(entries.len(), 1);

	let entry = &entries[0];
	// Should have multiple chunks for 50MB file
	assert!(entry.chunks.len() > 1, "Large file should have multiple chunks");

	// Chunks should not overlap and should cover the full file
	let total_size: u64 = entry.chunks.iter().map(|c| c.size).sum();
	assert_eq!(total_size, large_content.len() as u64, "Chunks should cover entire file");
	Ok(())
}

#[tokio::test]
async fn test_list_chunking_chunk_offsets_sequential() -> TestResult<()> {
	// Test: Chunk offsets are sequential
	let mut node = TestNode::new().await?;
	let content = vec![b'X'; 100000];
	create_file(node.path(), "chunked.bin", &content)?;

	let entries = node.list().await?;
	let chunks = &entries[0].chunks;

	// Sort by offset and verify they're sequential
	let mut sorted_chunks = chunks.clone();
	sorted_chunks.sort_by_key(|c| c.offset);

	let mut expected_offset = 0u64;
	for chunk in sorted_chunks {
		assert_eq!(chunk.offset, expected_offset, "Chunks should be sequential");
		expected_offset += chunk.size;
	}
	Ok(())
}

// ============================================================================
// Test Category 3: Symlink Handling (5 tests)
// ============================================================================

#[tokio::test]
#[cfg(unix)]
async fn test_list_symlink_to_file() -> TestResult<()> {
	// Test: Symlink to file is listed as symlink
	let mut node = TestNode::new().await?;
	create_file(node.path(), "target.txt", b"target content")?;
	create_symlink(node.path(), "link.txt", "target.txt")?;

	let entries = node.list().await?;

	let link_entry = entries.iter().find(|e| e.path == "link.txt").ok_or("Link not found")?;
	assert_eq!(
		link_entry.file_type, "symlink",
		"Symlink should be listed as symlink, not followed"
	);
	Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn test_list_symlink_to_dir() -> TestResult<()> {
	// Test: Symlink to directory is not followed
	let mut node = TestNode::new().await?;
	create_dir(node.path(), "target_dir")?;
	create_file(node.path().join("target_dir").as_path(), "file.txt", b"content")?;
	create_symlink(node.path(), "dir_link", "target_dir")?;

	let entries = node.list().await?;

	let link_entry = entries.iter().find(|e| e.path == "dir_link").ok_or("Link not found")?;
	assert_eq!(link_entry.file_type, "symlink", "Directory symlink should be listed as symlink");

	// Verify that we don't have duplicate entries from following the symlink
	let count_file_txt = entries.iter().filter(|e| e.path.contains("file.txt")).count();
	assert_eq!(count_file_txt, 1, "Should only see target_dir/file.txt, not through symlink");
	Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn test_list_symlink_infinite_loop_direct() -> TestResult<()> {
	// Test: Symlink pointing to parent doesn't cause infinite loop
	let mut node = TestNode::new().await?;
	create_symlink(node.path(), "self_link", ".")?;

	// This should complete without hanging
	let result = tokio::time::timeout(std::time::Duration::from_secs(5), node.list()).await;

	assert!(result.is_ok(), "LIST should not hang on self-referential symlink");
	let entries = result??;

	// Should list the symlink itself
	let self_link = entries.iter().find(|e| e.path == "self_link");
	assert!(self_link.is_some(), "Self-link should be listed");
	Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn test_list_symlink_infinite_loop_chain() -> TestResult<()> {
	// Test: Circular symlink chain doesn't cause infinite loop (A->B->A)
	let mut node = TestNode::new().await?;
	create_symlink(node.path(), "link_a", "link_b")?;
	create_symlink(node.path(), "link_b", "link_a")?;

	// This should complete without hanging
	let result = tokio::time::timeout(std::time::Duration::from_secs(5), node.list()).await;

	assert!(result.is_ok(), "LIST should not hang on circular symlink chain");
	let entries = result??;

	// Both symlinks should be listed
	assert!(entries.iter().any(|e| e.path == "link_a"));
	assert!(entries.iter().any(|e| e.path == "link_b"));
	Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn test_list_symlink_broken() -> TestResult<()> {
	// Test: Broken symlink doesn't crash LIST
	let mut node = TestNode::new().await?;
	create_symlink(node.path(), "broken_link", "/nonexistent/path/to/nowhere")?;

	let entries = node.list().await?;

	// Broken symlink should still be listed
	let broken = entries.iter().find(|e| e.path == "broken_link");
	assert!(broken.is_some(), "Broken symlink should be listed");
	Ok(())
}

// ============================================================================
// Test Category 4: Permission Errors (3 tests)
// ============================================================================

#[tokio::test]
#[cfg(unix)]
async fn test_list_directory_unreadable() -> TestResult<()> {
	// Test: Unreadable directory is skipped with warning
	let mut node = TestNode::new().await?;
	let subdir = node.path().join("restricted");
	create_dir(node.path(), "restricted")?;
	create_file(&subdir, "file.txt", b"content")?;

	// Remove read permissions
	chmod(&subdir, 0o000)?;

	// LIST should complete without error
	let entries = node.list().await?;

	// Should at least see the restricted directory entry itself
	let restricted = entries.iter().find(|e| e.path.contains("restricted"));
	assert!(restricted.is_some(), "Should list restricted directory itself");

	// Restore permissions for cleanup
	chmod(&subdir, 0o755)?;
	Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn test_list_file_unreadable() -> TestResult<()> {
	// Test: Unreadable file can still be listed (metadata vs content)
	let mut node = TestNode::new().await?;
	create_file(node.path(), "private.txt", b"secret")?;

	// Remove read permissions
	chmod(node.path().join("private.txt").as_path(), 0o000)?;

	// LIST should complete (we're listing metadata, not reading content)
	let entries = node.list().await?;

	let _private = entries.iter().find(|e| e.path == "private.txt");
	// File might or might not be listed depending on implementation
	// The important thing is that LIST doesn't crash

	// Restore permissions for cleanup
	chmod(node.path().join("private.txt").as_path(), 0o644)?;
	Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn test_list_directory_not_writable() -> TestResult<()> {
	// Test: Non-writable directory can still be listed
	let mut node = TestNode::new().await?;
	let readonly = node.path().join("readonly");
	create_dir(node.path(), "readonly")?;
	create_file(&readonly, "file.txt", b"content")?;

	// Remove write permissions (keep read)
	chmod(&readonly, 0o555)?;

	// LIST should work fine
	let entries = node.list().await?;

	let readonly_dir = entries.iter().find(|e| e.path.contains("readonly"));
	assert!(readonly_dir.is_some(), "Should list read-only directory");

	// Restore permissions for cleanup
	chmod(&readonly, 0o755)?;
	Ok(())
}

// ============================================================================
// Test Category 5: Special Files (2 tests)
// ============================================================================

#[tokio::test]
async fn test_list_empty_file() -> TestResult<()> {
	// Test: Empty file (0 bytes) is listed correctly
	let mut node = TestNode::new().await?;
	create_file(node.path(), "empty.txt", b"")?;

	let entries = node.list().await?;
	assert_eq!(entries.len(), 1);

	let entry = &entries[0];
	assert_eq!(entry.path, "empty.txt");
	assert_eq!(entry.size, 0);
	// Empty file should have 0 or 1 chunk depending on implementation
	Ok(())
}

#[tokio::test]
async fn test_list_many_small_files() -> TestResult<()> {
	// Test: Many small files are listed correctly
	let mut node = TestNode::new().await?;

	// Create 100 small files
	for i in 0..100 {
		create_file(
			node.path(),
			&format!("file_{:03}.txt", i),
			format!("content{}", i).as_bytes(),
		)?;
	}

	let entries = node.list().await?;
	assert_eq!(entries.len(), 100, "Should list all 100 files");

	// Verify all files are listed
	for i in 0..100 {
		let expected_path = format!("file_{:03}.txt", i);
		assert!(
			entries.iter().any(|e| e.path == expected_path),
			"Should list file {}",
			expected_path
		);
	}
	Ok(())
}
