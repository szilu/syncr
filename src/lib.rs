//! # SyncR - Fast Deduplicating Filesystem Synchronizer
//!
//! SyncR is a fast, content-determined chunk-based filesystem synchronizer
//! that can perform n-way synchronization across local and remote directories.
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use syncr::sync::sync;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let result = sync(vec!["./dir1", "./dir2"], None).await?;
//!     println!("Synced {} files", result.files_synced);
//!     Ok(())
//! }
//! ```
//!
//! ## Using the Builder Pattern
//!
//! ```rust,ignore
//! use syncr::sync::SyncBuilder;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let result = SyncBuilder::new()
//!         .add_location("./dir1")
//!         .add_location("./dir2")
//!         .conflict_resolution(syncr::config::ConflictResolution::PreferNewest)
//!         .sync()
//!         .await?;
//!     Ok(())
//! }
//! ```

pub mod callbacks;
pub mod chunking;
pub mod config;
pub mod conflict;
pub mod connection;
pub mod error;
pub mod metadata_utils;
pub mod protocol_utils;
pub mod state;
pub mod sync;
pub mod types;
pub mod util;

// Re-export commonly used types and functions
pub use config::SyncConfig;
pub use error::{ChunkError, ConflictError, ConnectionError, StateError, SyncError};
pub use types::{FileChunk, FileData, FileType, HashChunk};

// vim: ts=4
