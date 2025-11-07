//! Signal handlers for graceful termination

use tracing::debug;

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
				eprintln!("Failed to setup SIGTERM handler: {}", e);
				return;
			}
		};

		let mut sigint = match signal::unix::signal(signal::unix::SignalKind::interrupt()) {
			Ok(stream) => stream,
			Err(e) => {
				eprintln!("Failed to setup SIGINT handler: {}", e);
				return;
			}
		};

		tokio::select! {
			_ = sigterm.recv() => {
				debug!("Received SIGTERM, exiting gracefully...");
				std::process::exit(130); // SIGTERM exit code
			}
			_ = sigint.recv() => {
				debug!("Received SIGINT, exiting gracefully...");
				std::process::exit(130); // SIGINT exit code
			}
		}
	});
}

// vim: ts=4
