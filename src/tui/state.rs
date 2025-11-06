//! Application state management

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

use super::event::ProgressStats;
use crate::types::{Config, SyncPhase, SyncResult};

/// Main application state
pub struct AppState {
	/// Current view being displayed
	pub current_view: ViewType,

	/// Sync-related state
	pub sync: SyncState,

	/// UI-specific state (ephemeral)
	pub ui: UiState,

	/// Configuration
	#[allow(dead_code)]
	pub config: Config,

	/// Sync locations/directories
	pub locations: Vec<String>,

	/// Log entries (ring buffer style)
	pub logs: VecDeque<LogEntry>,

	/// Should the application quit?
	pub should_quit: bool,
}

/// Available view types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewType {
	Setup,
	Sync, // Main sync view with tabs
	Help,
}

/// Tab types within the sync view
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabType {
	Nodes,     // Node statistics
	Files,     // All files with status
	Conflicts, // Conflicts that need resolution
	Logs,      // Log messages
}

/// Sync operation state tracking
pub struct SyncState {
	/// Current sync phase
	pub phase: Option<SyncPhase>,

	/// Progress statistics
	pub progress: Option<ProgressStats>,

	/// Per-node information
	pub nodes: Vec<NodeInfo>,

	/// Current file operations in progress
	pub active_operations: Vec<FileOperation>,

	/// All files being tracked during sync
	pub files: Vec<FileEntry>,

	/// Conflicts that need resolution (path, description, resolution_choice)
	pub conflicts: Vec<ConflictEntry>,

	/// Index of currently selected conflict (for keyboard navigation)
	pub selected_conflict_index: Option<usize>,

	/// Currently active tab in sync view
	pub active_tab: TabType,

	/// Scroll position per tab
	pub scroll_positions: TabScrollPositions,

	/// Final result (if completed)
	pub result: Option<SyncResult>,

	/// When the sync started
	#[allow(dead_code)]
	pub start_time: Option<Instant>,

	/// Is sync currently running?
	pub is_running: bool,

	/// Channel for sending conflict resolutions to the sync engine
	pub conflict_resolution_tx:
		Option<std::sync::mpsc::Sender<crate::sync_impl::ConflictResolution>>,
}

/// Tracks scroll position for each tab
#[derive(Debug, Clone)]
pub struct TabScrollPositions {
	pub nodes: usize,
	pub files: usize,
	pub conflicts: usize,
	pub logs: usize,
}

/// Status of a file on a specific node
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileNodeStatus {
	Unknown,
	Exists,
	Missing,
	Pending, // Will be synced
	#[allow(dead_code)]
	Syncing, // Currently syncing
	Synced,  // Successfully synced
	#[allow(dead_code)]
	Failed, // Sync failed
}

/// A file being tracked during sync
#[derive(Debug, Clone)]
pub struct FileEntry {
	pub path: PathBuf,
	/// Status on each node (index matches node index)
	pub node_status: Vec<FileNodeStatus>,
	/// Is this file part of a conflict?
	pub is_conflict: bool,
}

/// A conflict that needs resolution
#[derive(Debug, Clone)]
pub struct ConflictEntry {
	pub path: PathBuf,
	/// Which node's version was chosen (None = not resolved yet)
	pub resolution: Option<usize>,
}

/// Information about a single sync node
#[derive(Debug, Clone)]
pub struct NodeInfo {
	#[allow(dead_code)]
	pub index: usize,
	pub location: String,
	pub connected: bool,
	pub error: Option<String>,

	// Statistics
	pub files_collected: usize, // Files cataloged during collection
	pub bytes_collected: u64,   // Bytes cataloged during collection
	pub files_sent: usize,      // Files sent during sync
	pub bytes_sent: u64,        // Bytes sent during sync
	pub files_received: usize,  // Files received during sync
	pub bytes_received: u64,    // Bytes received during sync
}

/// Tracks a file operation in progress
#[derive(Debug, Clone)]
pub struct FileOperation {
	pub path: PathBuf,
	#[allow(dead_code)]
	pub from_node: usize,
	#[allow(dead_code)]
	pub to_node: usize,
	#[allow(dead_code)]
	pub progress: f32, // 0.0 to 1.0
	#[allow(dead_code)]
	pub bytes_total: u64,
	#[allow(dead_code)]
	pub bytes_done: u64,
}

/// Ephemeral UI state (reset when views change)
pub struct UiState {
	/// Scroll offset for scrollable areas
	pub scroll_offset: usize,

	/// Selected item index
	pub selected_index: usize,

	/// Which area has focus
	pub focus: FocusArea,

	/// Input buffer for text input fields
	pub input_buffer: String,

	/// Cursor position in input buffer
	pub cursor_pos: usize,

	/// Animation frame counter for UI animations (increments each render)
	pub animation_frame: u32,
}

