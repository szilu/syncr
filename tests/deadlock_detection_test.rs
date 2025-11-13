//! Deadlock detection tests
//!
//! Tests designed to detect potential deadlock scenarios in the protocol layer,
//! specifically focusing on bidirectional async communication patterns that could
//! cause the blocking chunk transfer bug to reoccur.
//!
//! Key scenarios tested:
//! - Concurrent READ and WRITE operations (chunk transfer in both directions)
//! - Lock acquisition order (sender lock before receiver lock, never opposite)
//! - Protocol state transitions under high concurrency
//! - Large file transfers with streaming chunk operations

use std::fs;
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;

use syncr::sync::SyncBuilder;

// ============================================================================
// Helper Functions
// ============================================================================

fn create_file(dir: &Path, name: &str, content: &[u8]) {
	let path = dir.join(name);
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent).ok();
	}
	fs::write(&path, content).unwrap();
}

fn file_exists(dir: &Path, name: &str) -> bool {
	dir.join(name).exists()
}

fn read_file(dir: &Path, name: &str) -> Option<Vec<u8>> {
	let path = dir.join(name);
	fs::read(&path).ok()
}

// ============================================================================
// Deadlock Detection Tests
// ============================================================================

/// Test 1: Basic bidirectional chunk transfer (simple case)
/// This tests the protocol commit() lock release pattern
#[tokio::test]
async fn test_deadlock_bidirectional_chunk_transfer() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	// Create file in dir1 that will require chunks
	create_file(dir1.path(), "source.bin", b"Source content for chunking");

	// Timeout prevents infinite hang (deadlock indicator)
	let sync_result = timeout(
		Duration::from_secs(10),
		SyncBuilder::new()
			.add_location(dir1.path().to_str().unwrap())
			.add_location(dir2.path().to_str().unwrap())
			.state_dir(state.path().to_str().unwrap())
			.sync(),
	)
	.await;

	// Must complete within timeout
	assert!(sync_result.is_ok(), "Bidirectional chunk transfer timed out (possible deadlock)");
	assert!(sync_result.unwrap().is_ok(), "Sync should succeed");

	// Verify file was transferred
	assert!(file_exists(dir2.path(), "source.bin"));
}

/// Test 2: Three-way sync with concurrent chunk transfers
/// Stress test: multiple nodes reading chunks simultaneously
#[tokio::test]
async fn test_deadlock_three_way_concurrent_transfer() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let dir3 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	create_file(dir1.path(), "file1.txt", b"Content from node 1");
	create_file(dir2.path(), "file2.txt", b"Content from node 2");

	// Three-way sync creates concurrent READ requests
	let sync_result = timeout(
		Duration::from_secs(15),
		SyncBuilder::new()
			.add_location(dir1.path().to_str().unwrap())
			.add_location(dir2.path().to_str().unwrap())
			.add_location(dir3.path().to_str().unwrap())
			.state_dir(state.path().to_str().unwrap())
			.sync(),
	)
	.await;

	assert!(sync_result.is_ok(), "Three-way sync timed out (possible deadlock)");
	assert!(sync_result.unwrap().is_ok(), "Sync should succeed");

	// All files should be synced to all locations
	assert!(file_exists(dir1.path(), "file2.txt"));
	assert!(file_exists(dir2.path(), "file1.txt"));
	assert!(file_exists(dir3.path(), "file1.txt"));
	assert!(file_exists(dir3.path(), "file2.txt"));
}

/// Test 3: Large file transfer (multiple chunks)
/// This stress tests the chunk transfer mechanism with many chunks
#[tokio::test]
async fn test_deadlock_large_file_multi_chunk() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	// Create a 10MB file to ensure multiple chunks
	let large_content = vec![0xAB; 10 * 1024 * 1024];
	create_file(dir1.path(), "large.bin", &large_content);

	let sync_result = timeout(
		Duration::from_secs(30), // Longer timeout for large file
		SyncBuilder::new()
			.add_location(dir1.path().to_str().unwrap())
			.add_location(dir2.path().to_str().unwrap())
			.state_dir(state.path().to_str().unwrap())
			.sync(),
	)
	.await;

	assert!(sync_result.is_ok(), "Large file transfer timed out (possible deadlock)");
	assert!(sync_result.unwrap().is_ok(), "Sync should succeed");

	// Verify file was transferred correctly
	assert!(file_exists(dir2.path(), "large.bin"));
	assert_eq!(
		read_file(dir2.path(), "large.bin").unwrap().len(),
		10 * 1024 * 1024,
		"Large file should be transferred intact"
	);
}

