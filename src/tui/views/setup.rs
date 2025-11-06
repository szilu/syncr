//! Setup view for directory and configuration selection

use crossterm::event::KeyEvent;
use ratatui::{
	layout::{Constraint, Direction, Layout, Rect},
	style::{Color, Modifier, Style},
	text::{Line, Span},
	widgets::{Block, Borders, Paragraph, Wrap},
	Frame,
};

use crate::tui::state::AppState;

/// Render the setup view
pub fn render(frame: &mut Frame, state: &AppState) {
	let chunks = Layout::default()
		.direction(Direction::Vertical)
		.constraints([Constraint::Length(3), Constraint::Min(10), Constraint::Length(3)])
		.split(frame.area());

	// Header
	render_header(frame, chunks[0]);

	// Content
	render_content(frame, chunks[1], state);

	// Footer
	render_footer(frame, chunks[2]);
}

fn render_header(frame: &mut Frame, area: Rect) {
	let header = Paragraph::new("SyncR - Setup")
		.style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
		.block(Block::default().borders(Borders::ALL).title(" Setup "));

	frame.render_widget(header, area);
}

fn render_content(frame: &mut Frame, area: Rect, state: &AppState) {
	let content = vec![Line::from(Span::raw("Sync Locations:")), Line::from("")];

	let mut location_lines: Vec<Line> = state
		.locations
		.iter()
		.enumerate()
		.map(|(i, loc)| Line::from(format!("  [{}] {}", i + 1, loc)))
		.collect();

	let mut all_lines = content;
	all_lines.append(&mut location_lines);
	all_lines.push(Line::from(""));
	all_lines.push(Line::from(Span::styled(
		"Press [Enter] to start sync, [Esc] to cancel",
		Style::default().fg(Color::DarkGray),
	)));

	let paragraph = Paragraph::new(all_lines)
		.block(Block::default().borders(Borders::ALL))
		.wrap(Wrap { trim: true });

	frame.render_widget(paragraph, area);
}

fn render_footer(frame: &mut Frame, area: Rect) {
	let footer = Paragraph::new("[Enter] Start  [Esc] Cancel  [?] Help")
		.style(Style::default().fg(Color::DarkGray))
		.block(Block::default().borders(Borders::ALL));

	frame.render_widget(footer, area);
}

/// Handle keyboard input in setup view
pub async fn handle_key(
	state: &mut AppState,
	key: KeyEvent,
	_command_tx: &tokio::sync::mpsc::Sender<crate::tui::app::TuiCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
	use crossterm::event::KeyCode;

	match key.code {
		KeyCode::Enter => {
			// Signal to start sync by changing view
			// The app will detect this and spawn the sync task
			state.change_view(crate::tui::state::ViewType::Sync);
		}
		KeyCode::Esc => {
			state.should_quit = true;
		}
		_ => {}
	}

	Ok(())
}

// vim: ts=4
