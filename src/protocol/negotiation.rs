//! Protocol negotiation message types
//!
//! Defines message types for the multi-version protocol negotiation protocol.
//! This allows clients and servers to exchange capabilities and select a common version.

use super::error::ProtocolError;

/// Protocol versions supported by this implementation
pub const SUPPORTED_VERSIONS: &[u32] = &[3];

/// Generate server capabilities announcement message
pub fn server_capabilities_message() -> String {
	format!(
		"SyNcR:{}",
		SUPPORTED_VERSIONS.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")
	)
}

/// Check if a protocol version is supported by this implementation
pub fn is_version_supported(version: u32) -> bool {
	SUPPORTED_VERSIONS.contains(&version)
}

/// Client announcement of supported protocol versions
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ClientCapabilities {
	pub versions: Vec<u32>,
}

impl ClientCapabilities {
	/// Create from a list of versions
	#[allow(dead_code)]
	pub fn new(versions: Vec<u32>) -> Self {
		Self { versions }
	}

	/// Parse from wire format: "SyNcR:2,3"
	#[allow(dead_code)]
	pub fn parse(s: &str) -> Result<Self, ProtocolError> {
		if !s.starts_with("SyNcR:") {
			return Err(ProtocolError::InvalidVersionFormat(format!(
				"Expected SyNcR: prefix, got: {}",
				s
			)));
		}

		let versions_str = &s[6..]; // Skip "SyNcR:"
		let versions: Result<Vec<u32>, _> =
			versions_str.split(',').map(|v| v.trim().parse::<u32>()).collect();

		match versions {
			Ok(v) if !v.is_empty() => Ok(Self { versions: v }),
			Ok(_) => Err(ProtocolError::InvalidVersionFormat("Version list is empty".to_string())),
			Err(e) => {
				Err(ProtocolError::InvalidVersionFormat(format!("Failed to parse versions: {}", e)))
			}
		}
	}
}

impl std::fmt::Display for ClientCapabilities {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(
			f,
			"SyNcR:{}",
			self.versions.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")
		)
	}
}

/// Server announcement of supported protocol versions
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ServerCapabilities {
	pub versions: Vec<u32>,
}

impl ServerCapabilities {
	/// Create from a list of versions
	#[allow(dead_code)]
	pub fn new(versions: Vec<u32>) -> Self {
		Self { versions }
	}

	/// Parse from wire format: "CAPS:2,3"
	#[allow(dead_code)]
	pub fn parse(s: &str) -> Result<Self, ProtocolError> {
		if !s.starts_with("CAPS:") {
			return Err(ProtocolError::InvalidVersionFormat(format!(
				"Expected CAPS: prefix, got: {}",
				s
			)));
		}

		let versions_str = &s[5..]; // Skip "CAPS:"
		let versions: Result<Vec<u32>, _> =
			versions_str.split(',').map(|v| v.trim().parse::<u32>()).collect();

		match versions {
			Ok(v) if !v.is_empty() => Ok(Self { versions: v }),
			Ok(_) => Err(ProtocolError::InvalidVersionFormat("Version list is empty".to_string())),
			Err(e) => {
				Err(ProtocolError::InvalidVersionFormat(format!("Failed to parse versions: {}", e)))
			}
		}
	}
}

impl std::fmt::Display for ServerCapabilities {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(
			f,
			"CAPS:{}",
			self.versions.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")
		)
	}
}

/// Client command to use a specific protocol version
#[derive(Debug, Clone)]
pub struct VersionSelection {
	pub version: u32,
}

impl VersionSelection {
	/// Create a version selection
	pub fn new(version: u32) -> Self {
		Self { version }
	}

	/// Parse from wire format: "USE:3"
	#[allow(dead_code)]
	pub fn parse(s: &str) -> Result<Self, ProtocolError> {
		if !s.starts_with("USE:") {
			return Err(ProtocolError::InvalidVersionFormat(format!(
				"Expected USE: prefix, got: {}",
				s
			)));
		}

		let version_str = &s[4..]; // Skip "USE:"
		match version_str.trim().parse::<u32>() {
			Ok(v) => Ok(Self { version: v }),
			Err(e) => Err(ProtocolError::InvalidVersionFormat(format!(
				"Failed to parse version number: {}",
				e
			))),
		}
	}
}

