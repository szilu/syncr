//! Utility modules for common functionality

pub mod lock;
pub mod terminal;

// Re-export commonly used items
#[allow(unused_imports)]
pub use lock::{check_shutdown, setup_signal_handlers};
#[allow(unused_imports)]
pub use terminal::TerminalGuard;

// vim: ts=4
