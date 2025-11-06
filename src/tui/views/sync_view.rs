//! Active sync view showing progress and status

use crossterm::event::KeyEvent;
use ratatui::{
	layout::{Constraint, Direction, Layout, Rect},
	style::{Color, Modifier, Style},
	text::{Line, Span},
	widgets::{Block, Borders, Gauge, Paragraph},
	Frame,
};

use crate::tui::app::TuiCommand;
use crate::tui::state::AppState;
use tokio::sync::mpsc;

/// Render the active sync view
pub fn render(frame: &mut Frame, state: &AppState) {
	let chunks = Layout::default()
		.direction(Direction::Vertical)
		.constraints([
			Constraint::Length(3), // Header
			Constraint::Length(5), // Overall progress
			Constraint::Length(5), // Current phase
			Constraint::Length(3), // Current file
			Constraint::Min(5),    // Node status
			Constraint::Length(3), // Footer
		])
		.split(frame.area());

	render_header(frame, chunks[0], state);
	render_overall_progress(frame, chunks[1], state);
	render_phase_progress(frame, chunks[2], state);
	render_current_file(frame, chunks[3], state);
	render_node_status(frame, chunks[4], state);
	render_footer(frame, chunks[5]);
}

fn render_header(frame: &mut Frame, area: Rect, state: &AppState) {
	let phase_name = state
		.sync
		.phase
		.as_ref()
		.map(|p| format!("{:?}", p))
		.unwrap_or_else(|| "Initializing".to_string());

	let header_text = format!("SyncR - Synchronizing (Phase: {})", phase_name);
	let header = Paragraph::new(header_text)
		.style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
		.block(Block::default().borders(Borders::ALL).title(" Sync "));

	frame.render_widget(header, area);
}

fn render_overall_progress(frame: &mut Frame, area: Rect, state: &AppState) {
	use crate::types::SyncPhase;

	let (ratio, label) = if let Some(ref progress) = state.sync.progress {
		let ratio = if progress.files_total > 0 {
			progress.files_processed as f64 / progress.files_total as f64
		} else {
			0.0
		};

		// During Committing phase, show "nodes" instead of "files"
		let unit = if state.sync.phase == Some(SyncPhase::Committing) {
			"nodes"
		} else {
			"files"
		};

		let label = format!("{}/{} {}", progress.files_processed, progress.files_total, unit);
		(ratio, label)
	} else {
		(0.0, "0/0 files".to_string())
	};

	let gauge = Gauge::default()
		.block(Block::default().title("Overall Progress").borders(Borders::ALL))
		.gauge_style(Style::default().fg(Color::Green).bg(Color::Black))
		.percent((ratio * 100.0) as u16)
		.label(label);

	frame.render_widget(gauge, area);
}

fn render_phase_progress(frame: &mut Frame, area: Rect, state: &AppState) {
	let (bytes_done, bytes_total, ratio, rate) = if let Some(ref progress) = state.sync.progress {
		let ratio = if progress.bytes_total > 0 {
			progress.bytes_transferred as f64 / progress.bytes_total as f64
		} else {
			0.0
		};
		(progress.bytes_transferred, progress.bytes_total, ratio, progress.transfer_rate)
	} else {
		(0, 0, 0.0, 0.0)
	};

	let mb_done = bytes_done as f64 / 1_000_000.0;
	let mb_total = bytes_total as f64 / 1_000_000.0;
	let mb_rate = rate / 1_000_000.0;

	let gauge = Gauge::default()
		.block(Block::default().title("Transfer Progress").borders(Borders::ALL))
		.gauge_style(Style::default().fg(Color::Yellow).bg(Color::Black))
		.percent((ratio * 100.0) as u16)
		.label(format!("{:.1}/{:.1} MB @ {:.1} MB/s", mb_done, mb_total, mb_rate));

	frame.render_widget(gauge, area);
}

fn render_current_file(frame: &mut Frame, area: Rect, state: &AppState) {
	let file_text = if let Some(first_op) = state.sync.active_operations.first() {
		format!(
			"Current: {}",
			first_op
				.path
				.file_name()
				.and_then(|n| n.to_str())
				.unwrap_or("(unknown)")
		)
	} else {
		"Current: (none)".to_string()
	};

	let paragraph = Paragraph::new(file_text)
		.block(Block::default().title("File").borders(Borders::ALL))
		.style(Style::default().fg(Color::Yellow));

	frame.render_widget(paragraph, area);
}

fn render_node_status(frame: &mut Frame, area: Rect, state: &AppState) {
	use crate::types::SyncPhase;

	let mut lines: Vec<Line> = Vec::new();

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
			Span::styled(
				format!("{} ", spinner),
				Style::default().fg(Color::Yellow),
			),
			Span::styled(
				format!("Collecting from {}/{} nodes...", nodes_ready, total_nodes),
				Style::default().fg(Color::Cyan),
			),
		]));

		lines.push(Line::from(""));
	} else {
		lines.push(Line::from("Node Status:"));
	}

	for node in &state.sync.nodes {
		let status = if node.connected { "✓" } else { "✗" };
		let status_color = if node.connected { Color::Green } else { Color::Red };

		lines.push(Line::from(vec![
			Span::styled(format!("  {} ", status), Style::default().fg(status_color)),
			Span::raw(&node.location),
		]));

		// Show three separate statistics
		lines.push(Line::from(format!(
			"     Collected: {} files ({:.1} MB) | Sent: {} ({:.1} MB) | Received: {} ({:.1} MB)",
			node.files_collected,
			node.bytes_collected as f64 / 1_000_000.0,
			node.files_sent,
			node.bytes_sent as f64 / 1_000_000.0,
			node.files_received,
			node.bytes_received as f64 / 1_000_000.0,
		)));
	}

	let paragraph =
		Paragraph::new(lines).block(Block::default().title("Nodes").borders(Borders::ALL));

	frame.render_widget(paragraph, area);
}

fn render_footer(frame: &mut Frame, area: Rect) {
	let footer = Paragraph::new("[a] Abort  [?] Help")
		.style(Style::default().fg(Color::DarkGray))
		.block(Block::default().borders(Borders::ALL));

	frame.render_widget(footer, area);
}

/// Handle keyboard input in sync view
pub async fn handle_key(
	state: &mut AppState,
	key: KeyEvent,
	command_tx: &mpsc::Sender<TuiCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
	use crossterm::event::KeyCode;

	match key.code {
		KeyCode::Char('a') => {
			let _ = command_tx.send(TuiCommand::AbortSync).await;
			state.should_quit = true;
		}
		_ => {}
	}

	Ok(())
}

// vim: ts=4