impl std::fmt::Display for VersionSelection {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "USE:{}", self.version)
	}
}

/// Server acknowledgment of version selection
#[derive(Debug, Clone)]
pub struct ReadyAck {
	pub version: Option<u32>,
}

impl ReadyAck {
	/// Create a ready acknowledgment (optional with version echo)
	#[allow(dead_code)]
	pub fn new(version: Option<u32>) -> Self {
		Self { version }
	}

	/// Parse from wire format: "READY" or "READY:3"
	pub fn parse(s: &str) -> Result<Self, ProtocolError> {
		if s == "READY" {
			return Ok(Self { version: None });
		}

		if let Some(version_str) = s.strip_prefix("READY:") {
			match version_str.trim().parse::<u32>() {
				Ok(v) => Ok(Self { version: Some(v) }),
				Err(e) => Err(ProtocolError::InvalidVersionFormat(format!(
					"Failed to parse version number in READY: {}",
					e
				))),
			}
		} else {
			Err(ProtocolError::InvalidVersionFormat(format!(
				"Expected READY or READY:version, got: {}",
				s
			)))
		}
	}
}

impl std::fmt::Display for ReadyAck {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self.version {
			Some(v) => write!(f, "READY:{}", v),
			None => write!(f, "READY"),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	// ─── ClientCapabilities Tests ───

	#[test]
	fn test_client_capabilities_serialize() {
		let cap = ClientCapabilities::new(vec![2, 3]);
		assert_eq!(cap.to_string(), "SyNcR:2,3");
	}

	#[test]
	fn test_client_capabilities_parse() {
		let cap = ClientCapabilities::parse("SyNcR:2,3").unwrap();
		assert_eq!(cap.versions, vec![2, 3]);
	}

	#[test]
	fn test_client_capabilities_parse_single() {
		let cap = ClientCapabilities::parse("SyNcR:3").unwrap();
		assert_eq!(cap.versions, vec![3]);
	}

	#[test]
	fn test_client_capabilities_parse_with_spaces() {
		let cap = ClientCapabilities::parse("SyNcR: 2 , 3 ").unwrap();
		assert_eq!(cap.versions, vec![2, 3]);
	}

	#[test]
	fn test_client_capabilities_roundtrip() {
		let original = vec![2, 3, 4];
		let cap = ClientCapabilities::new(original.clone());
		let serialized = cap.to_string();
		let cap2 = ClientCapabilities::parse(&serialized).unwrap();
		assert_eq!(cap2.versions, original);
	}

	// ─── ServerCapabilities Tests ───

	#[test]
	fn test_server_capabilities_serialize() {
		let cap = ServerCapabilities::new(vec![3]);
		assert_eq!(cap.to_string(), "CAPS:3");
	}

	#[test]
	fn test_server_capabilities_parse() {
		let cap = ServerCapabilities::parse("CAPS:2,3").unwrap();
		assert_eq!(cap.versions, vec![2, 3]);
	}

	#[test]
	fn test_server_capabilities_parse_single() {
		let cap = ServerCapabilities::parse("CAPS:2").unwrap();
		assert_eq!(cap.versions, vec![2]);
	}

	#[test]
	fn test_server_capabilities_roundtrip() {
		let original = vec![1, 2, 3];
		let cap = ServerCapabilities::new(original.clone());
		let serialized = cap.to_string();
		let cap2 = ServerCapabilities::parse(&serialized).unwrap();
		assert_eq!(cap2.versions, original);
	}

	// ─── VersionSelection Tests ───

	#[test]
	fn test_version_selection_serialize() {
		let sel = VersionSelection::new(3);
		assert_eq!(sel.to_string(), "USE:3");
	}

	#[test]
	fn test_version_selection_parse() {
		let sel = VersionSelection::parse("USE:2").unwrap();
		assert_eq!(sel.version, 2);
	}

