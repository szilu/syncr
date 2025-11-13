//! Signal handlers for graceful termination
//!
//! NOTE: Signal handlers are spawned but graceful shutdown is handled through
//! normal application flows. Do NOT call std::process::exit() directly as it
//! bypasses Drop implementations and terminal cleanup code (TerminalGuard, etc).

use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{debug, warn};

/// Shared shutdown flag for signal handlers
pub static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Setup signal handlers for graceful cleanup on termination
/// With path-level locking in the cache DB, locks are automatically
/// released when the process exits.
///
/// NOTE: This sets a shutdown flag but does NOT call exit(). The main application
/// loop must check this flag and perform cleanup before exiting.
#[allow(dead_code)]
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
				debug!("Received SIGTERM, requesting graceful shutdown...");
				SHUTDOWN_REQUESTED.store(true, Ordering::Relaxed);
			}
			_ = sigint.recv() => {
				debug!("Received SIGINT, requesting graceful shutdown...");
				SHUTDOWN_REQUESTED.store(true, Ordering::Relaxed);
			}
		}
	});
}

// vim: ts=4
