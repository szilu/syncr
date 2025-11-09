//! Protocol error types
//!
//! Provides comprehensive error handling for the protocol module with
//! automatic conversions from various underlying error types.

use std::fmt;
use std::io;

/// Protocol error type
#[derive(Debug)]
pub enum ProtocolError {
	/// I/O error from async operations
	Io(io::Error),
	/// JSON5 parsing error
	Json5(String),
	/// Base64 decoding error
	Base64(String),
	/// Protocol violation (unexpected format or state)
	ProtocolViolation(String),
	/// Generic error message
	Other(String),
}

impl fmt::Display for ProtocolError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			ProtocolError::Io(e) => write!(f, "I/O error: {}", e),
			ProtocolError::Json5(msg) => write!(f, "JSON5 parse error: {}", msg),
			ProtocolError::Base64(msg) => write!(f, "Base64 decode error: {}", msg),
			ProtocolError::ProtocolViolation(msg) => write!(f, "Protocol violation: {}", msg),
			ProtocolError::Other(msg) => write!(f, "{}", msg),
		}
	}
}

impl std::error::Error for ProtocolError {}

// From implementations for automatic conversion
impl From<io::Error> for ProtocolError {
	fn from(e: io::Error) -> Self {
		ProtocolError::Io(e)
	}
}

impl From<String> for ProtocolError {
	fn from(e: String) -> Self {
		ProtocolError::Other(e)
	}
}

impl From<&str> for ProtocolError {
	fn from(e: &str) -> Self {
		ProtocolError::Other(e.to_string())
	}
}

impl From<base64::DecodeError> for ProtocolError {
	fn from(e: base64::DecodeError) -> Self {
		ProtocolError::Base64(e.to_string())
	}
}

impl From<json5::Error> for ProtocolError {
	fn from(e: json5::Error) -> Self {
		ProtocolError::Json5(e.to_string())
	}
}

impl From<serde_json::Error> for ProtocolError {
	fn from(e: serde_json::Error) -> Self {
		ProtocolError::Json5(e.to_string())
	}
}

impl From<Box<dyn std::error::Error>> for ProtocolError {
	fn from(e: Box<dyn std::error::Error>) -> Self {
		ProtocolError::Other(e.to_string())
	}
}

impl From<Box<dyn std::error::Error + Send + Sync>> for ProtocolError {
	fn from(e: Box<dyn std::error::Error + Send + Sync>) -> Self {
		ProtocolError::Other(e.to_string())
	}
}

// vim: ts=4
