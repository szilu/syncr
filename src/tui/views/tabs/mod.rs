//! Tab-based sync view with multiple panels

use crossterm::event::KeyEvent;
use ratatui::{
	layout::{Constraint, Direction, Layout, Rect},
	style::{Color, Modifier, Style},
	text::Span,
	widgets::{Block, Borders, Gauge, Paragraph, Tabs},
	Frame,
};

use crate::tui::state::{AppState, TabType};
use crate::types::SyncPhase;

mod conflicts_tab;
mod files_tab;
mod logs_tab;
mod nodes_tab;

/// Render the main sync view with tabs
pub fn render(frame: &mut Frame, state: &AppState) {
	let chunks = Layout::default()
		.direction(Direction::Vertical)
		.constraints([
			Constraint::Length(7), // Header with progress
			Constraint::Length(3), // Tabs
			Constraint::Min(10),   // Tab content
			Constraint::Length(3), // Footer
		])
		.split(frame.area());

	render_header(frame, chunks[0], state);
	render_tabs(frame, chunks[1], state);
	render_active_tab_content(frame, chunks[2], state);
	render_footer(frame, chunks[3], state);
}

fn render_header(frame: &mut Frame, area: Rect, state: &AppState) {
	// Check if sync is complete
	let sync_completed = !state.sync.is_running && state.sync.result.is_some();

	let (phase_name, header_color) = if sync_completed {
		("✓ Completed".to_string(), Color::Green)
	} else {
		let phase = state
			.sync
			.phase
			.as_ref()
			.map(|p| format!("{:?}", p))
			.unwrap_or_else(|| "Initializing".to_string());
		(phase, Color::Cyan)
	};

	let header_text = format!("SyncR - Phase: {}", phase_name);

	// Split header into title and progress
	let header_chunks = Layout::default()
		.direction(Direction::Vertical)
		.constraints([Constraint::Length(3), Constraint::Length(4)])
		.split(area);

	// Title
	let header = Paragraph::new(header_text)
		.style(Style::default().fg(header_color).add_modifier(Modifier::BOLD))
		.block(Block::default().borders(Borders::ALL));
	frame.render_widget(header, header_chunks[0]);

	// Progress bar (only show when sync is running)
	if !sync_completed {
		if let Some(ref progress) = state.sync.progress {
			let ratio = if progress.files_total > 0 {
				progress.files_processed as f64 / progress.files_total as f64
			} else {
				0.0
			};

			let (_unit, progress_text) = match state.sync.phase {
				Some(SyncPhase::Committing) => (
					"nodes",
					format!("{}/{} nodes", progress.files_processed, progress.files_total),
				),
				Some(SyncPhase::TransferringChunks) => (
					"chunks",
					format!(
						"{}/{} chunks | {:.1} MB | {:.1} MB/s",
						progress.files_processed,
						progress.files_total,
						progress.bytes_transferred as f64 / 1_000_000.0,
						progress.transfer_rate
					),
				),
				_ => (
					"files",
					format!("{}/{} files", progress.files_processed, progress.files_total),
				),
			};

			let progress_widget = Gauge::default()
				.block(Block::default().borders(Borders::ALL).title(" Progress "))
				.gauge_style(Style::default().fg(Color::Cyan))
				.percent((ratio.clamp(0.0, 1.0) * 100.0) as u16)
				.label(progress_text);
			frame.render_widget(progress_widget, header_chunks[1]);
		}
	} else {
		// Show completion summary in progress area
		if let Some(ref result) = state.sync.result {
			let duration_secs = result.duration.as_secs_f64();
			let mb_transferred = result.bytes_transferred as f64 / 1_000_000.0;
			let overall_rate =
				if duration_secs > 0.0 { mb_transferred / duration_secs } else { 0.0 };
			let node_count = state.sync.nodes.len().max(1);
			let _avg_rate_per_node = overall_rate / node_count as f64;

			let summary_text = format!(
				"Duration: {:.2}s | {:.1} MB | {:.1} MB/s",
				duration_secs, mb_transferred, overall_rate,
			);
			let summary_widget = Paragraph::new(summary_text)
				.style(Style::default().fg(Color::Green))
				.block(Block::default().borders(Borders::ALL).title(" Summary "));
			frame.render_widget(summary_widget, header_chunks[1]);
		}
	}
}

