//! Layout utilities and common layout patterns

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Create a vertical split layout
#[allow(dead_code)]
pub fn vertical_split(area: Rect, heights: &[Constraint]) -> Vec<Rect> {
	Layout::default()
		.direction(Direction::Vertical)
		.constraints(heights)
		.split(area)
		.to_vec()
}

/// Create a horizontal split layout
#[allow(dead_code)]
pub fn horizontal_split(area: Rect, widths: &[Constraint]) -> Vec<Rect> {
	Layout::default()
		.direction(Direction::Horizontal)
		.constraints(widths)
		.split(area)
		.to_vec()
}

/// Create a centered rect within another rect
#[allow(dead_code)]
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
	let popup_layout = Layout::default()
		.direction(Direction::Vertical)
		.constraints([
			Constraint::Percentage((100 - percent_y) / 2),
			Constraint::Percentage(percent_y),
			Constraint::Percentage((100 - percent_y) / 2),
		])
		.split(r);

	Layout::default()
		.direction(Direction::Horizontal)
		.constraints([
			Constraint::Percentage((100 - percent_x) / 2),
			Constraint::Percentage(percent_x),
			Constraint::Percentage((100 - percent_x) / 2),
		])
		.split(popup_layout[1])[1]
}

// vim: ts=4