/// Indicates which area of UI has focus
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum FocusArea {
	Main,
	Sidebar,
	Input,
}

/// A log entry with timestamp and level
#[derive(Debug, Clone)]
pub struct LogEntry {
	#[allow(dead_code)]
	pub timestamp: Instant,
	#[allow(dead_code)]
	pub level: LogLevel,
	#[allow(dead_code)]
	pub message: String,
}

/// Log severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
	Trace,
	Debug,
	Info,
	Warning,
	Error,
	Success,
}

impl AppState {
	/// Create a new application state
	pub fn new(config: Config, locations: Vec<String>) -> Self {
		let node_count = locations.len();

		AppState {
			current_view: ViewType::Setup,
			sync: SyncState {
				phase: None,
				progress: None,
				nodes: (0..node_count)
					.map(|i| NodeInfo {
						index: i,
						location: locations[i].clone(),
						connected: false,
						error: None,
						files_collected: 0,
						bytes_collected: 0,
						files_sent: 0,
						bytes_sent: 0,
						files_received: 0,
						bytes_received: 0,
					})
					.collect(),
				active_operations: Vec::new(),
				files: Vec::new(),
				conflicts: Vec::new(),
				selected_conflict_index: None,
				active_tab: TabType::Nodes,
				scroll_positions: TabScrollPositions { nodes: 0, files: 0, conflicts: 0, logs: 0 },
				result: None,
				start_time: None,
				is_running: false,
				conflict_resolution_tx: None,
			},
			ui: UiState {
				scroll_offset: 0,
				selected_index: 0,
				focus: FocusArea::Main,
				input_buffer: String::new(),
				cursor_pos: 0,
				animation_frame: 0,
			},
			config,
			locations,
			logs: VecDeque::with_capacity(1000),
			should_quit: false,
		}
	}

	/// Add a log entry
	pub fn add_log(&mut self, level: LogLevel, message: String) {
		self.logs.push_back(LogEntry { timestamp: Instant::now(), level, message });

		// Keep only last 1000 entries
		while self.logs.len() > 1000 {
			self.logs.pop_front();
		}
	}

	/// Track a file and update its status on a specific node
	pub fn track_file(
		&mut self,
		path: PathBuf,
		node_index: usize,
		status: FileNodeStatus,
		is_conflict: bool,
	) {
		let node_count = self.sync.nodes.len();

		// Find or create the file entry
		if let Some(file) = self.sync.files.iter_mut().find(|f| f.path == path) {
			// Update existing entry
			if node_index < file.node_status.len() {
				file.node_status[node_index] = status;
			}
			if is_conflict {
				file.is_conflict = true;
			}
		} else {
			// Create new entry with Unknown status for all nodes
			let mut node_status = vec![FileNodeStatus::Unknown; node_count];
			if node_index < node_status.len() {
				node_status[node_index] = status;
			}

			self.sync.files.push(FileEntry { path, node_status, is_conflict });
		}
	}

	/// Change to a different view, resetting UI state for new view
	pub fn change_view(&mut self, view: ViewType) {
		self.current_view = view;
		self.reset_ui_state();
	}

	/// Reset ephemeral UI state
	fn reset_ui_state(&mut self) {
		self.ui.scroll_offset = 0;
		self.ui.selected_index = 0;
		self.ui.focus = FocusArea::Main;
		self.ui.input_buffer.clear();
		self.ui.cursor_pos = 0;
	}

	/// Get the elapsed time since sync started
	#[allow(dead_code)]
	pub fn elapsed(&self) -> Option<std::time::Duration> {
		self.sync.start_time.map(|start| start.elapsed())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_state_creation() {
		let config = Config {
			syncr_dir: std::path::PathBuf::from("/tmp/.syncr"),
			profile: "test".to_string(),
		};
		let locations = vec!["./dir1".to_string(), "./dir2".to_string()];
		let state = AppState::new(config, locations);

		assert_eq!(state.current_view, ViewType::Setup);
		assert_eq!(state.sync.nodes.len(), 2);
		assert!(!state.should_quit);
	}

	#[test]
	fn test_add_log() {
		let config = Config {
			syncr_dir: std::path::PathBuf::from("/tmp/.syncr"),
			profile: "test".to_string(),
		};
		let mut state = AppState::new(config, vec![]);

		state.add_log(LogLevel::Info, "Test message".to_string());
		assert_eq!(state.logs.len(), 1);
		assert_eq!(state.logs[0].level, LogLevel::Info);
	}

	#[test]
	fn test_change_view() {
		let config = Config {
			syncr_dir: std::path::PathBuf::from("/tmp/.syncr"),
			profile: "test".to_string(),
		};
		let mut state = AppState::new(config, vec![]);
		state.change_view(ViewType::Sync);

		assert_eq!(state.current_view, ViewType::Sync);
		assert_eq!(state.ui.scroll_offset, 0);
	}
}

// vim: ts=4
