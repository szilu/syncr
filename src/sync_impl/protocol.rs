//! Protocol handling for syncr communication

use std::error::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, info, trace, warn};

/// Protocol version for handshake and compatibility checking
pub const PROTOCOL_VERSION: u8 = 1;

/// Parse a trace message from child process
/// Format: #<LEVEL>:<message> or !<LEVEL>:<message>
/// Returns: (level_char, message_string)
pub fn parse_trace_message(line: &str) -> Option<(char, String)> {
	if line.len() < 3 {
		return None;
	}

	let prefix = line.chars().next()?;
	if prefix != '#' && prefix != '!' {
		return None;
	}

	let level = line.chars().nth(1)?;
	let message = if line.len() > 3 && line.chars().nth(2) == Some(':') {
		line[3..].to_string()
	} else {
		String::new()
	};

	Some((level, message))
}

/// Log a trace message from child with node context
pub fn log_trace_message(level: char, message: &str, node_id: u8) {
	match level {
		'I' => info!("[node {}] {}", node_id, message),
		'W' => warn!("[node {}] {}", node_id, message),
		'D' => debug!("[node {}] {}", node_id, message),
		'T' => trace!("[node {}] {}", node_id, message),
		'E' => error!("[node {}] {}", node_id, message),
		_ => {}
	}
}

/// Perform protocol version handshake with a remote node
/// Parent sends VERSION:<version>, child responds with VERSION:<version>
pub async fn handshake(
	send: &mut tokio::process::ChildStdin,
	recv: &mut BufReader<tokio::process::ChildStdout>,
) -> Result<(), Box<dyn Error>> {
	// Read server's ready signal (.)
	// Child may emit tracing messages (#I:, #W:, !E:) before ready signal, skip them
	let mut buf = String::new();
	loop {
		buf.clear();
		let n = recv.read_line(&mut buf).await?;
		if n == 0 {
			return Err("Child process closed connection before sending ready signal".into());
		}

		let line = buf.trim();

		// Skip trace messages from child (they start with # or !)
		if line.starts_with('#') || line.starts_with('!') {
			debug!("Handshake: skipping child trace message: {}", line);
			continue;
		}

		// Check for ready signal
		if line == "." {
			break;
		}

		return Err(format!("Expected ready signal from server, got: {}", line).into());
	}

	// Send our protocol version
	send.write_all(format!("VERSION:{}\n", PROTOCOL_VERSION).as_bytes()).await?;
	send.flush().await?;

	// Read remote protocol version
	buf.clear();
	recv.read_line(&mut buf).await?;

	let fields: Vec<&str> = buf.trim().split(':').collect();
	if fields.len() != 2 || fields[0] != "VERSION" {
		return Err("Invalid handshake response".into());
	}

	let remote_version: u8 =
		fields[1].parse().map_err(|_| "Invalid version number in handshake")?;

	if remote_version != PROTOCOL_VERSION {
		return Err(format!(
			"Protocol version mismatch: local={}, remote={}",
			PROTOCOL_VERSION, remote_version
		)
		.into());
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_parse_trace_message_info() {
		let (level, msg) = parse_trace_message("#I:test message").unwrap();
		assert_eq!(level, 'I');
		assert_eq!(msg, "test message");
	}

	#[test]
	fn test_parse_trace_message_warning() {
		let (level, msg) = parse_trace_message("#W:warning").unwrap();
		assert_eq!(level, 'W');
		assert_eq!(msg, "warning");
	}

	#[test]
	fn test_parse_trace_message_error_prefix() {
		let (level, msg) = parse_trace_message("!E:error").unwrap();
		assert_eq!(level, 'E');
		assert_eq!(msg, "error");
	}

	#[test]
	fn test_parse_trace_message_invalid() {
		assert!(parse_trace_message("invalid").is_none());
		assert!(parse_trace_message("ab").is_none());
		assert!(parse_trace_message("$I:test").is_none());
	}
}

// vim: ts=4
