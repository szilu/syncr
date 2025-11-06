//! State management and persistence for sync operations

use crate::error::StateError;
use crate::types::FileData;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Previous sync state for three-way merge detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviousSyncState {
	/// Files from previous sync, keyed by path
	pub files: BTreeMap<String, FileData>,

	/// Timestamp of previous sync
	pub timestamp: u64,
}

/// Persistent state manager for sync profiles
pub struct StateManager {
	state_dir: PathBuf,
	profile: String,
}

impl StateManager {
	/// Create a new state manager
	#[allow(dead_code)]
	pub fn new(state_dir: PathBuf, profile: &str) -> Self {
		StateManager { state_dir, profile: profile.to_string() }
	}

	/// Load previous sync state if it exists
	pub async fn load(&self) -> Result<Option<PreviousSyncState>, StateError> {
		let path = self.state_path();

		if !path.exists() {
			return Ok(None);
		}

		let contents = tokio::fs::read_to_string(&path)
			.await
			.map_err(|e| StateError::LoadFailed { source: Box::new(e) })?;

		serde_json::from_str(&contents).map_err(|e| StateError::Corrupted {
			message: format!("Failed to parse state JSON: {}", e),
		})
	}

	/// Save current sync state
	pub async fn save(&self, state: &PreviousSyncState) -> Result<(), StateError> {
		let path = self.state_path();

		// Ensure directory exists
		if !path.parent().unwrap_or(Path::new(".")).exists() {
			tokio::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))
				.await
				.map_err(|e| StateError::SaveFailed { source: Box::new(e) })?;
		}

		let json = serde_json::to_string(state)
			.map_err(|e| StateError::SaveFailed { source: Box::new(e) })?;

		tokio::fs::write(&path, json)
			.await
			.map_err(|e| StateError::SaveFailed { source: Box::new(e) })
	}

	/// Delete saved state
	pub async fn clear(&self) -> Result<(), StateError> {
		let path = self.state_path();

		if path.exists() {
			tokio::fs::remove_file(&path)
				.await
				.map_err(|e| StateError::SaveFailed { source: Box::new(e) })?;
		}

		Ok(())
	}

	/// Get state file path
	pub fn state_path(&self) -> PathBuf {
		self.state_dir.join(format!("{}.json", self.profile))
	}

	/// Acquire an exclusive lock on the state
	pub async fn lock(&self) -> Result<StateLock, StateError> {
		let lock_path = self.state_dir.join(".SyNcR-LOCK");

		// Check if lock file already exists
		if lock_path.exists() {
			return Err(StateError::LockFailed {
				message: format!(
					"Sync already in progress (lock file exists). If stale, delete: {}",
					lock_path.display()
				),
			});
		}

		// Create lock file with our PID
		let pid = std::process::id();
		tokio::fs::write(&lock_path, pid.to_string()).await.map_err(|e| {
			StateError::LockFailed { message: format!("Failed to create lock file: {}", e) }
		})?;

		Ok(StateLock { path: lock_path })
	}
}

/// RAII lock guard for exclusive sync access
pub struct StateLock {
	path: PathBuf,
}

impl Drop for StateLock {
	fn drop(&mut self) {
		// Remove lock file on drop (whether success or failure)
		let _ = std::fs::remove_file(&self.path);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn test_state_manager_creation() {
		let manager = StateManager::new(PathBuf::from("/tmp"), "test");
		assert_eq!(manager.profile, "test");
	}

	#[tokio::test]
	async fn test_state_path() {
		let manager = StateManager::new(PathBuf::from("/tmp"), "myprofile");
		let path = manager.state_path();
		assert!(path.to_string_lossy().ends_with("myprofile.json"));
	}
}

// vim: ts=4
