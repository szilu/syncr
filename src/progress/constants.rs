//! Progress display constants

/// Width of the progress bar display
pub const PROGRESS_BAR_WIDTH: usize = 30;

/// Bytes per megabyte for display conversions
pub const BYTES_PER_MB: f64 = 1_000_000.0;

/// Throttle updates to this many milliseconds
#[allow(dead_code)]
pub const UPDATE_THROTTLE_MS: u128 = 100;
