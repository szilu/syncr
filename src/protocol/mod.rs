//! Protocol abstraction layer
//!
//! This module provides a trait-based abstraction for sync communication protocols.
//! The sync engine depends only on the trait, enabling clean separation from protocol-specific details.
//!
//! # Architecture
//!
//! The protocol layer supports multiple implementations:
//! - **Internal Protocol**: In-process channels for local file system legs
//! - **V3 Protocol**: JSON5 over pipes for remote connections
//!
//! # Example Usage
//!
//! ```ignore
//! use syncr::protocol::{create_local_protocol, create_remote_protocol};
//!
//! // For local paths
//! let protocol = create_local_protocol(path, state).await?;
//!
//! // For remote paths
//! let protocol = create_remote_protocol(stdin, stdout).await?;
//!
//! // Use the protocol the same way regardless of implementation
//! protocol.request_listing().await?;
//! while let Some(entry) = protocol.receive_entry().await? {
//!     // Process file system entry
//! }
//! ```

pub mod error;
pub mod factory;
pub mod file_operations;
pub mod internal_client;
pub mod internal_server;
pub mod messages;
pub mod negotiation;
pub mod traits;
pub mod types;
pub mod v3_client;
pub mod v3_server;

// Re-export public API
#[allow(unused_imports)]
pub use error::ProtocolError;
pub use factory::{create_local_protocol, create_remote_protocol};
#[allow(unused_imports)]
pub use traits::{ProtocolClient, ProtocolResult, ProtocolServer};
#[allow(unused_imports)]
pub use types::{
	ChunkData, ChunkInfo, CommitResponse, FileSystemEntry, FileSystemEntryType, MetadataEntry,
};

// vim: ts=4
