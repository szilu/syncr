//! Help and keybindings view

use crossterm::event::KeyEvent;
use ratatui::{
	layout::{Constraint, Direction, Layout, Rect},
	style::{Color, Modifier, Style},
	text::{Line, Span},
	widgets::{Block, Borders, Paragraph},
	Frame,
};

use crate::tui::state::{AppState, ViewType};

/// Render the help view
pub fn render(frame: &mut Frame, _state: &AppState) {
	let chunks = Layout::default()
		.direction(Direction::Vertical)
		.constraints([Constraint::Length(3), Constraint::Min(10), Constraint::Length(3)])
		.split(frame.area());

	render_header(frame, chunks[0]);
	render_content(frame, chunks[1]);
	render_footer(frame, chunks[2]);
}

fn render_header(frame: &mut Frame, area: Rect) {
	let header = Paragraph::new("Help - Keybindings")
		.style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
		.block(Block::default().borders(Borders::ALL).title(" Help "));

	frame.render_widget(header, area);
}

fn render_content(frame: &mut Frame, area: Rect) {
	let lines = vec![
		Line::from(Span::styled("Global Commands:", Style::default().add_modifier(Modifier::BOLD))),
		Line::from("  Ctrl+C, q     Quit the application"),
		Line::from("  ?             Show this help"),
		Line::from(""),
		Line::from(Span::styled(
			"Sync View Commands:",
			Style::default().add_modifier(Modifier::BOLD),
		)),
		Line::from("  p             Pause/Resume sync"),
		Line::from("  a             Abort sync"),
		Line::from(""),
		Line::from(Span::styled(
			"Conflict Resolution Commands:",
			Style::default().add_modifier(Modifier::BOLD),
		)),
		Line::from("  1, 2, 3       Choose version 1, 2, or 3"),
		Line::from("  s             Skip this file"),
		Line::from(""),
		Line::from(Span::styled(
			"Dashboard Commands:",
			Style::default().add_modifier(Modifier::BOLD),
		)),
		Line::from("  s             Start sync"),
		Line::from("  c             Configure settings"),
		Line::from("  l             View logs"),
	];

	let paragraph = Paragraph::new(lines).block(Block::default().borders(Borders::ALL));

	frame.render_widget(paragraph, area);
}

fn render_footer(frame: &mut Frame, area: Rect) {
	let footer = Paragraph::new("[Esc] Back to previous view")
		.style(Style::default().fg(Color::DarkGray))
		.block(Block::default().borders(Borders::ALL));

	frame.render_widget(footer, area);
}

/// Handle keyboard input in help view
pub async fn handle_key(
	state: &mut AppState,
	key: KeyEvent,
) -> Result<(), Box<dyn std::error::Error>> {
	use crossterm::event::KeyCode;

	if key.code == KeyCode::Esc {
		// Go back to Sync view if sync is running, otherwise Setup
		if state.sync.is_running {
			state.change_view(ViewType::Sync);
		} else {
			state.change_view(ViewType::Setup);
		}
	}

	Ok(())
}

// vim: ts=4
