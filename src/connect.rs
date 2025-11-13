use std::error::Error;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::BufReader;
use tracing::info;

/// Connection type that can be either local (in-process) or remote (subprocess with pipes)
#[derive(Debug)]
pub enum ConnectionType {
	/// Local path - will use in-process internal protocol via channels
	Local(PathBuf),
	/// Remote path - uses subprocess with V3 protocol over pipes
	Remote { send: tokio::process::ChildStdin, recv: BufReader<tokio::process::ChildStdout> },
}

/// Determine connection type and establish connection to a directory.
///
/// Returns:
/// - `ConnectionType::Local(path)` for local paths - uses in-process protocol via channels
/// - `ConnectionType::Remote { send, recv }` for remote paths - uses V3 protocol over pipes
pub async fn connect(dir: &str) -> Result<ConnectionType, Box<dyn Error>> {
	// Detect if this is a local or remote path
	let is_remote = if dir.starts_with('/') || dir.starts_with('.') || dir.starts_with('~') {
		false
	} else {
		dir.contains(':')
	};

	if is_remote {
		connect_remote(dir).await
	} else {
		connect_local(dir).await
	}
}

/// Connect to a remote directory via SSH
async fn connect_remote(dir: &str) -> Result<ConnectionType, Box<dyn Error>> {
	// Find the colon separating host and path
	let colon_pos = dir.find(':').ok_or("Expected ':' in remote path")?;
	let host = &dir[..colon_pos];
	let path = &dir[colon_pos + 1..];
	info!("Connecting to remote {}:{}", host, path);

	let mut child = tokio::process::Command::new("ssh")
		.arg(host)
		.arg("syncr")
		.arg("serve")
		.arg(path)
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.spawn()
		.map_err(|e| format!("Failed to spawn SSH subprocess for {}:{}: {}", host, path, e))?;

	let send = child.stdin.take().ok_or("Failed to acquire stdin from subprocess")?;
	let recv =
		BufReader::new(child.stdout.take().ok_or("Failed to acquire stdout from subprocess")?);

	Ok(ConnectionType::Remote { send, recv })
}

/// Connect to a local directory - no subprocess needed!
async fn connect_local(dir: &str) -> Result<ConnectionType, Box<dyn Error>> {
	// Convert to absolute path to ensure it works across threads
	// This is critical for the internal server thread which may have different cwd context
	let path = if dir.starts_with('/') {
		// Already absolute
		PathBuf::from(dir)
	} else {
		// Relative path - make absolute
		let current_dir = std::env::current_dir()?;
		current_dir.join(dir)
	};

	info!("Using in-process protocol for local path: {} (absolute: {})", dir, path.display());
	Ok(ConnectionType::Local(path))
}

// vim: ts=4
