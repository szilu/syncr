use std::error::Error;

/// Parse a protocol line into fields, validating minimum field count.
///
/// Protocol lines are colon-separated field values. This function:
/// - Trims whitespace
/// - Splits on ':' delimiter
/// - Validates minimum field count
///
/// # Arguments
/// * `buf` - Protocol line to parse
/// * `expected_fields` - Minimum number of fields required
///
/// # Returns
/// Vector of field strings if valid, or error describing mismatch
///
/// # Example
/// ```ignore
/// let fields = parse_protocol_line("FILE:path:hash:size", 3)?;
/// assert_eq!(fields, vec!["FILE", "path", "hash", "size"]);
/// ```
pub fn parse_protocol_line(buf: &str, expected_fields: usize) -> Result<Vec<&str>, Box<dyn Error>> {
	let fields: Vec<&str> = buf.trim().split(':').collect();
	if fields.len() < expected_fields {
		return Err(format!(
			"Protocol error: expected {} fields, got {} in line: {}",
			expected_fields,
			fields.len(),
			buf.trim()
		)
		.into());
	}
	Ok(fields)
}
