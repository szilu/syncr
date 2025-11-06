//! TUI frontend for SyncR
//!
//! This module is only compiled when the 'tui' feature is enabled.
//! It provides a terminal user interface for interactive synchronization
//! management using the ratatui framework.

mod app;
mod bridge;
mod event;
mod state;
mod ui;
mod views;

pub use app::run_tui;
pub use event::SyncEvent;
pub use state::LogLevel;

// vim: ts=4
