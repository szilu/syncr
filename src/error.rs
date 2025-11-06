//! Error types for SyncR operations

use std::error::Error;
use std::fmt;
use std::io;

/// Main error type for sync operations
#[derive(Debug)]
pub enum SyncError {
	/// Failed to connect to a location
	ConnectionFailed { location: String, source: Box<dyn Error + Send + Sync> },

	/// Permission denied on a path
	PermissionDenied { path: String },

	/// Sync state is corrupted
	StateCorrupted { message: String },

	/// Protocol version mismatch
	ProtocolMismatch { local: u8, remote: u8 },

	/// Hash verification failed
	HashMismatch { expected: String, actual: String },

	/// I/O error
	Io(io::Error),

	/// Invalid configuration
	InvalidConfig { message: String },

	/// Lock acquisition failed
	LockFailed { message: String },

	/// Operation aborted by user
	Aborted,

	/// Connection error (nested)
	Connection(ConnectionError),

	/// Chunk error (nested)
	Chunk(ChunkError),

	/// State error (nested)
	State(StateError),

	/// Conflict error (nested)
	Conflict(ConflictError),

	/// Generic error message
	Other { message: String },
}

impl fmt::Display for SyncError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			SyncError::ConnectionFailed { location, source } => {
				write!(f, "Failed to connect to {}: {}", location, source)
			}
			SyncError::PermissionDenied { path } => {
				write!(f, "Permission denied: {}", path)
			}
			SyncError::StateCorrupted { message } => {
				write!(f, "Sync state corrupted: {}", message)
			}
			SyncError::ProtocolMismatch { local, remote } => {
				write!(f, "Protocol version mismatch: local={}, remote={}", local, remote)
			}
			SyncError::HashMismatch { expected, actual } => {
				write!(f, "Hash mismatch: expected {}, got {}", expected, actual)
			}
			SyncError::Io(e) => write!(f, "I/O error: {}", e),
			SyncError::InvalidConfig { message } => {
				write!(f, "Invalid configuration: {}", message)
			}
			SyncError::LockFailed { message } => {
				write!(f, "Lock acquisition failed: {}", message)
			}
			SyncError::Aborted => write!(f, "Operation aborted by user"),
			SyncError::Connection(e) => write!(f, "Connection error: {}", e),
			SyncError::Chunk(e) => write!(f, "Chunk error: {}", e),
			SyncError::State(e) => write!(f, "State error: {}", e),
			SyncError::Conflict(e) => write!(f, "Conflict error: {}", e),
			SyncError::Other { message } => write!(f, "{}", message),
		}
	}
}

impl Error for SyncError {}

impl From<io::Error> for SyncError {
	fn from(e: io::Error) -> Self {
		SyncError::Io(e)
	}
}

impl From<Box<dyn Error>> for SyncError {
	fn from(e: Box<dyn Error>) -> Self {
		SyncError::Other { message: e.to_string() }
	}
}

impl From<String> for SyncError {
	fn from(e: String) -> Self {
		SyncError::Other { message: e }
	}
}

impl From<ConnectionError> for SyncError {
	fn from(e: ConnectionError) -> Self {
		SyncError::Connection(e)
	}
}

impl From<ChunkError> for SyncError {
	fn from(e: ChunkError) -> Self {
		SyncError::Chunk(e)
	}
}

impl From<StateError> for SyncError {
	fn from(e: StateError) -> Self {
		SyncError::State(e)
	}
}

impl From<ConflictError> for SyncError {
	fn from(e: ConflictError) -> Self {
		SyncError::Conflict(e)
	}
}

/// Connection-specific errors
#[derive(Debug)]
pub enum ConnectionError {
	/// SSH connection failed
	SshFailed { host: String, source: Box<dyn Error + Send + Sync> },

	/// Subprocess spawn failed
	SpawnFailed { cmd: String, source: io::Error },

	/// Protocol handshake failed
	HandshakeFailed { message: String },

	/// Protocol error (invalid message format)
	ProtocolError { message: String },

	/// Connection disconnected unexpectedly
	Disconnected,

	/// Operation timeout
	Timeout,

