//! Protocol abstraction layer
//!
//! This module provides a trait-based abstraction for sync communication protocols.
//! The sync engine depends only on the trait, enabling clean separation from protocol-specific details.
//!
//! # Example Usage
//!
//! ```ignore
//! use syncr::protocol::{negotiate_protocol};
//!
//! let protocol = negotiate_protocol(send, recv).await?;
//! protocol.request_listing().await?;
//! while let Some(entry) = protocol.receive_entry().await? {
//!     // Process file system entry
//! }
//! ```

pub mod error;
pub mod factory;
pub mod traits;
pub mod types;
pub mod v3;

// Re-export public API
#[allow(unused_imports)]
pub use error::ProtocolError;
pub use factory::negotiate_protocol;
#[allow(unused_imports)]
pub use traits::{ProtocolResult, SyncProtocol};
#[allow(unused_imports)]
pub use types::{
	ChunkData, ChunkInfo, CommitResponse, FileSystemEntry, FileSystemEntryType, MetadataEntry,
	ProtocolVersion,
};

// vim: ts=4
