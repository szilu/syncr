//! Terminal mode management for raw input handling

use std::io::Write;
use termios::{tcsetattr, Termios, ECHO, ICANON, TCSANOW};

/// RAII guard for raw terminal input mode
/// Disables line buffering (ICANON) and character echo (ECHO)
/// Automatically restores terminal settings on drop
#[allow(dead_code)]
pub struct TerminalGuard {
	fd: i32,
	original: Termios,
}

impl TerminalGuard {
	/// Enable raw terminal mode on stdin
	/// Returns None if not connected to a terminal
	#[allow(dead_code)]
	pub fn new() -> Option<Self> {
		let fd = 0; // stdin
		let original = match Termios::from_fd(fd) {
			Ok(term) => term,
			Err(_) => return None, // Not a terminal
		};
		let mut new_termios = original;
		new_termios.c_lflag &= !(ICANON | ECHO);
		if tcsetattr(fd, TCSANOW, &new_termios).is_err() {
			return None; // Failed to set terminal mode
		}
		Some(TerminalGuard { fd, original })
	}
}

impl Drop for TerminalGuard {
	fn drop(&mut self) {
		restore_terminal_state();
	}
}

/// Restore terminal to normal state
/// Called by TerminalGuard drop and panic hooks
pub fn restore_terminal_state() {
	// Flush output
	let _ = std::io::stdout().flush();
	let _ = std::io::stderr().flush();

	// Try to restore with termios
	if let Ok(term) = Termios::from_fd(0) {
		// Reset flags to normal values (line buffering + echo)
		let mut normal_termios = term;
		normal_termios.c_lflag |= ICANON | ECHO;
		let _ = tcsetattr(0, TCSANOW, &normal_termios);
	}

	// Show cursor if it's hidden (ANSI escape sequence)
	let _ = write!(std::io::stdout(), "\x1B[?25h");
	let _ = write!(std::io::stdout(), "\r\n"); // Move to fresh line
	let _ = std::io::stdout().flush();
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_terminal_guard_creation() {
		// Test that TerminalGuard can be created
		// (may return None if not in a terminal environment)
		let _guard = TerminalGuard::new();
		// Guard should drop without panicking
	}
}

// vim: ts=4
