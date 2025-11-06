//! File locking mechanism for sync state management

use std::error::Error;
use std::path;
use std::sync::OnceLock;
use tracing::{debug, info, warn};

/// Global lock file path - used for signal handler cleanup
static LOCK_FILE_PATH: OnceLock<std::sync::Mutex<Option<path::PathBuf>>> = OnceLock::new();

/// File locking mechanism to prevent concurrent sync operations
/// Automatically cleaned up on drop or on signal termination
pub struct FileLock {
	path: path::PathBuf,
}

impl FileLock {
	/// Acquire an exclusive lock on the sync state directory
	pub fn acquire(syncr_dir: &path::Path) -> Result<Self, Box<dyn Error>> {
		let lock_path = syncr_dir.join(".SyNcR-LOCK");

		// Check if lock file already exists
		if lock_path.exists() {
			let _pid_str = std::fs::read_to_string(&lock_path)?;
			return Err(format!(
				"Sync already in progress (lock file exists at {}). \
                 If this is stale, delete the lock file manually.",
				lock_path.display()
			)
			.into());
		}

		// Create lock file with our PID
		let pid = std::process::id();
		std::fs::write(&lock_path, pid.to_string())?;

		// Register global lock path for signal handlers
		let lock_storage_mutex = LOCK_FILE_PATH.get_or_init(|| std::sync::Mutex::new(None));
		if let Ok(mut lock_storage) = lock_storage_mutex.lock() {
			*lock_storage = Some(lock_path.clone());
		}

		Ok(FileLock { path: lock_path })
	}

	/// Remove the lock file immediately (used by signal handlers)
	fn remove_now(&self) {
		let _ = std::fs::remove_file(&self.path);
		// Clear the global lock path
		if let Ok(mut lock_storage) = LOCK_FILE_PATH.get().unwrap().lock() {
			*lock_storage = None;
		}
	}
}

impl Drop for FileLock {
	fn drop(&mut self) {
		// Remove lock file on drop (whether success or failure)
		self.remove_now();
	}
}

/// Setup signal handlers for graceful cleanup on termination
/// This ensures the lock file is removed even if the process receives SIGTERM or SIGINT
pub fn setup_signal_handlers() {
	// Spawn a task to handle signals
	tokio::spawn(async {
		use tokio::signal;

		let mut sigterm = match signal::unix::signal(signal::unix::SignalKind::terminate()) {
			Ok(stream) => stream,
			Err(e) => {
				warn!("Failed to setup SIGTERM handler: {}", e);
				return;
			}
		};

		let mut sigint = match signal::unix::signal(signal::unix::SignalKind::interrupt()) {
			Ok(stream) => stream,
			Err(e) => {
				warn!("Failed to setup SIGINT handler: {}", e);
				return;
			}
		};

		tokio::select! {
			_ = sigterm.recv() => {
				debug!("Received SIGTERM, cleaning up lock file...");
				cleanup_lock_file();
				std::process::exit(130); // SIGTERM exit code
			}
			_ = sigint.recv() => {
				debug!("Received SIGINT, cleaning up lock file...");
				cleanup_lock_file();
				std::process::exit(130); // SIGINT exit code
			}
		}
	});
}

/// Clean up the lock file if it exists
fn cleanup_lock_file() {
	if let Some(lock_storage_mutex) = LOCK_FILE_PATH.get() {
		if let Ok(lock_storage) = lock_storage_mutex.lock() {
			if let Some(lock_path) = lock_storage.as_ref() {
				let _ = std::fs::remove_file(lock_path);
				info!("Lock file cleaned up on signal termination: {}", lock_path.display());
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs;
	use tempfile::TempDir;

	#[test]
	fn test_lock_creation() {
		let temp_dir = TempDir::new().unwrap();
		let lock = FileLock::acquire(temp_dir.path()).unwrap();

		// Lock file should exist
		assert!(lock.path.exists());

		// File should contain the process ID
		let content = fs::read_to_string(&lock.path).unwrap();
		assert_eq!(content, std::process::id().to_string());
	}

	#[test]
	fn test_lock_cleanup_on_drop() {
		let temp_dir = TempDir::new().unwrap();
		let lock_path = {
			let lock = FileLock::acquire(temp_dir.path()).unwrap();
			let path = lock.path.clone();
			assert!(path.exists());
			path
		};

		// Lock file should be removed after drop
		assert!(!lock_path.exists());
	}

	#[test]
	fn test_lock_prevents_concurrent_access() {
		let temp_dir = TempDir::new().unwrap();
		let _lock1 = FileLock::acquire(temp_dir.path()).unwrap();

		// Second attempt should fail
		let result = FileLock::acquire(temp_dir.path());
		assert!(result.is_err());
		if let Err(e) = result {
			assert!(e.to_string().contains("Sync already in progress"));
		}
	}
}

// vim: ts=4
