//! Color themes and styling

use ratatui::style::{Color, Modifier, Style};

/// Color theme for the TUI
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Theme {
	pub primary: Color,
	pub secondary: Color,
	pub success: Color,
	pub warning: Color,
	pub error: Color,
	pub text: Color,
	pub text_muted: Color,
	pub bg: Color,
}

impl Theme {
	/// Create the default theme
	#[allow(dead_code)]
	pub fn default() -> Self {
		Theme {
			primary: Color::Cyan,
			secondary: Color::Blue,
			success: Color::Green,
			warning: Color::Yellow,
			error: Color::Red,
			text: Color::White,
			text_muted: Color::DarkGray,
			bg: Color::Black,
		}
	}

	/// Style for primary elements
	#[allow(dead_code)]
	pub fn primary_style(&self) -> Style {
		Style::default().fg(self.primary)
	}

	/// Style for secondary elements
	#[allow(dead_code)]
	pub fn secondary_style(&self) -> Style {
		Style::default().fg(self.secondary)
	}

	/// Style for success messages
	#[allow(dead_code)]
	pub fn success_style(&self) -> Style {
		Style::default().fg(self.success)
	}

	/// Style for warning messages
	#[allow(dead_code)]
	pub fn warning_style(&self) -> Style {
		Style::default().fg(self.warning)
	}

	/// Style for error messages
	#[allow(dead_code)]
	pub fn error_style(&self) -> Style {
		Style::default().fg(self.error)
	}

	/// Style for normal text
	#[allow(dead_code)]
	pub fn text_style(&self) -> Style {
		Style::default().fg(self.text)
	}

	/// Style for muted/secondary text
	#[allow(dead_code)]
	pub fn muted_style(&self) -> Style {
		Style::default().fg(self.text_muted)
	}

	/// Style for selected elements
	#[allow(dead_code)]
	pub fn selected_style(&self) -> Style {
		Style::default().fg(self.bg).bg(self.primary).add_modifier(Modifier::BOLD)
	}

	/// Style for header text
	#[allow(dead_code)]
	pub fn header_style(&self) -> Style {
		Style::default().fg(self.primary).add_modifier(Modifier::BOLD)
	}
}

impl Default for Theme {
	fn default() -> Self {
		Self::default()
	}
}

// vim: ts=4
