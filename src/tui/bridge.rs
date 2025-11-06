//! Bridge between sync engine and TUI
//!
//! Translates sync events to TUI events sent over a broadcast channel.

use super::event::{ProgressStats, SyncEvent};
use tokio::sync::broadcast;

/// Bridge that sends sync progress events to the TUI
pub struct TuiBridge {
	event_tx: broadcast::Sender<SyncEvent>,
}

impl TuiBridge {
	/// Create a new TUI bridge with an event sender
	pub fn new(event_tx: broadcast::Sender<SyncEvent>) -> Self {
		TuiBridge { event_tx }
	}

	/// Send an event to the TUI (ignores errors if no receivers)
	fn send_event(&self, event: SyncEvent) {
		// Ignore send errors - means no receivers listening (which is ok)
		let _ = self.event_tx.send(event);
	}
}

impl crate::sync_impl::SyncProgressCallback for TuiBridge {
	fn on_event(&self, event: crate::sync_impl::SyncCallbackEvent) {
		use crate::sync_impl::SyncCallbackEvent;

		match event {
			SyncCallbackEvent::PhaseChanged { phase, is_starting } => {
				if is_starting {
					self.send_event(SyncEvent::PhaseStarted { phase });
				} else {
					self.send_event(SyncEvent::PhaseCompleted { phase });
				}
			}

			SyncCallbackEvent::Progress(update) => {
				self.send_event(SyncEvent::Progress {
					stats: ProgressStats {
						phase: update.phase,
						files_processed: update.files_processed,
						files_total: update.files_total,
						bytes_transferred: update.bytes_transferred,
						bytes_total: update.bytes_total,
						transfer_rate: update.transfer_rate,
						elapsed: std::time::Duration::ZERO,
						eta: std::time::Duration::ZERO,
					},
				});
			}

			SyncCallbackEvent::NodeConnecting { node_id, location } => {
				self.send_event(SyncEvent::NodeConnected { index: node_id, location });
			}

			SyncCallbackEvent::NodeReady { node_id, location } => {
				self.send_event(SyncEvent::NodeReady { index: node_id, location });
			}

			SyncCallbackEvent::NodeDisconnecting { node_id } => {
				self.send_event(SyncEvent::NodeDisconnecting { index: node_id });
			}

			SyncCallbackEvent::NodeStats { node_id, files_known, bytes_known } => {
				self.send_event(SyncEvent::NodeStats { index: node_id, files_known, bytes_known });
			}

			SyncCallbackEvent::FileDiscovered { path, node_id, exists } => {
				self.send_event(SyncEvent::FileDiscovered {
					path: std::path::PathBuf::from(path),
					node_index: node_id,
					exists,
				});
			}

			SyncCallbackEvent::FileOperation {
				path,
				operation,
				is_starting,
				file_size,
				from_node,
				to_node,
			} => {
				if is_starting {
					self.send_event(SyncEvent::FileOperationStarted {
						path: std::path::PathBuf::from(path),
						operation: operation.to_string(),
					});
				} else {
					self.send_event(SyncEvent::FileOperationCompleted {
						path: std::path::PathBuf::from(path),
						operation: operation.to_string(),
						size: file_size,
						from_node,
						to_node,
					});
				}
			}

			SyncCallbackEvent::Conflict { path, is_detected, num_versions, winner } => {
				if is_detected {
					self.send_event(SyncEvent::ConflictDetected {
						path: std::path::PathBuf::from(path),
						description: format!("{} versions detected", num_versions),
					});
				} else {
					self.send_event(SyncEvent::ConflictResolved {
						path: std::path::PathBuf::from(path),
						winner,
					});
				}
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_bridge_creation() {
		let (tx, _rx) = broadcast::channel(10);
		let _bridge = TuiBridge::new(tx);
		// Just verify it constructs without panicking
	}
}

// vim: ts=4
