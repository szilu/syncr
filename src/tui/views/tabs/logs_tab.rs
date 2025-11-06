//! Logs tab - shows log messages

use crate::tui::state::{AppState, LogLevel};
use crossterm::event::KeyEvent;
use ratatui::{
	layout::Rect,
	style::{Color, Style},
	text::{Line, Span},
	widgets::{Block, Borders, Paragraph},
	Frame,
};

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
	let mut lines: Vec<Line> = vec![];

	for entry in state.logs.iter().rev().take(100) {
		let (prefix, color) = match entry.level {
			LogLevel::Error => ("[ERROR]", Color::Red),
			LogLevel::Warning => ("[WARN]", Color::Yellow),
			LogLevel::Success => ("[OK]", Color::Green),
			LogLevel::Info => ("[INFO]", Color::Cyan),
			LogLevel::Debug => ("[DEBUG]", Color::DarkGray),
			LogLevel::Trace => ("[TRACE]", Color::DarkGray),
		};

		lines.push(Line::from(vec![
			Span::styled(format!("{} ", prefix), Style::default().fg(color)),
			Span::raw(&entry.message),
		]));
	}

	if lines.is_empty() {
		lines.push(Line::from("No log messages yet"));
	}

	let paragraph = Paragraph::new(lines)
		.block(Block::default().title(" Logs ").borders(Borders::ALL))
		.scroll((state.sync.scroll_positions.logs as u16, 0));

	frame.render_widget(paragraph, area);
}

pub async fn handle_key(
	state: &mut AppState,
	key: KeyEvent,
) -> Result<(), Box<dyn std::error::Error>> {
	use crossterm::event::KeyCode;

	match key.code {
		KeyCode::Char('x') | KeyCode::Char('X') => {
			state.logs.clear();
		}
		_ => {}
	}

	Ok(())
}
