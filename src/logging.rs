//! Logging prelude module for convenient access to tracing macros.
//!
//! This module provides convenient re-exports of common tracing macros
//! to reduce verbosity and maintain consistency across the codebase.
//!
//! # Usage
//!
//! ```ignore
//! use crate::logging::*;
//!
//! info!("This is an info message");
//! warn!("This is a warning");
//! error!("An error occurred");
//! debug!("Debug information");
//! trace!("Detailed trace information");
//! ```

pub use tracing::{debug, error, info, warn};

/// Initialize the tracing subscriber with environment filter support.
///
/// By default, logs at INFO level and above are displayed. Control the log level
/// with the `RUST_LOG` environment variable:
///
/// ```bash
/// RUST_LOG=debug cargo run
/// RUST_LOG=syncr=trace cargo run
/// RUST_LOG=syncr::serve=debug,syncr::sync_impl=trace cargo run
/// ```
pub fn init_tracing() {
	tracing_subscriber::fmt()
		.with_env_filter(
			tracing_subscriber::EnvFilter::try_from_default_env()
				.unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
		)
		.with_writer(std::io::stderr)
		.init();
}