fn render_tabs(frame: &mut Frame, area: Rect, state: &AppState) {
	let conflict_count = state.sync.conflicts.len();
	let conflict_badge =
		if conflict_count > 0 { format!(" ({})", conflict_count) } else { String::new() };

	let titles = vec![
		Span::raw("[S] Nodes"),
		Span::raw("[F] Files"),
		Span::raw(format!("[C] Conflicts{}", conflict_badge)),
		Span::raw("[L] Logs"),
	];

	let selected = match state.sync.active_tab {
		TabType::Nodes => 0,
		TabType::Files => 1,
		TabType::Conflicts => 2,
		TabType::Logs => 3,
	};

	let tabs = Tabs::new(titles)
		.block(Block::default().borders(Borders::ALL).title(" Views "))
		.highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
		.select(selected);

	frame.render_widget(tabs, area);
}

fn render_active_tab_content(frame: &mut Frame, area: Rect, state: &AppState) {
	match state.sync.active_tab {
		TabType::Nodes => nodes_tab::render(frame, area, state),
		TabType::Files => files_tab::render(frame, area, state),
		TabType::Conflicts => conflicts_tab::render(frame, area, state),
		TabType::Logs => logs_tab::render(frame, area, state),
	}
}

fn render_footer(frame: &mut Frame, area: Rect, state: &AppState) {
	let sync_completed = !state.sync.is_running && state.sync.result.is_some();

	let footer_text = if sync_completed {
		match state.sync.active_tab {
			TabType::Nodes => "[S/F/C/L] Switch Tab | [q] Quit",
			TabType::Files => "[S/F/C/L] Switch Tab | [↑↓] Scroll | [q] Quit",
			TabType::Conflicts => "[S/F/C/L] Switch Tab | [↑↓] Scroll | [q] Quit",
			TabType::Logs => "[S/F/C/L] Switch Tab | [↑↓] Scroll | [x] Clear | [q] Quit",
		}
	} else {
		match state.sync.active_tab {
			TabType::Nodes => "[S/F/C/L] Switch Tab | [↑↓] Scroll | [q] Quit",
			TabType::Files => "[S/F/C/L] Switch Tab | [↑↓] Scroll | [q] Quit",
			TabType::Conflicts => {
				"[S/F/C/L] Switch Tab | [↑↓] Navigate | [1-9] Choose Node | [q] Quit"
			}
			TabType::Logs => "[S/F/C/L] Switch Tab | [↑↓] Scroll | [x] Clear | [q] Quit",
		}
	};

	let footer = Paragraph::new(footer_text)
		.style(Style::default().fg(Color::DarkGray))
		.block(Block::default().borders(Borders::ALL));

	frame.render_widget(footer, area);
}

/// Handle keyboard input in tabbed sync view
pub async fn handle_key(
	state: &mut AppState,
	key: KeyEvent,
	_command_tx: &tokio::sync::mpsc::Sender<crate::tui::app::TuiCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
	use crossterm::event::KeyCode;

	match key.code {
		// Tab switching (case-insensitive)
		KeyCode::Char('s') | KeyCode::Char('S') => {
			state.sync.active_tab = TabType::Nodes;
		}
		KeyCode::Char('f') | KeyCode::Char('F') => {
			state.sync.active_tab = TabType::Files;
		}
		KeyCode::Char('c') | KeyCode::Char('C') => {
			state.sync.active_tab = TabType::Conflicts;
		}
		KeyCode::Char('l') | KeyCode::Char('L') => {
			state.sync.active_tab = TabType::Logs;
		}

		// Scrolling / Navigation (conflicts tab uses arrow keys for navigation, not scrolling)
		KeyCode::Up | KeyCode::Down => {
			match state.sync.active_tab {
				TabType::Conflicts => {
					// Let conflicts tab handle arrow keys for navigation
					conflicts_tab::handle_key(state, key)?;
				}
				_ => {
					// Regular scrolling for other tabs
					let scroll_pos = match state.sync.active_tab {
						TabType::Nodes => &mut state.sync.scroll_positions.nodes,
						TabType::Files => &mut state.sync.scroll_positions.files,
						TabType::Conflicts => &mut state.sync.scroll_positions.conflicts,
						TabType::Logs => &mut state.sync.scroll_positions.logs,
					};
					if key.code == KeyCode::Up {
						*scroll_pos = scroll_pos.saturating_sub(1);
					} else {
						*scroll_pos += 1;
					}
				}
			}
		}

		// Tab-specific actions
		_ => match state.sync.active_tab {
			TabType::Conflicts => conflicts_tab::handle_key(state, key)?,
			TabType::Logs => logs_tab::handle_key(state, key).await?,
			_ => {}
		},
	}

	Ok(())
}