	#[test]
	fn test_version_selection_roundtrip() {
		let original = 5;
		let sel = VersionSelection::new(original);
		let serialized = sel.to_string();
		let sel2 = VersionSelection::parse(&serialized).unwrap();
		assert_eq!(sel2.version, original);
	}

	// ─── ReadyAck Tests ───

	#[test]
	fn test_ready_ack_serialize_no_version() {
		let ack = ReadyAck::new(None);
		assert_eq!(ack.to_string(), "READY");
	}

	#[test]
	fn test_ready_ack_serialize_with_version() {
		let ack = ReadyAck::new(Some(3));
		assert_eq!(ack.to_string(), "READY:3");
	}

	#[test]
	fn test_ready_ack_parse_no_version() {
		let ack = ReadyAck::parse("READY").unwrap();
		assert_eq!(ack.version, None);
	}

	#[test]
	fn test_ready_ack_parse_with_version() {
		let ack = ReadyAck::parse("READY:2").unwrap();
		assert_eq!(ack.version, Some(2));
	}

	#[test]
	fn test_ready_ack_roundtrip_no_version() {
		let original = ReadyAck::new(None);
		let serialized = original.to_string();
		let ack2 = ReadyAck::parse(&serialized).unwrap();
		assert_eq!(ack2.version, None);
	}

	#[test]
	fn test_ready_ack_roundtrip_with_version() {
		let original = ReadyAck::new(Some(42));
		let serialized = original.to_string();
		let ack2 = ReadyAck::parse(&serialized).unwrap();
		assert_eq!(ack2.version, Some(42));
	}

	// ─── Error Cases ───

	#[test]
	fn test_invalid_client_caps_prefix() {
		assert!(ClientCapabilities::parse("INVALID:2,3").is_err());
	}

	#[test]
	fn test_invalid_client_caps_empty() {
		assert!(ClientCapabilities::parse("SyNcR:").is_err());
	}

	#[test]
	fn test_invalid_server_caps_prefix() {
		assert!(ServerCapabilities::parse("INVALID:2,3").is_err());
	}

	#[test]
	fn test_invalid_server_caps_empty() {
		assert!(ServerCapabilities::parse("CAPS:").is_err());
	}

	#[test]
	fn test_invalid_version_selection_prefix() {
		assert!(VersionSelection::parse("WRONG:2").is_err());
	}

	#[test]
	fn test_invalid_version_selection_not_number() {
		assert!(VersionSelection::parse("USE:abc").is_err());
	}

	#[test]
	fn test_invalid_ready_ack() {
		assert!(ReadyAck::parse("NOTREADY").is_err());
	}

	#[test]
	fn test_invalid_ready_ack_bad_version() {
		assert!(ReadyAck::parse("READY:abc").is_err());
	}

	// ─── Complex Version Lists ───

	#[test]
	fn test_many_versions() {
		let cap = ClientCapabilities::new(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
		let serialized = cap.to_string();
		assert_eq!(serialized, "SyNcR:1,2,3,4,5,6,7,8,9,10");
		let cap2 = ClientCapabilities::parse(&serialized).unwrap();
		assert_eq!(cap2.versions, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
	}

	// ─── Supported Versions Tests ───

	#[test]
	fn test_supported_versions_constant() {
		assert!(!SUPPORTED_VERSIONS.contains(&2));
		assert!(SUPPORTED_VERSIONS.contains(&3));
		assert!(!SUPPORTED_VERSIONS.contains(&1));
		assert!(!SUPPORTED_VERSIONS.contains(&4));
	}

	#[test]
	fn test_is_version_supported() {
		assert!(!is_version_supported(2));
		assert!(is_version_supported(3));
		assert!(!is_version_supported(1));
		assert!(!is_version_supported(4));
		assert!(!is_version_supported(0));
		assert!(!is_version_supported(100));
	}

	#[test]
	fn test_server_capabilities_message() {
		let msg = server_capabilities_message();
		assert!(msg.starts_with("SyNcR:"));
		assert!(!msg.contains("2"));
		assert!(msg.contains("3"));
		// Message should match the format exactly
		assert_eq!(msg, "SyNcR:3");
	}
}

// vim: ts=4
