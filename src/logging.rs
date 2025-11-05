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

pub use tracing::{debug, error, info, trace, warn};

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

/// Initialize tracing subscriber that propagates messages via protocol
/// Used by child processes (serve.rs) to send all logs through stdout
/// Messages are formatted as: #<LEVEL>:<message> or !E:<message> for errors
pub fn init_protocol_propagation() {
	use tracing_subscriber::fmt::format::FormatFields;

	struct ProtocolFormatter;

	impl<S, N> tracing_subscriber::fmt::FormatEvent<S, N> for ProtocolFormatter
	where
		S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
		N: for<'a> FormatFields<'a> + 'static,
	{
		fn format_event(
			&self,
			_ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
			mut writer: tracing_subscriber::fmt::format::Writer<'_>,
			event: &tracing::Event<'_>,
		) -> std::fmt::Result {
			let metadata = event.metadata();
			let level_char = match *metadata.level() {
				tracing::Level::ERROR => 'E',
				tracing::Level::WARN => 'W',
				tracing::Level::INFO => 'I',
				tracing::Level::DEBUG => 'D',
				tracing::Level::TRACE => 'T',
			};

			let prefix = match *metadata.level() {
				tracing::Level::ERROR => '!',
				_ => '#',
			};

			write!(&mut writer, "{}{}:", prefix, level_char)?;

			// Format the message field directly with a newline
			event.record(&mut |field: &tracing::field::Field, value: &dyn std::fmt::Debug| {
				if field.name() == "message" {
					// Output the debug-formatted value with a newline
					let _ = writeln!(writer, "{:?}", value);
				}
			});

			Ok(())
		}
	}

	tracing_subscriber::fmt()
		.with_ansi(false)
		.with_target(false)
		.with_thread_ids(false)
		.with_thread_names(false)
		.with_level(false)
		.with_writer(std::io::stdout)
		.event_format(ProtocolFormatter)
		.with_env_filter(
			tracing_subscriber::EnvFilter::try_from_default_env()
				.unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
		)
		.init();
}
