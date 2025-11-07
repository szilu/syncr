//! Nodes tab - shows statistics for each sync node

use ratatui::{
	layout::Rect,
	style::{Color, Modifier, Style},
	text::{Line, Span},
	widgets::{Block, Borders, Paragraph},
	Frame,
};

use crate::tui::state::AppState;
use crate::types::SyncPhase;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
	let mut lines: Vec<Line> = vec![];

	// Check if sync completed
	let sync_completed = !state.sync.is_running && state.sync.result.is_some();

	if sync_completed {
		// Show completion banner
		lines.push(Line::from(vec![
			Span::styled("✓ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
			Span::styled(
				"Sync Completed Successfully!",
				Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
			),
		]));
		lines.push(Line::from(""));

		// Show summary statistics if available
		if let Some(ref result) = state.sync.result {
			lines.push(Line::from(format!(
				"Bytes transferred: {:.2} MB",
				result.bytes_transferred as f64 / 1_000_000.0
			)));
			lines.push(Line::from(format!("Chunks transferred: {}", result.chunks_transferred)));
			lines.push(Line::from(format!("Chunks deduplicated: {}", result.chunks_deduplicated)));
			lines.push(Line::from(format!(
				"Conflicts: {} encountered, {} resolved",
				result.conflicts_encountered, result.conflicts_resolved
			)));
			lines.push(Line::from(format!("Duration: {:.2}s", result.duration.as_secs_f64())));
			lines.push(Line::from(""));

			// Show warning if there were unresolved conflicts
			let unresolved = state.sync.conflicts.iter().filter(|c| c.resolution.is_none()).count();
			if unresolved > 0 {
				lines.push(Line::from(""));
				lines.push(Line::from(Span::styled(
					format!("⚠ Warning: {} conflicts were skipped!", unresolved),
					Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
				)));
				lines.push(Line::from(vec![
					Span::styled("  Switch to ", Style::default().fg(Color::DarkGray)),
					Span::styled("C", Style::default().fg(Color::Yellow)),
					Span::styled("onflicts tab to review", Style::default().fg(Color::DarkGray)),
				]));
			}

			lines.push(Line::from(""));
			lines.push(Line::from(vec![
				Span::styled("Press ", Style::default().fg(Color::DarkGray)),
				Span::styled("q", Style::default().fg(Color::Yellow)),
				Span::styled(" to exit", Style::default().fg(Color::DarkGray)),
			]));
			lines.push(Line::from(""));
		}
	}

	// Check if we're in collecting phase
	let is_collecting = state.sync.phase == Some(SyncPhase::Collecting);

	if is_collecting {
		// Show animated collecting status
		let spinner_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
		let spinner_index = (state.ui.animation_frame as usize) / 4 % spinner_chars.len();
		let spinner = spinner_chars[spinner_index];

		// Count nodes that have collected data (files_collected > 0)
		let nodes_ready = state.sync.nodes.iter().filter(|n| n.files_collected > 0).count();
		let total_nodes = state.sync.nodes.len();

		lines.push(Line::from(vec![
			Span::styled(format!("{} ", spinner), Style::default().fg(Color::Yellow)),
			Span::styled(
				format!("Collecting from {}/{} nodes...", nodes_ready, total_nodes),
				Style::default().fg(Color::Cyan),
			),
		]));

		lines.push(Line::from(""));
	}

	// Check if we're waiting for conflict resolution
	let unresolved_count = state.sync.conflicts.iter().filter(|c| c.resolution.is_none()).count();
	let is_detecting_conflicts = state.sync.phase == Some(SyncPhase::DetectingConflicts);

	if state.sync.is_running
		&& !state.sync.conflicts.is_empty()
		&& unresolved_count > 0
		&& is_detecting_conflicts
	{
		// Show animated waiting status
		let spinner_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
		let spinner_index = (state.ui.animation_frame as usize) / 4 % spinner_chars.len();
		let spinner = spinner_chars[spinner_index];

		let resolved_count = state.sync.conflicts.len() - unresolved_count;

		lines.push(Line::from(vec![
			Span::styled(format!("{} ", spinner), Style::default().fg(Color::Yellow)),
			Span::styled(
				format!(
					"Waiting for conflict resolution: {}/{} resolved",
					resolved_count,
					state.sync.conflicts.len()
				),
				Style::default().fg(Color::Yellow),
			),
		]));
		lines.push(Line::from(vec![
			Span::styled("  Switch to ", Style::default().fg(Color::DarkGray)),
			Span::styled("C", Style::default().fg(Color::Yellow)),
			Span::styled("onflicts tab to resolve", Style::default().fg(Color::DarkGray)),
		]));

		lines.push(Line::from(""));
	}

	// Show all nodes with their statistics
	for node in &state.sync.nodes {
		// During collection phase, show spinner for nodes that are actively collecting
		let (status, status_color) = if is_collecting {
			if node.files_collected > 0 {
				// Node is actively collecting - show animated spinner
				let spinner_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
				let spinner_index = (state.ui.animation_frame as usize) / 4 % spinner_chars.len();
				let spinner = spinner_chars[spinner_index];
				(format!("{}", spinner), Color::Yellow)
			} else if node.connected {
				// Connected but not collecting yet
				("○".to_string(), Color::DarkGray)
			} else {
				// Not connected
				("✗".to_string(), Color::Red)
			}
		} else if node.connected {
			// After collection phase - show status
			("✓".to_string(), Color::Green)
		} else {
			("✗".to_string(), Color::Red)
		};

		lines.push(Line::from(vec![
			Span::styled(format!("  {} ", status), Style::default().fg(status_color)),
			Span::styled(&node.label, Style::default().add_modifier(Modifier::BOLD)),
		]));

		// Show three separate statistics
		lines.push(Line::from(format!(
			"     Collected: {} files ({:.1} MB)",
			node.files_collected,
			node.bytes_collected as f64 / 1_000_000.0,
		)));

		lines.push(Line::from(format!(
			"     Sent: {:.1} MB | Received: {:.1} MB",
			node.bytes_sent as f64 / 1_000_000.0,
			node.bytes_received as f64 / 1_000_000.0,
		)));

		if let Some(ref error) = node.error {
			lines.push(Line::from(Span::styled(
				format!("     Error: {}", error),
				Style::default().fg(Color::Red),
			)));
		}

		lines.push(Line::from(""));
	}

	let paragraph = Paragraph::new(lines)
		.block(Block::default().title(" Node Statistics ").borders(Borders::ALL))
		.scroll((state.sync.scroll_positions.nodes as u16, 0));

	frame.render_widget(paragraph, area);
}