	/// Stdio unavailable
	StdioUnavailable { what: String },
}

impl fmt::Display for ConnectionError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			ConnectionError::SshFailed { host, source } => {
				write!(f, "SSH connection to {} failed: {}", host, source)
			}
			ConnectionError::SpawnFailed { cmd, source } => {
				write!(f, "Failed to spawn '{}': {}", cmd, source)
			}
			ConnectionError::HandshakeFailed { message } => {
				write!(f, "Handshake failed: {}", message)
			}
			ConnectionError::ProtocolError { message } => {
				write!(f, "Protocol error: {}", message)
			}
			ConnectionError::Disconnected => write!(f, "Connection disconnected"),
			ConnectionError::Timeout => write!(f, "Connection timeout"),
			ConnectionError::StdioUnavailable { what } => {
				write!(f, "Stdio unavailable: {}", what)
			}
		}
	}
}

impl Error for ConnectionError {}

/// Chunking-specific errors
#[derive(Debug)]
pub enum ChunkError {
	/// Failed to read chunk data
	ReadFailed { source: io::Error },

	/// Invalid chunk configuration
	InvalidConfig { message: String },

	/// Hash verification failed
	HashFailed { message: String },

	/// Chunk size out of bounds
	SizeOutOfBounds { size: usize, max: usize },
}

impl fmt::Display for ChunkError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			ChunkError::ReadFailed { source } => write!(f, "Failed to read chunk: {}", source),
			ChunkError::InvalidConfig { message } => write!(f, "Invalid chunk config: {}", message),
			ChunkError::HashFailed { message } => {
				write!(f, "Hash verification failed: {}", message)
			}
			ChunkError::SizeOutOfBounds { size, max } => {
				write!(f, "Chunk size {} exceeds maximum {}", size, max)
			}
		}
	}
}

impl Error for ChunkError {}

impl From<io::Error> for ChunkError {
	fn from(e: io::Error) -> Self {
		ChunkError::ReadFailed { source: e }
	}
}

/// State management errors
#[derive(Debug)]
pub enum StateError {
	/// Failed to load state
	LoadFailed { source: Box<dyn Error + Send + Sync> },

	/// Failed to save state
	SaveFailed { source: Box<dyn Error + Send + Sync> },

	/// Lock acquisition failed
	LockFailed { message: String },

	/// State file is corrupted
	Corrupted { message: String },

	/// Invalid state directory
	InvalidDirectory { path: String },
}

impl fmt::Display for StateError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			StateError::LoadFailed { source } => write!(f, "Failed to load state: {}", source),
			StateError::SaveFailed { source } => write!(f, "Failed to save state: {}", source),
			StateError::LockFailed { message } => write!(f, "Lock failed: {}", message),
			StateError::Corrupted { message } => write!(f, "State corrupted: {}", message),
			StateError::InvalidDirectory { path } => {
				write!(f, "Invalid state directory: {}", path)
			}
		}
	}
}

impl Error for StateError {}

/// Conflict resolution errors
#[derive(Debug)]
pub enum ConflictError {
	/// Invalid winner choice (index out of range)
	InvalidChoice { choice: usize, max: usize },

	/// User cancelled operation
	UserCancelled,

	/// Conflict resolution strategy failed
	StrategyFailed { message: String },

	/// Conflict is unresolvable
	Unresolvable { message: String },
}

impl fmt::Display for ConflictError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			ConflictError::InvalidChoice { choice, max } => {
				write!(f, "Invalid choice {}: only 0-{} available", choice, max - 1)
			}
			ConflictError::UserCancelled => write!(f, "User cancelled operation"),
			ConflictError::StrategyFailed { message } => {
				write!(f, "Conflict resolution strategy failed: {}", message)
			}
			ConflictError::Unresolvable { message } => {
				write!(f, "Conflict is unresolvable: {}", message)
			}
		}
	}
}

impl Error for ConflictError {}

// Convenience conversion from Box<dyn Error> for original error handling
pub fn box_error_to_sync_error(e: Box<dyn Error>) -> SyncError {
	SyncError::Other { message: e.to_string() }
}

// vim: ts=4
