//! Signal handlers for graceful termination

use tracing::{debug, warn};

/// Setup signal handlers for graceful cleanup on termination
/// With path-level locking in the cache DB, locks are automatically
/// released when the process exits.
pub fn setup_signal_handlers() {
	// Spawn a task to handle signals
	tokio::spawn(async {
		use tokio::signal;

		let mut sigterm = match signal::unix::signal(signal::unix::SignalKind::terminate()) {
			Ok(stream) => stream,
			Err(e) => {
				warn!("Failed to setup SIGTERM handler: {}. Process will not handle SIGTERM gracefully.", e);
				return;
			}
		};

		let mut sigint = match signal::unix::signal(signal::unix::SignalKind::interrupt()) {
			Ok(stream) => stream,
			Err(e) => {
				warn!("Failed to setup SIGINT handler: {}. Process will not handle SIGINT gracefully.", e);
				return;
			}
		};

		tokio::select! {
			_ = sigterm.recv() => {
				debug!("Received SIGTERM, exiting gracefully...");
				std::process::exit(143); // 128 + SIGTERM(15)
			}
			_ = sigint.recv() => {
				debug!("Received SIGINT, exiting gracefully...");
				std::process::exit(130); // 128 + SIGINT(2)
			}
		}
	});
}

// vim: ts=4
