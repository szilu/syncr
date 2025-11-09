//! Protocol factory and negotiation
//!
//! Handles handshake negotiation with remote nodes and creates
//! the appropriate protocol instance based on negotiated version.

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, info};

use super::error::ProtocolError;
use super::traits::*;
use super::v3::ProtocolV3;

/// Negotiate protocol version and create protocol instance
pub async fn negotiate_protocol(
	mut send: tokio::process::ChildStdin,
	mut recv: BufReader<tokio::process::ChildStdout>,
) -> Result<Box<dyn SyncProtocol>, ProtocolError> {
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

	// Negotiate V3 protocol (JSON5)
	let ver_cmd = serde_json::json!({
		"cmd": "VER",
		"ver": 3,
	});

	let ver_json = serde_json::to_string(&ver_cmd)?;
	send.write_all(format!("{}\n", ver_json).as_bytes()).await?;
	send.flush().await?;

	// Read response
	buf.clear();
	recv.read_line(&mut buf).await?;
	let response_line = buf.trim();

	// Parse response as v3 JSON5
	match json5::from_str::<serde_json::Value>(response_line) {
		Ok(response) => {
			// Check for successful version response
			if let Some("VER") = response.get("cmd").and_then(|v| v.as_str()) {
				if let Some(3) = response.get("ver").and_then(|v| v.as_i64()) {
					info!("Successfully negotiated Protocol V3 (JSON5)");
					return Ok(Box::new(ProtocolV3::new(send, recv)));
				}
			}

			// Check for error response
			if let Some("ERR") = response.get("cmd").and_then(|v| v.as_str()) {
				let msg = response.get("msg").and_then(|v| v.as_str()).unwrap_or("Unknown error");
				return Err(format!("Child error during handshake: {}", msg).into());
			}

			// Unexpected response format
			Err(format!(
				"Unexpected handshake response: expected cmd=VER ver=3, got: {}",
				response_line
			)
			.into())
		}
		Err(e) => Err(format!(
			"Failed to parse handshake response as JSON5: {} (got: {})",
			e, response_line
		)
		.into()),
	}
}

#[cfg(test)]
mod tests {
	use super::super::types::ProtocolVersion;

	#[test]
	fn test_protocol_version_v3() {
		assert_eq!(ProtocolVersion::V3, ProtocolVersion::V3);
	}
}

// vim: ts=4