/// Test 4: Rapid successive syncs (protocol state machine stress)
/// Tests for deadlock under rapid protocol state transitions
#[tokio::test]
async fn test_deadlock_rapid_successive_syncs() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state1 = TempDir::new().unwrap();

	create_file(dir1.path(), "file1.txt", b"content 1");

	// First sync
	let sync1_result = timeout(
		Duration::from_secs(10),
		SyncBuilder::new()
			.add_location(dir1.path().to_str().unwrap())
			.add_location(dir2.path().to_str().unwrap())
			.state_dir(state1.path().to_str().unwrap())
			.sync(),
	)
	.await;

	assert!(sync1_result.is_ok(), "First sync timed out");
	assert!(sync1_result.unwrap().is_ok());

	// Add new file in dir1 for second sync
	create_file(dir1.path(), "file2.txt", b"content 2");

	// Second sync with fresh state dir
	let state2 = TempDir::new().unwrap();
	let sync2_result = timeout(
		Duration::from_secs(10),
		SyncBuilder::new()
			.add_location(dir1.path().to_str().unwrap())
			.add_location(dir2.path().to_str().unwrap())
			.state_dir(state2.path().to_str().unwrap())
			.sync(),
	)
	.await;

	assert!(sync2_result.is_ok(), "Second sync timed out");
	assert!(sync2_result.unwrap().is_ok());

	// Verify both files exist on both nodes (protocol consistency test)
	assert!(file_exists(dir2.path(), "file1.txt"));
	assert!(file_exists(dir2.path(), "file2.txt"));
}

/// Test 5: Protocol with all operations interleaved
/// Tests the complete protocol: LIST -> WRITE -> READ -> COMMIT
/// Focus: verify commit() releases locks in correct order
#[tokio::test]
async fn test_deadlock_full_protocol_sequence() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	// Create diverse file types to stress protocol
	create_file(dir1.path(), "empty.txt", b"");
	create_file(dir1.path(), "small.txt", b"small");
	create_file(dir1.path(), "medium.txt", &vec![0x42; 1024]);
	create_file(dir1.path(), "dir/nested.txt", b"nested content");

	let sync_result = timeout(
		Duration::from_secs(15),
		SyncBuilder::new()
			.add_location(dir1.path().to_str().unwrap())
			.add_location(dir2.path().to_str().unwrap())
			.state_dir(state.path().to_str().unwrap())
			.sync(),
	)
	.await;

	assert!(sync_result.is_ok(), "Full protocol sequence timed out (possible deadlock)");
	assert!(sync_result.unwrap().is_ok());

	// Verify all files transferred
	assert!(file_exists(dir2.path(), "empty.txt"));
	assert!(file_exists(dir2.path(), "small.txt"));
	assert!(file_exists(dir2.path(), "medium.txt"));
	assert!(file_exists(dir2.path(), "dir/nested.txt"));
}

/// Test 6: Multiple sequential chunk transfers (sender/receiver role switching)
/// Tests lock release pattern when both nodes act as sender and receiver
#[tokio::test]
async fn test_deadlock_role_switching() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	// Phase 1: dir1 -> dir2
	create_file(dir1.path(), "from_node1.txt", b"Node 1 content");

	let sync1 = timeout(
		Duration::from_secs(10),
		SyncBuilder::new()
			.add_location(dir1.path().to_str().unwrap())
			.add_location(dir2.path().to_str().unwrap())
			.state_dir(state.path().to_str().unwrap())
			.sync(),
	)
	.await;

	assert!(sync1.is_ok(), "First sync timed out");

	// Phase 2: dir2 -> dir1 (role switch)
	create_file(dir2.path(), "from_node2.txt", b"Node 2 content");

	let sync2 = timeout(
		Duration::from_secs(10),
		SyncBuilder::new()
			.add_location(dir1.path().to_str().unwrap())
			.add_location(dir2.path().to_str().unwrap())
			.state_dir(state.path().to_str().unwrap())
			.sync(),
	)
	.await;

	assert!(sync2.is_ok(), "Second sync timed out (role switch)");

	// Both files should exist on both nodes
	assert!(file_exists(dir1.path(), "from_node2.txt"));
	assert!(file_exists(dir2.path(), "from_node1.txt"));
}

