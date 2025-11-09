//! Conflict resolution view

use crossterm::event::KeyEvent;
use ratatui::{
	layout::{Constraint, Direction, Layout, Rect},
	style::{Color, Modifier, Style},
	text::{Line, Span},
	widgets::{Block, Borders, Paragraph},
	Frame,
};

use crate::tui::app::TuiCommand;
use crate::tui::state::AppState;
use tokio::sync::mpsc;

/// Render the conflict resolution view
pub fn render(frame: &mut Frame, state: &AppState) {
	let chunks = Layout::default()
		.direction(Direction::Vertical)
		.constraints([
			Constraint::Length(3),  // Header
			Constraint::Length(4),  // File path (increased height)
			Constraint::Min(8),     // Versions
			Constraint::Length(3),  // Footer
		])
		.split(frame.area());

	render_header(frame, chunks[0], state);
	render_file_path(frame, chunks[1], state);
	render_versions(frame, chunks[2], state);
	render_footer(frame, chunks[3]);
}

fn render_header(frame: &mut Frame, area: Rect, state: &AppState) {
	let total_conflicts = state.sync.conflicts.len() + 1;
	let current_num = total_conflicts - state.sync.conflicts.len();

	let header_text = format!("Conflict Resolution - {}/{}", current_num, total_conflicts);

	let header = Paragraph::new(header_text)
		.style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
		.block(Block::default().borders(Borders::ALL).title(" Conflict "));

	frame.render_widget(header, area);
}

fn render_file_path(frame: &mut Frame, area: Rect, state: &AppState) {
	let (path_text, description) = if let Some((ref path, ref desc)) = state.sync.current_conflict {
		(format!("File: {}", path.display()), desc.clone())
	} else {
		("No current conflict".to_string(), String::new())
	};

	let lines = vec![
		Line::from(Span::styled(path_text, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
		Line::from(Span::styled(description, Style::default().fg(Color::DarkGray))),
	];

	let paragraph = Paragraph::new(lines)
		.block(Block::default().borders(Borders::ALL).title(" Conflicting File "));

	frame.render_widget(paragraph, area);
}

fn render_versions(frame: &mut Frame, area: Rect, state: &AppState) {
	let lines = if let Some((_, ref description)) = state.sync.current_conflict {
		// Parse description to extract version information
		// Format: "N versions detected" or specific node info
		let parts: Vec<&str> = description.split_whitespace().collect();
		let num_versions = parts
			.first()
			.and_then(|s| s.parse::<usize>().ok())
			.unwrap_or(2);

		let mut lines = vec![
			Line::from(Span::styled(
				"Multiple versions exist. Choose which one to keep:",
				Style::default().add_modifier(Modifier::BOLD),
			)),
			Line::from(""),
		];

		// Show available options with node locations
		for i in 1..=num_versions.min(3) {
			let style = if i <= num_versions {
				Style::default().fg(Color::Green)
			} else {
				Style::default().fg(Color::DarkGray)
			};

			// Get node location if available
			let node_location = state.sync.nodes.get(i - 1)
				.map(|n| format!(" ({})", n.location))
				.unwrap_or_default();

			lines.push(Line::from(vec![
				Span::styled(
					format!("[{}] ", i),
					Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan),
				),
				Span::styled(format!("Keep version from node {}{}", i, node_location), style),
			]));
		}

		lines.push(Line::from(""));
		lines.push(Line::from(vec![
			Span::styled("[s] ", Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan)),
			Span::styled("Skip this file", Style::default().fg(Color::Yellow)),
		]));
		lines.push(Line::from(""));
		lines.push(Line::from(Span::styled(
			"Tip: Compare file timestamps/sizes at each location to decide.",
			Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
		)));

		lines
	} else {
		vec![Line::from("No current conflict")]
	};

	let paragraph = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Options "));

	frame.render_widget(paragraph, area);
}

fn render_footer(frame: &mut Frame, area: Rect) {
	let footer = Paragraph::new("[1-3] Choose version  [s] Skip  [q] Quit  [Esc] Back  [?] Help")
		.style(Style::default().fg(Color::DarkGray))
		.block(Block::default().borders(Borders::ALL));

	frame.render_widget(footer, area);
}

/// Handle keyboard input in conflict view
pub async fn handle_key(
	state: &mut AppState,
	key: KeyEvent,
	_command_tx: &mpsc::Sender<TuiCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
	use crossterm::event::KeyCode;

	match key.code {
		KeyCode::Char('1') | KeyCode::Char('2') | KeyCode::Char('3') => {
			// User chose a version
			let chosen_node = match key.code {
				KeyCode::Char('1') => 1,
				KeyCode::Char('2') => 2,
				KeyCode::Char('3') => 3,
				_ => 0,
			};

			if chosen_node > 0 {
				if let Some((ref path, _)) = state.sync.current_conflict {
					state.add_log(
						crate::tui::state::LogLevel::Info,
						format!("Conflict resolved: {} (chose version {})", path.display(), chosen_node),
					);
				}
				// Move to next conflict
				move_to_next_conflict(state);
			}
		}
		KeyCode::Char('s') => {
			// Skip this file/conflict
			if let Some((ref path, _)) = state.sync.current_conflict {
				state.add_log(
					crate::tui::state::LogLevel::Warning,
					format!("Conflict skipped: {}", path.display()),
				);
			}
			move_to_next_conflict(state);
		}
		KeyCode::Char('q') => {
			// Quit - close conflict view and return to sync view
			state.sync.current_conflict = None;
			state.sync.conflicts.clear();
			state.change_view(crate::tui::state::ViewType::Sync);
		}
		KeyCode::Esc => {
			// Escape - return to sync view without clearing remaining conflicts
			state.sync.current_conflict = None;
			state.change_view(crate::tui::state::ViewType::Sync);
		}
		_ => {}
	}

	Ok(())
}

/// Move to the next conflict in the queue
fn move_to_next_conflict(state: &mut AppState) {
	if let Some((path, description)) = state.sync.conflicts.pop_front() {
		state.sync.current_conflict = Some((path, description));
	} else {
		// No more conflicts - return to sync view
		state.sync.current_conflict = None;
		state.change_view(crate::tui::state::ViewType::Sync);
	}
}

// vim: ts=4
