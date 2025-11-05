//! Connection management for local and remote sync nodes

use crate::error::ConnectionError;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::BufReader;

/// Represents a connection to a sync node (local or remote)
pub struct Node {
	/// Unique node identifier
	pub id: u8,

	/// Location string (path or host:path)
	pub location: String,

	/// Connection type
	pub connection_type: ConnectionType,

	/// Stdin for sending commands
	send: tokio::process::ChildStdin,

	/// Stdout for reading responses
	recv: BufReader<tokio::process::ChildStdout>,

	/// Child process handle
	_child: tokio::process::Child,
}

/// Connection type indicator
#[derive(Debug, Clone)]
pub enum ConnectionType {
	Local { path: PathBuf },
	Remote { host: String, path: String },
}

impl ConnectionType {
	/// Detect connection type from location string
	pub fn detect(location: &str) -> Self {
		let is_relative =
			location.starts_with("/") || location.starts_with(".") || location.starts_with("~");

		if !is_relative {
			if let Some(colon_pos) = location.find(':') {
				let host = location[..colon_pos].to_string();
				let path = location[colon_pos + 1..].to_string();
				return ConnectionType::Remote { host, path };
			}
		}

		ConnectionType::Local { path: PathBuf::from(location) }
	}
}

impl std::fmt::Debug for Node {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Node")
			.field("id", &self.id)
			.field("location", &self.location)
			.field("connection_type", &self.connection_type)
			.finish()
	}
}

/// Connect to a single node
pub async fn connect(location: &str) -> Result<Node, ConnectionError> {
	let conn_type = ConnectionType::detect(location);

	let mut child = match &conn_type {
		ConnectionType::Remote { host, path } => tokio::process::Command::new("ssh")
			.arg(host)
			.arg("syncr")
			.arg("serve")
			.arg(path)
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.spawn()
			.map_err(|e| ConnectionError::SshFailed { host: host.clone(), source: Box::new(e) })?,
		ConnectionType::Local { path } => tokio::process::Command::new("syncr")
			.arg("serve")
			.arg(path)
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.spawn()
			.map_err(|e| ConnectionError::SpawnFailed {
				cmd: "syncr serve".to_string(),
				source: e,
			})?,
	};

	let send = child
		.stdin
		.take()
		.ok_or(ConnectionError::StdioUnavailable { what: "stdin".to_string() })?;

	let stdout = child
		.stdout
		.take()
		.ok_or(ConnectionError::StdioUnavailable { what: "stdout".to_string() })?;

	let recv = BufReader::new(stdout);

	Ok(Node {
		id: 0, // Will be assigned by sync session
		location: location.to_string(),
		connection_type: conn_type,
		send,
		recv,
		_child: child,
	})
}

/// Connect to multiple nodes in parallel
pub async fn connect_all(locations: Vec<&str>) -> Result<Vec<Node>, ConnectionError> {
	let mut handles = Vec::new();

	// Convert &str to owned String to avoid lifetime issues
	let owned_locations: Vec<String> = locations.into_iter().map(|s| s.to_string()).collect();

	for location in owned_locations {
		let handle = tokio::spawn(async move { connect(&location).await });
		handles.push(handle);
	}

	let mut nodes = Vec::new();
	for (i, handle) in handles.into_iter().enumerate() {
		let mut node = handle.await.map_err(|e| ConnectionError::ProtocolError {
			message: format!("Join error: {}", e),
		})??;
		node.id = i as u8;
		nodes.push(node);
	}

	Ok(nodes)
}

impl Node {
	/// Get node ID
	pub fn id(&self) -> u8 {
		self.id
	}

	/// Get location string
	pub fn location(&self) -> &str {
		&self.location
	}

	/// Check if this is a remote connection
	pub fn is_remote(&self) -> bool {
		matches!(self.connection_type, ConnectionType::Remote { .. })
	}

	/// Get the underlying stdin writer
	pub fn stdin(&mut self) -> &mut tokio::process::ChildStdin {
		&mut self.send
	}

	/// Get the underlying stdout reader
	pub fn stdout(&mut self) -> &mut BufReader<tokio::process::ChildStdout> {
		&mut self.recv
	}
}
