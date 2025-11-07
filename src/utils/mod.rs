//! Utility modules for common functionality

pub mod lock;
pub mod terminal;

// Re-export commonly used items
pub use lock::setup_signal_handlers;
pub use terminal::TerminalGuard;

// vim: ts=4
