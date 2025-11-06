//! Dashboard view showing sync locations and recent activity

use crossterm::event::KeyEvent;
use ratatui::{
	layout::{Constraint, Direction, Layout, Rect},
	style::{Color, Modifier, Style},
	text::{Line, Span},
	widgets::{Block, Borders, Paragraph},
	Frame,
};

use crate::tui::state::AppState;

/// Render the dashboard view
pub fn render(frame: &mut Frame, state: &AppState) {
	let chunks = Layout::default()
		.direction(Direction::Vertical)
		.constraints([Constraint::Length(3), Constraint::Min(10), Constraint::Length(3)])
		.split(frame.area());

	// Header
	render_header(frame, chunks[0], state);

	// Content
	render_content(frame, chunks[1], state);

	// Footer
	render_footer(frame, chunks[2]);
}

fn render_header(frame: &mut Frame, area: Rect, state: &AppState) {
	let profile = &state.config.profile;
	let header_text = format!("SyncR - Dashboard (Profile: {})", profile);
	let header = Paragraph::new(header_text)
		.style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
		.block(Block::default().borders(Borders::ALL).title(" Dashboard "));

	frame.render_widget(header, area);
}

fn render_content(frame: &mut Frame, area: Rect, state: &AppState) {
	let mut lines: Vec<Line> = vec![Line::from("Sync Locations:"), Line::from("")];

	for node in &state.sync.nodes {
		let status = if node.connected { "✓" } else { "✗" };
		let status_color = if node.connected { Color::Green } else { Color::Red };

		lines.push(Line::from(vec![
			Span::styled(status, Style::default().fg(status_color)),
			Span::raw(format!(" {}", node.location)),
		]));

		lines.push(Line::from(format!(
			"   Files: {} | Data: {:.2} MB",
			node.files_received + node.files_sent,
			(node.bytes_received + node.bytes_sent) as f64 / 1_000_000.0
		)));

		// Show error if present
		if let Some(ref err) = node.error {
			lines.push(Line::from(Span::styled(
				format!("   Error: {}", err),
				Style::default().fg(Color::Red),
			)));
		}
	}

	lines.push(Line::from(""));
	lines.push(Line::from("Recent Activity:"));
	lines.push(Line::from("  (Log entries would appear here)"));

	let paragraph = Paragraph::new(lines).block(Block::default().borders(Borders::ALL));

	frame.render_widget(paragraph, area);
}

fn render_footer(frame: &mut Frame, area: Rect) {
	let footer = Paragraph::new("[s] Start Sync  [?] Help  [q] Quit")
		.style(Style::default().fg(Color::DarkGray))
		.block(Block::default().borders(Borders::ALL));

	frame.render_widget(footer, area);
}

/// Handle keyboard input in dashboard view
pub async fn handle_key(
	state: &mut AppState,
	key: KeyEvent,
) -> Result<(), Box<dyn std::error::Error>> {
	use crossterm::event::KeyCode;

	match key.code {
		KeyCode::Char('s') => {
			// TODO: Start sync
		}
		KeyCode::Char('q') => {
			state.should_quit = true;
		}
		_ => {}
	}

	Ok(())
}

// vim: ts=4
