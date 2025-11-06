//! Conflicts tab - list and resolve conflicts

use crate::tui::state::AppState;
use crossterm::event::KeyEvent;
use ratatui::{
	layout::Rect,
	style::{Color, Modifier, Style},
	text::{Line, Span},
	widgets::{Block, Borders, Paragraph},
	Frame,
};

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
	let mut lines = vec![];

	if state.sync.conflicts.is_empty() {
		lines.push(Line::from(Span::styled(
			"No conflicts detected",
			Style::default().fg(Color::Green),
		)));
	} else {
		let unresolved_count =
			state.sync.conflicts.iter().filter(|c| c.resolution.is_none()).count();
		let node_count = state.sync.nodes.len();

		if unresolved_count > 0 {
			lines.push(Line::from(vec![
				Span::styled(
					format!("{} unresolved conflicts", unresolved_count),
					Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
				),
				Span::raw(" - Use "),
				Span::styled("↑↓", Style::default().fg(Color::Yellow)),
				Span::raw(" to select, "),
				Span::styled("1-9", Style::default().fg(Color::Yellow)),
				Span::raw(" to choose"),
			]));
		} else {
			lines.push(Line::from(Span::styled(
				"All conflicts resolved!",
				Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
			)));
		}
		lines.push(Line::from(""));

		// Column headers (node numbers) - same format as files tab
		if node_count > 0 {
			let mut header_spans =
				vec![Span::raw("File Path                                      ")];
			for i in 0..node_count {
				header_spans
					.push(Span::styled(format!(" N{} ", i + 1), Style::default().fg(Color::Cyan)));
			}
			lines.push(Line::from(header_spans));
			lines.push(Line::from("─".repeat(area.width as usize)));
		}

		let selected_idx = state.sync.selected_conflict_index.unwrap_or(0);

		for (idx, conflict) in state.sync.conflicts.iter().enumerate() {
			let is_selected = idx == selected_idx;

			let path_str = conflict.path.display().to_string();
			let truncated_path = if path_str.len() > 45 {
				format!("{}...", &path_str[..42])
			} else {
				format!("{:<45}", path_str)
			};

			let mut spans = vec![];

			// Base style for the line (background highlight if selected)
			let base_style =
				if is_selected { Style::default().bg(Color::DarkGray) } else { Style::default() };

			// Selection indicator
			if is_selected {
				spans.push(Span::styled(
					"►",
					base_style.fg(Color::Yellow).add_modifier(Modifier::BOLD),
				));
			} else {
				spans.push(Span::styled(" ", base_style));
			}

			spans.push(Span::styled(
				truncated_path,
				if is_selected {
					base_style.fg(Color::Yellow).add_modifier(Modifier::BOLD)
				} else {
					base_style.fg(Color::Yellow)
				},
			));

			// Find corresponding file in files list to show node status
			if let Some(file) = state.sync.files.iter().find(|f| f.path == conflict.path) {
				for (node_idx, status) in file.node_status.iter().enumerate() {
					use crate::tui::state::FileNodeStatus;

					// Check if this node was chosen as the resolution
					let is_chosen = conflict.resolution == Some(node_idx);

					let (symbol, color) = if is_chosen {
						// Show [x] for chosen node
						(" [x]", Color::Green)
					} else {
						match status {
							FileNodeStatus::Unknown => ("  ? ", Color::DarkGray),
							FileNodeStatus::Exists => ("  * ", Color::Yellow),
							FileNodeStatus::Missing => ("  - ", Color::Red),
							FileNodeStatus::Pending => ("  > ", Color::Yellow),
							FileNodeStatus::Syncing => ("  ~ ", Color::Cyan),
							FileNodeStatus::Synced => ("  = ", Color::Green),
							FileNodeStatus::Failed => ("  ! ", Color::Red),
						}
					};
					spans.push(Span::styled(symbol, base_style.fg(color)));
				}
			} else {
				// Fallback: show Unknown for all nodes if file not tracked
				for node_idx in 0..node_count {
					let is_chosen = conflict.resolution == Some(node_idx);
					let symbol = if is_chosen { " [x]" } else { "  ? " };
					let color = if is_chosen { Color::Green } else { Color::DarkGray };
					spans.push(Span::styled(symbol, base_style.fg(color)));
				}
			}

			lines.push(Line::from(spans).style(base_style));
		}
	}

	let paragraph = Paragraph::new(lines)
		.block(Block::default().title(" Conflicts ").borders(Borders::ALL))
		.scroll((state.sync.scroll_positions.conflicts as u16, 0));

	frame.render_widget(paragraph, area);
}

pub fn handle_key(state: &mut AppState, key: KeyEvent) -> Result<(), Box<dyn std::error::Error>> {
	use crossterm::event::KeyCode;

	if state.sync.conflicts.is_empty() {
		return Ok(());
	}

	// Initialize selected index if needed
	if state.sync.selected_conflict_index.is_none() && !state.sync.conflicts.is_empty() {
		state.sync.selected_conflict_index = Some(0);
	}

	let selected_idx = state.sync.selected_conflict_index.unwrap_or(0);

	match key.code {
		KeyCode::Up => {
			if selected_idx > 0 {
				state.sync.selected_conflict_index = Some(selected_idx - 1);
			}
		}
		KeyCode::Down => {
			if selected_idx < state.sync.conflicts.len() - 1 {
				state.sync.selected_conflict_index = Some(selected_idx + 1);
			}
		}
		KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
			// Number keys 1-9 to choose node
			let node_choice = c.to_digit(10).unwrap() as usize - 1;
			let node_count = state.sync.nodes.len();

			if node_choice < node_count {
				// Check if this conflict is already resolved
				if let Some(conflict) = state.sync.conflicts.get(selected_idx) {
					if conflict.resolution.is_none() {
						// Send resolution to sync engine if channel is available
						if let Some(ref tx) = state.sync.conflict_resolution_tx {
							let resolution = crate::sync_impl::ConflictResolution {
								path: conflict.path.to_string_lossy().to_string(),
								chosen_node: node_choice,
							};

							// Send the resolution (ignore errors if receiver is dropped)
							let _ = tx.send(resolution);
						}

						// Mark as resolved locally
						if let Some(conflict) = state.sync.conflicts.get_mut(selected_idx) {
							conflict.resolution = Some(node_choice);
						}

						// Move to next unresolved conflict
						let next_unresolved = state
							.sync
							.conflicts
							.iter()
							.enumerate()
							.skip(selected_idx + 1)
							.find(|(_, c)| c.resolution.is_none())
							.map(|(idx, _)| idx);

						if let Some(next_idx) = next_unresolved {
							state.sync.selected_conflict_index = Some(next_idx);
						} else {
							// All resolved, stay on current
						}
					}
				}
			}
		}
		_ => {}
	}

	Ok(())
}
