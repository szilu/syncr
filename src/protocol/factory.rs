//! Protocol factory and negotiation
//!
//! Creates protocol instances for both local (in-process) and remote (child process)
//! connections. This factory abstracts the transport details from the sync orchestrator.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tracing::{debug, info};

use super::error::ProtocolError;
use super::internal_client::ProtocolInternalClient;
use super::internal_server::ProtocolInternalServer;
use super::negotiation::{ReadyAck, VersionSelection};
use super::traits::*;
use super::v3_client::ProtocolV3;
use crate::serve::DumpState;

//========== NEGOTIATION FUNCTIONS ==========

/// Find the highest protocol version supported by all nodes.
///
/// Takes a list of capabilities (versions supported by each node) and returns
/// the maximum version that appears in the intersection of all sets.
///
/// # Returns
/// - `Ok(version)`: The highest common version
/// - `Err`: NoCommonVersion if no version is supported by all nodes
pub fn find_common_version(all_capabilities: &[Vec<u32>]) -> Result<u32, ProtocolError> {
	if all_capabilities.is_empty() {
		return Err(ProtocolError::NoCommonVersion { capabilities: vec![] });
	}

	// Start with the first node's capabilities
	let mut common: BTreeSet<u32> = all_capabilities[0].iter().cloned().collect();

	// Intersect with all other nodes' capabilities
	for caps in &all_capabilities[1..] {
		let caps_set: BTreeSet<u32> = caps.iter().cloned().collect();
		common = common.intersection(&caps_set).cloned().collect();
	}

	// Return the maximum version in the common set
	common
		.iter()
		.max()
		.cloned()
		.ok_or_else(|| ProtocolError::NoCommonVersion { capabilities: all_capabilities.to_vec() })
}

/// Send the negotiated version to a server.
///
/// Instructs the server which protocol version to use.
async fn send_version_decision(
	send: &mut tokio::process::ChildStdin,
	version: u32,
) -> Result<(), ProtocolError> {
	let decision = VersionSelection::new(version);
	let msg = format!("{decision}\n");
	send.write_all(msg.as_bytes()).await?;
	send.flush().await?;

	debug!("Sent version decision: {}", decision.to_string());

	Ok(())
}

/// Wait for server to acknowledge the version selection.
async fn wait_for_ready(
	recv: &mut BufReader<tokio::process::ChildStdout>,
) -> Result<(), ProtocolError> {
	let mut buf = String::new();

	// Read with timeout
	tokio::time::timeout(Duration::from_secs(10), recv.read_line(&mut buf))
		.await
		.map_err(|_| ProtocolError::VersionSelectionTimeout)??;

	let line = buf.trim();

	// Try to parse as ReadyAck - if it fails, still accept it if it contains "READY"
	match ReadyAck::parse(line) {
		Ok(_) => {
			debug!("Server acknowledged with: {}", line);
			Ok(())
		}
		Err(_) if line.contains("READY") => {
			debug!("Server acknowledged with: {}", line);
			Ok(())
		}
		Err(_) => Err(ProtocolError::ServerDidNotAcknowledgeVersion),
	}
}

/// Create protocol for local in-process leg
///
/// For local directories, we use the internal (channel-based) protocol
/// which has zero serialization overhead and allows direct in-memory communication.
///
/// The server runs in a blocking thread via spawn_blocking to handle the !Send futures.
pub async fn create_local_protocol(
	path: PathBuf,
	state: DumpState,
) -> Result<Box<dyn ProtocolClient>, ProtocolError> {
	info!("Creating local in-process protocol for: {}", path.display());

	// Create channels for bidirectional communication
	let (cmd_tx, cmd_rx) = mpsc::channel(100);
	let (response_tx, response_rx) = mpsc::channel(100);

	// Create server
	let server = ProtocolInternalServer::new(path.clone(), state, cmd_rx, response_tx);

	// Spawn server in a blocking thread with its own tokio runtime
	// This avoids Send/Sync requirements while allowing the async server to run
	std::thread::spawn(move || {
		let rt = tokio::runtime::Runtime::new().unwrap();
		rt.block_on(async {
			if let Err(e) = server.run().await {
				debug!("Local protocol server error: {}", e);
			}
		});
	});

	// Create and return client
	let client = ProtocolInternalClient::new(cmd_tx, response_rx);
	Ok(Box::new(client))
}