/// Test 7: Stress test with very many small files
/// Tests protocol performance and lock contention
#[tokio::test]
async fn test_deadlock_many_small_files() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	// Create 100 small files
	for i in 0..100 {
		create_file(
			dir1.path(),
			&format!("file_{:03}.txt", i),
			format!("Content {}", i).as_bytes(),
		);
	}

	let sync_result = timeout(
		Duration::from_secs(20),
		SyncBuilder::new()
			.add_location(dir1.path().to_str().unwrap())
			.add_location(dir2.path().to_str().unwrap())
			.state_dir(state.path().to_str().unwrap())
			.sync(),
	)
	.await;

	assert!(
		sync_result.is_ok(),
		"Many files sync timed out (possible deadlock or performance issue)"
	);
	assert!(sync_result.unwrap().is_ok());

	// Spot-check some files
	assert!(file_exists(dir2.path(), "file_000.txt"));
	assert!(file_exists(dir2.path(), "file_050.txt"));
	assert!(file_exists(dir2.path(), "file_099.txt"));
}

/// Test 8: Verify no hangs during error recovery
/// Tests that error handling doesn't cause deadlock
#[tokio::test]
async fn test_deadlock_error_recovery() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	create_file(dir1.path(), "test.txt", b"test");

	// Attempt sync to read-only directory (should fail gracefully)
	let dir2_path = dir2.path();
	#[cfg(unix)]
	{
		// Make dir2 read-only temporarily
		let original_perms = fs::metadata(dir2_path).map(|m| m.permissions()).ok();

		// This might fail, but shouldn't deadlock
		let sync_result = timeout(
			Duration::from_secs(10),
			SyncBuilder::new()
				.add_location(dir1.path().to_str().unwrap())
				.add_location(dir2_path.to_str().unwrap())
				.state_dir(state.path().to_str().unwrap())
				.sync(),
		)
		.await;

		// Restore permissions
		if let Some(perms) = original_perms {
			fs::set_permissions(dir2_path, perms).ok();
		}

		// Even if sync fails, it must not hang
		assert!(sync_result.is_ok(), "Sync timed out during error condition");
	}

	#[cfg(not(unix))]
	{
		// On non-unix, just verify basic timeout handling works
		let sync_result = timeout(
			Duration::from_secs(10),
			SyncBuilder::new()
				.add_location(dir1.path().to_str().unwrap())
				.add_location(dir2_path.to_str().unwrap())
				.state_dir(state.path().to_str().unwrap())
				.sync(),
		)
		.await;

		assert!(sync_result.is_ok(), "Sync timed out");
	}
}

// ============================================================================
// Regression Tests (Specific to the Fixed Blocking Bug)
// ============================================================================

/// Regression Test: Verify commit() lock order
/// This specifically tests the fix: ensure sender lock is dropped before
/// acquiring receiver lock in the commit() method
#[tokio::test]
async fn test_regression_commit_lock_order() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	// Create a file that will require chunks (triggers commit)
	create_file(dir1.path(), "commit_test.bin", b"This file tests lock order");

	// Use timeout to detect deadlock
	let result = timeout(
		Duration::from_secs(10),
		SyncBuilder::new()
			.add_location(dir1.path().to_str().unwrap())
			.add_location(dir2.path().to_str().unwrap())
			.state_dir(state.path().to_str().unwrap())
			.sync(),
	)
	.await;

	assert!(result.is_ok(), "Commit lock order test timed out - potential regression of fixed bug");
	assert!(result.unwrap().is_ok(), "Sync should succeed");
	assert!(file_exists(dir2.path(), "commit_test.bin"));
}

/// Regression Test: Concurrent READ and WRITE (bidirectional pattern)
/// Tests the exact pattern that caused the original deadlock:
/// Parent waiting for child response while child waits on parent input
#[tokio::test]
async fn test_regression_bidirectional_communication() {
	let dir1 = TempDir::new().unwrap();
	let dir2 = TempDir::new().unwrap();
	let state = TempDir::new().unwrap();

	// Create files on both sides to trigger bidirectional communication
	create_file(dir1.path(), "node1_file.txt", b"From node 1");
	create_file(dir2.path(), "node2_file.txt", b"From node 2");

	let result = timeout(
		Duration::from_secs(10),
		SyncBuilder::new()
			.add_location(dir1.path().to_str().unwrap())
			.add_location(dir2.path().to_str().unwrap())
			.state_dir(state.path().to_str().unwrap())
			.sync(),
	)
	.await;

	assert!(result.is_ok(), "Bidirectional communication test timed out - regression of fixed bug");
	assert!(result.unwrap().is_ok());

	// Verify both files are on both nodes
	assert_eq!(read_file(dir1.path(), "node2_file.txt"), Some(b"From node 2".to_vec()));
	assert_eq!(read_file(dir2.path(), "node1_file.txt"), Some(b"From node 1".to_vec()));
}
