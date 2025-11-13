//! Configuration validation functions

use super::ValidationError;

/// Validate chunking configuration parameters
///
/// This is the extracted validation from original chunking.rs:139
/// Ensures chunk size parameters are within acceptable ranges.
///
/// # Arguments
/// * `chunk_bits` - Logarithm of chunk size (must be 8-32)
///
/// # Returns
/// `Ok(())` if valid, `Err(ValidationError)` if invalid
pub fn validate_chunk_bits(chunk_bits: u8) -> Result<(), ValidationError> {
	if chunk_bits < 8 {
		return Err(ValidationError::ConfigError(format!(
			"chunk_bits must be at least 8, got {}",
			chunk_bits
		)));
	}
	if chunk_bits > 32 {
		return Err(ValidationError::ConfigError(format!(
			"chunk_bits must be at most 32, got {}",
			chunk_bits
		)));
	}
	Ok(())
}

/// Validate cache size in bytes
pub fn validate_cache_size(size_bytes: u64) -> Result<(), ValidationError> {
	if size_bytes == 0 {
		return Err(ValidationError::ConfigError("Cache size must be greater than 0".to_string()));
	}
	Ok(())
}

/// Validate retry count
pub fn validate_retry_count(count: u32) -> Result<(), ValidationError> {
	if count > 100 {
		return Err(ValidationError::ConfigError(format!("Retry count too high: {}", count)));
	}
	Ok(())
}

/// Validate timeout in seconds
pub fn validate_timeout_secs(timeout_secs: u32) -> Result<(), ValidationError> {
	if timeout_secs == 0 {
		return Err(ValidationError::ConfigError("Timeout must be greater than 0".to_string()));
	}
	if timeout_secs > 3600 {
		return Err(ValidationError::ConfigError(format!(
			"Timeout too large: {} seconds (max 3600)",
			timeout_secs
		)));
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_validate_chunk_bits_valid() {
		assert!(validate_chunk_bits(8).is_ok());
		assert!(validate_chunk_bits(20).is_ok());
		assert!(validate_chunk_bits(32).is_ok());
	}

	#[test]
	fn test_validate_chunk_bits_too_small() {
		let result = validate_chunk_bits(7);
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("at least 8"));
	}

	#[test]
	fn test_validate_chunk_bits_too_large() {
		let result = validate_chunk_bits(33);
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("at most 32"));
	}

	#[test]
	fn test_validate_cache_size_valid() {
		assert!(validate_cache_size(1024).is_ok());
		assert!(validate_cache_size(1).is_ok());
		assert!(validate_cache_size(1_000_000_000).is_ok());
	}

	#[test]
	fn test_validate_cache_size_zero() {
		let result = validate_cache_size(0);
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("greater than 0"));
	}

	#[test]
	fn test_validate_retry_count_valid() {
		assert!(validate_retry_count(1).is_ok());
		assert!(validate_retry_count(50).is_ok());
		assert!(validate_retry_count(100).is_ok());
	}

	#[test]
	fn test_validate_retry_count_too_high() {
		let result = validate_retry_count(101);
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("too high"));
	}

	#[test]
	fn test_validate_timeout_secs_valid() {
		assert!(validate_timeout_secs(1).is_ok());
		assert!(validate_timeout_secs(60).is_ok());
		assert!(validate_timeout_secs(3600).is_ok());
	}

	#[test]
	fn test_validate_timeout_secs_zero() {
		let result = validate_timeout_secs(0);
		assert!(result.is_err());
	}

	#[test]
	fn test_validate_timeout_secs_too_large() {
		let result = validate_timeout_secs(3601);
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("too large"));
	}
}
