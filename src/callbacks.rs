//! Callback traits for progress reporting and event handling

use crate::conflict::Conflict;
use crate::error::SyncError;
use crate::types::SyncPhase;
use std::path::Path;
use std::time::Duration;

// Type aliases to reduce complexity
type ProgressFn = dyn Fn(ProgressStats) + Send + Sync;
type ConflictFn = dyn Fn(&Conflict) -> Option<usize> + Send + Sync;
type ErrorFn = dyn Fn(&SyncError) + Send + Sync;
type FileSyncFn = dyn Fn(&Path, usize, Vec<usize>) + Send + Sync;
type FileDeleteFn = dyn Fn(&Path, usize) + Send + Sync;
type DirCreateFn = dyn Fn(&Path, usize) + Send + Sync;

/// Progress statistics during sync operations
#[derive(Debug, Clone)]
pub struct ProgressStats {
	/// Current sync phase
	pub phase: SyncPhase,

	/// Files processed so far
	pub files_processed: usize,

	/// Total files to process
	pub files_total: usize,

	/// Bytes transferred so far
	pub bytes_transferred: u64,

	/// Estimated total bytes to transfer
	pub bytes_total: u64,

	/// Current transfer rate in bytes per second
	pub transfer_rate: f64,

	/// Elapsed time since start
	pub elapsed: Duration,

	/// Estimated time remaining
	pub eta: Duration,
}

/// Callback for progress updates
pub trait ProgressCallback: Send + Sync {
	/// Called periodically with progress statistics
	fn on_progress(&self, stats: ProgressStats);
}

/// Default progress callback that does nothing
pub struct NoProgressCallback;

impl ProgressCallback for NoProgressCallback {
	fn on_progress(&self, _stats: ProgressStats) {}
}

/// Callback for conflict resolution
pub trait ConflictCallback: Send + Sync {
	/// Called when a conflict is detected
	/// Return Some(index) to choose a winner, None to abort
	fn on_conflict(&self, conflict: &Conflict) -> Option<usize>;
}

/// Default conflict callback that aborts on any conflict
pub struct FailOnConflictCallback;

impl ConflictCallback for FailOnConflictCallback {
	fn on_conflict(&self, _conflict: &Conflict) -> Option<usize> {
		None
	}
}

/// Callback for error events
pub trait ErrorCallback: Send + Sync {
	/// Called when a non-fatal error occurs
	fn on_error(&self, error: &SyncError);
}

/// Default error callback that ignores errors
pub struct NoErrorCallback;

impl ErrorCallback for NoErrorCallback {
	fn on_error(&self, _error: &SyncError) {}
}

/// Callback for file-level operations
pub trait FileCallback: Send + Sync {
	/// Called when a file is synchronized
	///
	/// # Arguments
	/// * `path` - Path of the file
	/// * `from_node` - Source node index
	/// * `to_nodes` - Destination node indices
	fn on_file_sync(&self, path: &Path, from_node: usize, to_nodes: Vec<usize>);

	/// Called when a file is deleted from a node
	fn on_file_delete(&self, path: &Path, node: usize);

	/// Called when a directory is created on a node
	fn on_dir_create(&self, path: &Path, node: usize);
}

/// Default file callback that does nothing
pub struct NoFileCallback;

impl FileCallback for NoFileCallback {
	fn on_file_sync(&self, _path: &Path, _from_node: usize, _to_nodes: Vec<usize>) {}
	fn on_file_delete(&self, _path: &Path, _node: usize) {}
	fn on_dir_create(&self, _path: &Path, _node: usize) {}
}

/// Combined callback handler for all events
pub trait SyncCallbacks: Send + Sync {
	/// Called with progress update
	fn on_progress(&self, _stats: ProgressStats) {}

	/// Called when a conflict needs resolution
	fn on_conflict(&self, _conflict: &Conflict) -> Option<usize> {
		None
	}

	/// Called on non-fatal errors
	fn on_error(&self, _error: &SyncError) {}

	/// Called when a file is synchronized
	fn on_file_sync(&self, _path: &Path, _from_node: usize, _to_nodes: Vec<usize>) {}

	/// Called when a file is deleted
	fn on_file_delete(&self, _path: &Path, _node: usize) {}

