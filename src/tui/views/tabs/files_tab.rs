//! Files tab - shows all files with their status across nodes

use crate::tui::state::{AppState, FileNodeStatus};
use ratatui::{
	layout::Rect,
	style::{Color, Style},
	text::{Line, Span},
	widgets::{Block, Borders, Paragraph},
	Frame,
};

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
	let mut lines: Vec<Line> = vec![];

	if state.sync.files.is_empty() {
		lines.push(Line::from(Span::styled(
			"No files tracked yet",
			Style::default().fg(Color::DarkGray),
		)));
		lines.push(Line::from(""));
		lines.push(Line::from("Files will appear here as they are discovered during sync."));
	} else {
		// Header showing node count
		let node_count = state.sync.nodes.len();
		lines.push(Line::from(format!(
			"Total files: {} | Nodes: {}",
			state.sync.files.len(),
			node_count
		)));
		lines.push(Line::from(""));

		// Column headers (node labels)
		if node_count > 0 {
			let mut header_spans =
				vec![Span::raw("File Path                                      ")];
			for node in &state.sync.nodes {
				let label_short = if node.label.len() > 4 {
					node.label[..4].to_string()
				} else {
					node.label.clone()
				};
				header_spans.push(Span::styled(
					format!(" {:>4} ", label_short),
					Style::default().fg(Color::Cyan),
				));
			}
			lines.push(Line::from(header_spans));
			lines.push(Line::from("─".repeat(area.width as usize)));
		}

		// Show each file with status on each node
		for file in &state.sync.files {
			let path_str = file.path.display().to_string();
			let truncated_path = if path_str.len() > 45 {
				format!("{}...", &path_str[..42])
			} else {
				format!("{:<45}", path_str)
			};

			let mut spans = vec![Span::raw(truncated_path)];

			// Add status for each node
			for status in &file.node_status {
				let (symbol, color) = match status {
					FileNodeStatus::Unknown => ("  ? ", Color::DarkGray),
					FileNodeStatus::Exists => ("  + ", Color::Green),
					FileNodeStatus::Missing => ("  - ", Color::Red),
					FileNodeStatus::Pending => ("  > ", Color::Yellow),
					FileNodeStatus::Syncing => ("  ~ ", Color::Cyan),
					FileNodeStatus::Synced => ("  = ", Color::Green),
					FileNodeStatus::Failed => ("  ! ", Color::Red),
				};
				spans.push(Span::styled(symbol, Style::default().fg(color)));
			}

			// Highlight conflicts
			if file.is_conflict {
				lines.push(Line::from(spans).style(Style::default().fg(Color::Yellow)));
			} else {
				lines.push(Line::from(spans));
			}
		}
	}

	// Legend at the bottom
	lines.push(Line::from(""));
	lines.push(Line::from(vec![
		Span::styled("Legend: ", Style::default().fg(Color::DarkGray)),
		Span::styled("+", Style::default().fg(Color::Green)),
		Span::raw(" Exists  "),
		Span::styled("=", Style::default().fg(Color::Green)),
		Span::raw(" Synced  "),
		Span::styled("-", Style::default().fg(Color::Red)),
		Span::raw(" Missing  "),
		Span::styled(">", Style::default().fg(Color::Yellow)),
		Span::raw(" Pending  "),
		Span::styled("~", Style::default().fg(Color::Cyan)),
		Span::raw(" Syncing  "),
		Span::styled("!", Style::default().fg(Color::Red)),
		Span::raw(" Failed"),
	]));

	let paragraph = Paragraph::new(lines)
		.block(Block::default().title(" All Files ").borders(Borders::ALL))
		.scroll((state.sync.scroll_positions.files as u16, 0));

	frame.render_widget(paragraph, area);
}
