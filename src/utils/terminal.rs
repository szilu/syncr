//! Terminal mode management for raw input handling

use termios::{tcsetattr, Termios, ECHO, ICANON, TCSANOW};

/// RAII guard for raw terminal input mode
/// Disables line buffering (ICANON) and character echo (ECHO)
/// Automatically restores terminal settings on drop
pub struct TerminalGuard {
	fd: i32,
	original: Termios,
}

impl TerminalGuard {
	/// Enable raw terminal mode on stdin
	/// Returns None if not connected to a terminal
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
		// Restore terminal even if panic occurs
		let _ = tcsetattr(self.fd, TCSANOW, &self.original);
	}
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