	/// Called when a directory is created
	fn on_dir_create(&self, _path: &Path, _node: usize) {}
}

/// Default callback implementation that does nothing
pub struct NoCallbacks;

impl SyncCallbacks for NoCallbacks {}

/// Builder for callbacks using function closures
pub struct CallbackBuilder {
	progress: Option<Box<ProgressFn>>,
	conflict: Option<Box<ConflictFn>>,
	error: Option<Box<ErrorFn>>,
	file_sync: Option<Box<FileSyncFn>>,
	file_delete: Option<Box<FileDeleteFn>>,
	dir_create: Option<Box<DirCreateFn>>,
}

impl CallbackBuilder {
	/// Create a new callback builder
	pub fn new() -> Self {
		CallbackBuilder {
			progress: None,
			conflict: None,
			error: None,
			file_sync: None,
			file_delete: None,
			dir_create: None,
		}
	}

	/// Set progress callback
	pub fn on_progress<F>(mut self, callback: F) -> Self
	where
		F: Fn(ProgressStats) + Send + Sync + 'static,
	{
		self.progress = Some(Box::new(callback));
		self
	}

	/// Set conflict callback
	pub fn on_conflict<F>(mut self, callback: F) -> Self
	where
		F: Fn(&Conflict) -> Option<usize> + Send + Sync + 'static,
	{
		self.conflict = Some(Box::new(callback));
		self
	}

	/// Set error callback
	pub fn on_error<F>(mut self, callback: F) -> Self
	where
		F: Fn(&SyncError) + Send + Sync + 'static,
	{
		self.error = Some(Box::new(callback));
		self
	}

	/// Set file sync callback
	pub fn on_file_sync<F>(mut self, callback: F) -> Self
	where
		F: Fn(&Path, usize, Vec<usize>) + Send + Sync + 'static,
	{
		self.file_sync = Some(Box::new(callback));
		self
	}

	/// Set file delete callback
	pub fn on_file_delete<F>(mut self, callback: F) -> Self
	where
		F: Fn(&Path, usize) + Send + Sync + 'static,
	{
		self.file_delete = Some(Box::new(callback));
		self
	}

	/// Set directory create callback
	pub fn on_dir_create<F>(mut self, callback: F) -> Self
	where
		F: Fn(&Path, usize) + Send + Sync + 'static,
	{
		self.dir_create = Some(Box::new(callback));
		self
	}

	/// Build the callbacks handler
	pub fn build(self) -> Box<dyn SyncCallbacks> {
		Box::new(CompositeCallbacks {
			progress: self.progress,
			conflict: self.conflict,
			error: self.error,
			file_sync: self.file_sync,
			file_delete: self.file_delete,
			dir_create: self.dir_create,
		})
	}
}

impl Default for CallbackBuilder {
	fn default() -> Self {
		Self::new()
	}
}

/// Internal composite callbacks implementation
struct CompositeCallbacks {
	progress: Option<Box<ProgressFn>>,
	conflict: Option<Box<ConflictFn>>,
	error: Option<Box<ErrorFn>>,
	file_sync: Option<Box<FileSyncFn>>,
	file_delete: Option<Box<FileDeleteFn>>,
	dir_create: Option<Box<DirCreateFn>>,
}

impl SyncCallbacks for CompositeCallbacks {
	fn on_progress(&self, stats: ProgressStats) {
		if let Some(ref callback) = self.progress {
			callback(stats);
		}
	}

	fn on_conflict(&self, conflict: &Conflict) -> Option<usize> {
		if let Some(ref callback) = self.conflict {
			callback(conflict)
		} else {
			None
		}
	}

	fn on_error(&self, error: &SyncError) {
		if let Some(ref callback) = self.error {
			callback(error);
		}
	}

	fn on_file_sync(&self, path: &Path, from_node: usize, to_nodes: Vec<usize>) {
		if let Some(ref callback) = self.file_sync {
			callback(path, from_node, to_nodes);
		}
	}

	fn on_file_delete(&self, path: &Path, node: usize) {
		if let Some(ref callback) = self.file_delete {
			callback(path, node);
		}
	}

	fn on_dir_create(&self, path: &Path, node: usize) {
		if let Some(ref callback) = self.dir_create {
			callback(path, node);
		}
	}
}
