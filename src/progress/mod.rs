//! Progress display callback for CLI sync
//!
//! This module provides progress tracking and display for sync operations,
//! independent of conflict resolution.

pub mod constants;

use std::io::Write;
use std::sync::Mutex;
use std::time::Instant;

use crate::sync_impl::{SyncCallbackEvent, SyncProgressCallback};
use crate::types::SyncPhase;

/// Progress display constants
pub use constants::*;

/// Shared state for progress tracking
#[derive(Debug)]
pub struct ProgressState {
	pub current_phase: Mutex<Option<SyncPhase>>,
	pub last_update: Mutex<Instant>,
	pub collection_files: Mutex<usize>,
	pub collection_bytes: Mutex<u64>,
}

impl ProgressState {
	/// Create a new progress state
	pub fn new() -> Self {
		Self {
			current_phase: Mutex::new(None),
			last_update: Mutex::new(Instant::now()),
			collection_files: Mutex::new(0),
			collection_bytes: Mutex::new(0),
		}
	}
}

impl Default for ProgressState {
	fn default() -> Self {
		Self::new()
	}
}

/// CLI progress callback - displays sync progress without conflict handling
pub struct CliProgressCallback {
	state: ProgressState,
}

impl CliProgressCallback {
	/// Create a new progress callback
	pub fn new() -> Self {
		Self { state: ProgressState::new() }
	}
}

impl Default for CliProgressCallback {
	fn default() -> Self {
		Self::new()
	}
}

impl SyncProgressCallback for CliProgressCallback {
	fn on_event(&self, event: SyncCallbackEvent) {
		match event {
			SyncCallbackEvent::PhaseChanged { phase, is_starting } => {
				if is_starting {
					*self.state.current_phase.lock().unwrap() = Some(phase);
					// Reset collection stats for new phase
					if !matches!(phase, SyncPhase::Collecting) {
						*self.state.collection_files.lock().unwrap() = 0;
						*self.state.collection_bytes.lock().unwrap() = 0;
					}
					// Start new line for new phase
					let _ = writeln!(std::io::stderr());
					let phase_name = format!("{:?}", phase);
					let _ = writeln!(std::io::stderr(), "→ {} phase...", phase_name);
					let _ = std::io::stderr().flush();
				}
			}
			SyncCallbackEvent::NodeStats { node_id: _, files_known, bytes_known } => {
				// During collecting phase, show node stats as progress
				if let Ok(phase) = self.state.current_phase.lock() {
					if matches!(phase.as_ref(), Some(SyncPhase::Collecting)) {
						// Update accumulated stats (take the max from all nodes)
						let mut files = self.state.collection_files.lock().unwrap();
						let mut bytes = self.state.collection_bytes.lock().unwrap();
						*files = (*files).max(files_known);
						*bytes = (*bytes).max(bytes_known);
						drop(files); // Unlock before throttle check
						drop(bytes);

						// Throttle updates to every 100ms
						let mut last = self.state.last_update.lock().unwrap();
						let elapsed = last.elapsed().as_millis();
						if elapsed < 100 {
							return;
						}
						*last = Instant::now();
						drop(last);

						// Re-lock to read current values
						let files = self.state.collection_files.lock().unwrap();
						let bytes = self.state.collection_bytes.lock().unwrap();
						let _ = write!(
							std::io::stderr(),
							"\r  Collecting: {} files | {:.1} MB",
							*files,
							*bytes as f64 / BYTES_PER_MB
						);
						let _ = std::io::stderr().flush();
					}
				}
			}
			SyncCallbackEvent::Progress(update) => {
				// Throttle updates to every 100ms to avoid spamming
				let mut last = self.state.last_update.lock().unwrap();
				let elapsed = last.elapsed().as_millis();
				if elapsed < 100 {
					return;
				}
				*last = Instant::now();

				if matches!(update.phase, SyncPhase::TransferringChunks) {
					// Use byte-based progress instead of chunk count
					let ratio = if update.bytes_total > 0 {
						update.bytes_transferred as f64 / update.bytes_total as f64
					} else {
						0.0
					};
					let filled = (ratio * PROGRESS_BAR_WIDTH as f64) as usize;
					let bar = format!(
						"[{}{}]",
						"=".repeat(filled),
						" ".repeat(PROGRESS_BAR_WIDTH - filled)
					);
					let _ = write!(
						std::io::stderr(),
						"\r  Transferring: {} {:.1}/{:.1} MB ({}/{}  chunks) | {:.1} MB/s",
						bar,
						update.bytes_transferred as f64 / BYTES_PER_MB,
						update.bytes_total as f64 / BYTES_PER_MB,
						update.files_processed,
						update.files_total,
						update.transfer_rate
					);
					let _ = std::io::stderr().flush();
				}
			}
			_ => {}
		}
	}
}

// vim: ts=4
