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

pub use tracing::{debug, info, warn};

#[cfg(feature = "tui")]
use tokio::sync::broadcast;

/// Initialize the tracing subscriber for CLI mode (console output).
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
				.unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
		)
		.with_writer(std::io::stderr)
		.init();
}

/// Wrapper for stdout that silently ignores broken pipe errors
/// This prevents child processes from panicking when parent closes the pipe
#[allow(dead_code)]
struct ResilientStdout;

impl std::io::Write for ResilientStdout {
	fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
		match std::io::stdout().write(buf) {
			Ok(n) => Ok(n),
			Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
				// Silently ignore broken pipe - parent closed the connection
				Ok(buf.len())
			}
			Err(e) => Err(e),
		}
	}

	fn flush(&mut self) -> std::io::Result<()> {
		match std::io::stdout().flush() {
			Ok(()) => Ok(()),
			Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
				// Silently ignore broken pipe
				Ok(())
			}
			Err(e) => Err(e),
		}
	}
}

/// Initialize tracing subscriber that propagates messages via protocol
/// Used by child processes (serve.rs) to send all logs through stdout
/// Messages are formatted as: #<LEVEL>:<message> or !E:<message> for errors
#[allow(dead_code)]
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
		.with_writer(|| ResilientStdout)
		.event_format(ProtocolFormatter)
		.with_env_filter(
			tracing_subscriber::EnvFilter::try_from_default_env()
				.unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
		)
		.init();
}

/// Initialize tracing subscriber that forwards events to TUI via broadcast channel
#[cfg(feature = "tui")]
pub fn init_tui_tracing(event_tx: broadcast::Sender<crate::tui::SyncEvent>) {
	use tracing_subscriber::layer::SubscriberExt;
	use tracing_subscriber::util::SubscriberInitExt;

	// Create custom layer that forwards to TUI
	let tui_layer = TuiTracingLayer::new(event_tx);

	// Create registry with TUI layer and env filter
	let _ = tracing_subscriber::registry()
		.with(tui_layer)
		.with(
			tracing_subscriber::EnvFilter::try_from_default_env()
				.unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
		)
		.try_init();
}

/// Custom tracing layer that forwards events to TUI broadcast channel
#[cfg(feature = "tui")]
struct TuiTracingLayer {
	event_tx: broadcast::Sender<crate::tui::SyncEvent>,
}

#[cfg(feature = "tui")]
impl TuiTracingLayer {
	fn new(event_tx: broadcast::Sender<crate::tui::SyncEvent>) -> Self {
		TuiTracingLayer { event_tx }
	}
}

#[cfg(feature = "tui")]
impl<S> tracing_subscriber::Layer<S> for TuiTracingLayer
where
	S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
	fn on_event(
		&self,
		event: &tracing::Event<'_>,
		_ctx: tracing_subscriber::layer::Context<'_, S>,
	) {
		use crate::tui::LogLevel;

		// Map tracing level to our LogLevel
		let level = match *event.metadata().level() {
			tracing::Level::ERROR => LogLevel::Error,
			tracing::Level::WARN => LogLevel::Warning,
			tracing::Level::INFO => LogLevel::Info,
			tracing::Level::DEBUG => LogLevel::Debug,
			tracing::Level::TRACE => LogLevel::Trace,
		};

		// Extract message from event
		let mut message = String::new();
		event.record(&mut |field: &tracing::field::Field, value: &dyn std::fmt::Debug| {
			if field.name() == "message" {
				message = format!("{:?}", value);
			}
		});

		// Remove surrounding quotes if present (debug formatting adds them)
		if message.starts_with('"') && message.ends_with('"') {
			message = message[1..message.len() - 1].to_string();
		}

		// Send log event to TUI
		let _ = self.event_tx.send(crate::tui::SyncEvent::Log { level, message });
	}
}

// vim: ts=4
