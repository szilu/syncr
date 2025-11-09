//! Progress display callback for CLI sync
//!
//! This module provides progress tracking and display for sync operations,
//! independent of conflict resolution.

pub mod constants;

use std::collections::HashMap;
use std::io::Write;
use std::sync::Mutex;
use std::time::Instant;
use tracing::info;

use crate::node_labels::generate_node_labels;
use crate::sync_impl::{SyncCallbackEvent, SyncProgressCallback};
use crate::types::SyncPhase;

/// Progress display constants
pub use constants::*;

/// Per-node collection statistics
#[derive(Debug, Clone)]
pub(crate) struct NodeCollectionStats {
	pub(crate) files: usize,
	pub(crate) bytes: u64,
}

/// Shared state for progress tracking
#[derive(Debug)]
pub struct ProgressState {
	pub current_phase: Mutex<Option<SyncPhase>>,
	pub last_update: Mutex<Instant>,
	// Per-node collection stats during collecting phase
	pub node_stats: Mutex<HashMap<usize, NodeCollectionStats>>,
}

impl ProgressState {
	/// Create a new progress state
	pub fn new() -> Self {
		Self {
			current_phase: Mutex::new(None),
			last_update: Mutex::new(Instant::now()),
			node_stats: Mutex::new(HashMap::new()),
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
	node_labels: Vec<String>,
}

impl CliProgressCallback {
	/// Create a new progress callback
	pub fn new() -> Self {
		Self { state: ProgressState::new(), node_labels: Vec::new() }
	}

	/// Create a progress callback with smart node labels
	pub fn with_addresses(addresses: Vec<&str>) -> Self {
		let labels = generate_node_labels(&addresses);
		Self { state: ProgressState::new(), node_labels: labels }
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
					*self.state.current_phase.lock().unwrap_or_else(|e| e.into_inner()) =
						Some(phase);
					// Reset collection stats for new phase
					if !matches!(phase, SyncPhase::Collecting) {
						*self.state.node_stats.lock().unwrap_or_else(|e| e.into_inner()) =
							HashMap::new();
					}
					// Log phase change
					let phase_name = format!("{:?}", phase);
					info!("→ {} phase...", phase_name);
				}
			}
			SyncCallbackEvent::NodeStats { node_id, files_known, bytes_known } => {
				// During collecting phase, show per-node stats as progress
				if let Ok(phase) = self.state.current_phase.lock() {
					if matches!(phase.as_ref(), Some(SyncPhase::Collecting)) {
						// Update per-node stats
						{
							let mut stats =
								self.state.node_stats.lock().unwrap_or_else(|e| e.into_inner());
							stats.insert(
								node_id,
								NodeCollectionStats { files: files_known, bytes: bytes_known },
							);
						} // Drop lock before throttle check

						// Throttle updates to every 100ms
						let mut last =
							self.state.last_update.lock().unwrap_or_else(|e| e.into_inner());
						let elapsed = last.elapsed().as_millis();
						if elapsed < 100 {
							return;
						}
						*last = Instant::now();
						drop(last);

						// Display per-node stats on one line
						let stats = self.state.node_stats.lock().unwrap_or_else(|e| e.into_inner());
						let mut node_strs = Vec::new();

						// Collect and sort nodes
						let mut nodes: Vec<_> = stats.iter().collect();
						nodes.sort_by_key(|&(node_id, _)| node_id);

						for (node_id, node_stat) in nodes {
							let label = if *node_id < self.node_labels.len() {
								self.node_labels[*node_id].clone()
							} else {
								format!("N{}", node_id)
							};
							node_strs.push(format!(
								"{}: {}f/{:.1}MB",
								label,
								node_stat.files,
								node_stat.bytes as f64 / BYTES_PER_MB
							));
						}

						let _ =
							write!(std::io::stderr(), "\r  Collecting: {}", node_strs.join(" | "));
						let _ = std::io::stderr().flush();
					}
				}
			}
			SyncCallbackEvent::Progress(update) => {
				// Throttle updates to every 100ms to avoid spamming
				let mut last = self.state.last_update.lock().unwrap_or_else(|e| e.into_inner());
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
					// Clamp ratio to [0, 1] to prevent overflow in progress bar
					let ratio_clamped = ratio.clamp(0.0, 1.0);
					let filled = (ratio_clamped * PROGRESS_BAR_WIDTH as f64) as usize;
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