//========== PROTOCOL WAITING STATE ==========

/// Represents a protocol connection waiting for version decision.
///
/// After capabilities exchange but before version selection is finalized.
/// This is an intermediate state in the three-phase negotiation.
pub struct ProtocolV3Waiting {
	send: tokio::process::ChildStdin,
	recv: BufReader<tokio::process::ChildStdout>,
	server_capabilities: Vec<u32>,
}

impl ProtocolV3Waiting {
	/// Create a new waiting protocol instance
	pub fn new(
		send: tokio::process::ChildStdin,
		recv: BufReader<tokio::process::ChildStdout>,
		server_capabilities: Vec<u32>,
	) -> Self {
		Self { send, recv, server_capabilities }
	}

	/// Get the server's supported versions
	pub fn server_capabilities(&self) -> &[u32] {
		&self.server_capabilities
	}

	/// Send the version decision and convert to active protocol
	pub async fn finalize(
		mut self,
		version: u32,
	) -> Result<Box<dyn ProtocolClient>, ProtocolError> {
		// Validate version is supported by server
		if !self.server_capabilities.contains(&version) {
			return Err(ProtocolError::UnsupportedVersionRequested {
				requested: version,
				supported: self.server_capabilities,
			});
		}

		// Send USE:VERSION
		send_version_decision(&mut self.send, version).await?;

		// Wait for READY
		wait_for_ready(&mut self.recv).await?;

		info!("Protocol V3 negotiated at version {}", version);

		// Convert to active protocol
		Ok(Box::new(ProtocolV3::new(self.send, self.recv)))
	}
}

/// Create protocol for remote leg (via child process with multi-version negotiation)
///
/// This implements the new negotiation protocol:
/// 1. Wait for server to send SyNcR:X,Y (what versions it supports)
/// 2. Return ProtocolV3Waiting with server capabilities
/// 3. Caller decides version and calls finalize() which sends USE:Z and waits for READY
///
/// This allows sync orchestrator to collect capabilities from all nodes
/// before deciding on a common version.
pub async fn create_remote_protocol(
	send: tokio::process::ChildStdin,
	mut recv: BufReader<tokio::process::ChildStdout>,
) -> Result<ProtocolV3Waiting, ProtocolError> {
	// Phase 1: Wait for server to announce its capabilities (SyNcR:X,Y)
	// Server may emit tracing messages (#I:, #W:, !E:) before capabilities, skip them
	let mut buf = String::new();
	let server_caps = loop {
		buf.clear();
		let n = recv.read_line(&mut buf).await?;
		if n == 0 {
			return Err(ProtocolError::Other(
				"Child process closed connection before announcing capabilities".to_string(),
			));
		}

		let line = buf.trim();

		// Skip trace messages from child (they start with # or !)
		if line.starts_with('#') || line.starts_with('!') {
			debug!("Handshake: skipping child trace message: {}", line);
			continue;
		}

		// Check for server capabilities announcement (SyNcR:X,Y,...)
		if line.starts_with("SyNcR:") {
			match parse_server_capabilities(line) {
				Ok(caps) => break caps,
				Err(e) => return Err(e),
			}
		}

		if !line.is_empty() {
			return Err(ProtocolError::Other(format!(
				"Expected SyNcR: message from server, got: {}",
				line
			)));
		}
	};

	info!("Server capabilities: {:?}", server_caps);

	// Return waiting state - caller will decide version and call finalize()
	Ok(ProtocolV3Waiting::new(send, recv, server_caps))
}

