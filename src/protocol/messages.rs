//! Internal protocol message types
//!
//! These message types represent the protocol commands and responses
//! used by the internal (in-process) protocol implementation.
//! They are type-safe and efficient for in-memory communication via channels.

use super::types::*;
use std::path::PathBuf;

/// Commands sent from client to server (internal protocol)
#[derive(Debug, Clone)]
pub enum ProtocolCommand {
	/// Request node capabilities
	Capabilities,

	/// Request directory listing
	List,

	/// Begin metadata transfer phase
	BeginWrite,

	/// Send file/directory/symlink metadata
	WriteMetadata(MetadataEntry),

	/// Send deletion command
	Delete(PathBuf),

	/// End metadata transfer phase
	EndWrite,

	/// Begin chunk transfer phase
	BeginRead,

	/// Request specific chunks by hash
	RequestChunks(Vec<String>),

	/// Send chunk data
	SendChunk { hash: String, data: Vec<u8> },

	/// End chunk transfer phase
	EndRead,

	/// Commit all changes
	Commit,

	/// Quit/close connection
	Quit,
}

/// Responses sent from server to client (internal protocol)
#[derive(Debug, Clone)]
pub enum ProtocolResponse {
	/// Capabilities response
	Capabilities(NodeCapabilities),

	/// File system entry (during listing)
	Entry(FileSystemEntry),

	/// End of listing
	EndOfList,

	/// Chunk data (during read)
	Chunk(ChunkData),

	/// End of chunk stream
	EndOfChunks,

	/// Write operation completed
	WriteOk,

	/// Commit result
	CommitResult(CommitResponse),

	/// Generic success
	Ok,

	/// Error response
	Error(String),
}

// vim: ts=4
