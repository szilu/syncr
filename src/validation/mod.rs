//! Centralized validation system for SyncR
//!
//! This module provides common validation functions and traits for:
//! - Cache validation (TTL/freshness checking)
//! - Configuration validation (chunking parameters, size limits)
//! - Path validation (safety, normalization)

use std::error::Error;
use std::fmt;

pub mod cache;
pub mod config;
pub mod path;

pub use cache::*;
pub use config::*;
pub use path::*;

/// Generic validation error type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
	/// Invalid cache (expired, corrupted, etc.)
	CacheError(String),
	/// Invalid configuration
	ConfigError(String),
	/// Invalid path
	PathError(String),
	/// Other validation error
	Other(String),
}

impl fmt::Display for ValidationError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			ValidationError::CacheError(msg) => write!(f, "Cache validation error: {}", msg),
			ValidationError::ConfigError(msg) => write!(f, "Config validation error: {}", msg),
			ValidationError::PathError(msg) => write!(f, "Path validation error: {}", msg),
			ValidationError::Other(msg) => write!(f, "Validation error: {}", msg),
		}
	}
}

impl Error for ValidationError {}

/// Trait for validatable types
pub trait Validator {
	/// Validate this type
	/// Returns Ok(()) if valid, Err(ValidationError) if invalid
	fn validate(&self) -> Result<(), ValidationError>;
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_validation_error_display() {
		let err = ValidationError::ConfigError("test error".to_string());
		assert!(err.to_string().contains("Config validation error"));
	}

	#[test]
	fn test_validation_error_equality() {
		let err1 = ValidationError::PathError("test".to_string());
		let err2 = ValidationError::PathError("test".to_string());
		assert_eq!(err1, err2);
	}
}