/// Parse server capabilities message: "SyNcR:2,3"
fn parse_server_capabilities(msg: &str) -> Result<Vec<u32>, ProtocolError> {
	if !msg.starts_with("SyNcR:") {
		return Err(ProtocolError::InvalidVersionFormat("Expected SyNcR: prefix".to_string()));
	}

	let versions_str = &msg[6..]; // Skip "SyNcR:"
	if versions_str.is_empty() {
		return Err(ProtocolError::InvalidVersionFormat("Empty version list".to_string()));
	}

	let versions: Result<Vec<u32>, _> =
		versions_str.split(',').map(|v| v.trim().parse::<u32>()).collect();

	match versions {
		Ok(v) => Ok(v),
		Err(e) => {
			Err(ProtocolError::InvalidVersionFormat(format!("Failed to parse versions: {}", e)))
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// ─── find_common_version Tests ───

	#[test]
	fn test_find_common_version_all_same() {
		let caps = vec![vec![2, 3], vec![2, 3], vec![2, 3]];
		let result = find_common_version(&caps);
		assert!(result.is_ok());
		assert_eq!(result.unwrap(), 3);
	}

	#[test]
	fn test_find_common_version_mixed() {
		// Node 0: v2, v3
		// Node 1: v2, v3
		// Node 2: v2 only
		// Common: v2 (all nodes support it)
		let caps = vec![vec![2, 3], vec![2, 3], vec![2]];
		let result = find_common_version(&caps);
		assert!(result.is_ok());
		assert_eq!(result.unwrap(), 2);
	}

	#[test]
	fn test_find_common_version_two_nodes_both_v3() {
		let caps = vec![vec![2, 3], vec![3]];
		let result = find_common_version(&caps);
		assert!(result.is_ok());
		assert_eq!(result.unwrap(), 3);
	}

	#[test]
	fn test_find_common_version_single_node() {
		let caps = vec![vec![2, 3, 4]];
		let result = find_common_version(&caps);
		assert!(result.is_ok());
		assert_eq!(result.unwrap(), 4);
	}

	#[test]
	fn test_find_common_version_single_node_single_version() {
		let caps = vec![vec![3]];
		let result = find_common_version(&caps);
		assert!(result.is_ok());
		assert_eq!(result.unwrap(), 3);
	}

	#[test]
	fn test_find_common_version_no_common() {
		let caps = vec![vec![1], vec![3]];
		let result = find_common_version(&caps);
		assert!(result.is_err());
		match result {
			Err(ProtocolError::NoCommonVersion { capabilities }) => {
				assert_eq!(capabilities.len(), 2);
			}
			_ => panic!("Expected NoCommonVersion error"),
		}
	}

	#[test]
	fn test_find_common_version_empty_list() {
		let caps: Vec<Vec<u32>> = vec![];
		let result = find_common_version(&caps);
		assert!(result.is_err());
	}

	#[test]
	fn test_find_common_version_four_nodes() {
		let caps = vec![vec![1, 2, 3], vec![2, 3, 4], vec![2, 3], vec![2, 3, 5]];
		let result = find_common_version(&caps);
		assert!(result.is_ok());
		assert_eq!(result.unwrap(), 3);
	}

	#[test]
	fn test_find_common_version_only_v2_common() {
		let caps = vec![vec![2, 3], vec![2], vec![2, 3, 4]];
		let result = find_common_version(&caps);
		assert!(result.is_ok());
		assert_eq!(result.unwrap(), 2);
	}

	#[test]
	fn test_find_common_version_large_version_numbers() {
		let caps = vec![vec![10, 20, 30], vec![20, 30], vec![30]];
		let result = find_common_version(&caps);
		assert!(result.is_ok());
		assert_eq!(result.unwrap(), 30);
	}

	#[test]
	fn test_find_common_version_many_nodes_one_blocker() {
		let caps = vec![
			vec![2, 3],
			vec![2, 3],
			vec![2, 3],
			vec![2, 3],
			vec![2], // This node only supports v2
			vec![2, 3],
		];
		let result = find_common_version(&caps);
		assert!(result.is_ok());
		assert_eq!(result.unwrap(), 2);
	}
}

// vim: ts=4
