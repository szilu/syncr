//! Event types and handling for the TUI

use crossterm::event::{KeyEvent, MouseEvent};
use std::path::PathBuf;
use std::time::Duration;

use crate::types::{SyncPhase, SyncResult};

/// Progress statistics during sync
#[derive(Debug, Clone)]
pub struct ProgressStats {
	#[allow(dead_code)]
	pub phase: SyncPhase,
	pub files_processed: usize,
	pub files_total: usize,
	pub bytes_transferred: u64,
	#[allow(dead_code)]
	pub bytes_total: u64,
	pub transfer_rate: f64,
	#[allow(dead_code)]
	pub elapsed: Duration,
	#[allow(dead_code)]
	pub eta: Duration,
}

/// Events that drive the TUI
#[derive(Debug)]
#[allow(dead_code)]
pub enum TuiEvent {
	/// Keyboard input
	Key(KeyEvent),

	/// Mouse input
	Mouse(MouseEvent),

	/// Regular tick for animations and updates (typically 60 FPS)
	Tick,

	/// Events from the sync engine
	Sync(SyncEvent),

	/// Terminal resize
	Resize(u16, u16),
}

/// Events from the sync engine bridge
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum SyncEvent {
	/// Sync phase started
	PhaseStarted { phase: SyncPhase },

	/// Sync phase completed
	PhaseCompleted { phase: SyncPhase },

	/// Sync phase changed (legacy)
	PhaseChanged { phase: SyncPhase },

	/// Progress update with statistics
	Progress { stats: ProgressStats },

	/// Node successfully connected
	NodeConnected { index: usize, location: String },

	/// Node connection failed
	NodeConnectionFailed { index: usize, location: String, error: String },

	/// Node ready after handshake
	NodeReady { index: usize, location: String },

	/// Node disconnecting
	NodeDisconnecting { index: usize },

	/// Node statistics update (files scanned, bytes known)
	NodeStats { index: usize, files_known: usize, bytes_known: u64 },

	/// File discovered on a node during collection
	FileDiscovered { path: PathBuf, node_index: usize, exists: bool },

	/// File operation started
	FileOperationStarted { path: PathBuf, operation: String },

	/// File operation completed
	FileOperationCompleted {
		path: PathBuf,
		operation: String,
		size: u64,
		from_node: usize,
		to_node: usize,
	},

	/// File synchronized between nodes
	FileSync { path: PathBuf, from_node: usize, to_nodes: Vec<usize> },

	/// File deleted from a node
	FileDelete { path: PathBuf, node: usize },

	/// Directory created on a node
	DirCreate { path: PathBuf, node: usize },

	/// Conflict detected (needs resolution)
	ConflictDetected { path: PathBuf, description: String },

	/// Conflict resolved
	ConflictResolved { path: PathBuf, winner: Option<usize> },

	/// Non-fatal error occurred during sync
	Error { error: String },

	/// Tracing log message from parent process
	Log { level: crate::tui::state::LogLevel, message: String },

	/// Sync completed successfully
	Completed { result: SyncResult },

	/// Sync failed with error
	Failed { error: String },
}

/// Generates tick events at a fixed rate
pub struct TickGenerator {
	interval: Duration,
}

impl TickGenerator {
	/// Create a new tick generator with target FPS
	pub fn new(fps: u32) -> Self {
		let interval = Duration::from_millis(1000 / fps.max(1) as u64);
		TickGenerator { interval }
	}

	/// Wait for next tick
	pub async fn next_tick(&self) {
		tokio::time::sleep(self.interval).await;
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_tick_generator_creation() {
		let gen = TickGenerator::new(60);
		assert_eq!(gen.interval, Duration::from_millis(16)); // ~60 FPS
	}

	#[test]
	fn test_tick_generator_fps() {
		let gen = TickGenerator::new(30);
		assert_eq!(gen.interval, Duration::from_millis(33)); // ~30 FPS
	}
}

// vim: ts=4
