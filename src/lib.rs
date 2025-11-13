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
//!         .conflict_resolution(syncr::chunking::ConflictResolution::PreferNewest)
//!         .sync()
//!         .await?;
//!     Ok(())
//! }
//! ```

#![deny(unsafe_code)]
#![warn(dead_code)]

pub mod cache;
pub mod callbacks;
pub mod chunk_tracker;
pub mod chunking;
pub mod config;
pub mod conflict;
pub mod connect;
pub mod connection;
pub mod delete;
pub mod error;
pub mod exclusion;
pub mod logging;
pub mod metadata;
pub mod metadata_utils;
pub mod node_labels;
pub mod progress;
pub mod protocol;
pub mod protocol_utils;
pub mod serve;
pub mod state;
pub mod strategies; // Consolidated strategy/mode enums - declared early to avoid circular deps
pub mod sync;
pub mod sync_impl;
pub mod types;
pub mod util;
pub mod utils;
pub mod validation;

#[cfg(feature = "tui")]
pub mod tui;

// Re-export commonly used types and functions
pub use chunk_tracker::{ChunkTracker, ChunkTrackerError, TransferStatus};
pub use conflict::rules::{ConflictRule, ConflictRuleSet};
pub use conflict::ConflictResolver;
pub use delete::{DeleteHandler, DeleteProtection};
pub use error::{ChunkError, ConnectionError, StateError, SyncError};
pub use exclusion::{ExclusionEngine, ExclusionError};
#[allow(unused_imports)]
pub use metadata::{
	MetadataComparison, MetadataError, MetadataReconciler, MetadataStrategy, NodeCapabilities,
	ReconciliationMode,
};
pub use strategies::DeleteMode;
pub use types::{FileChunk, FileData, FileType, HashChunk};

// vim: ts=4
