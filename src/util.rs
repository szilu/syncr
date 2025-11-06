use base64::engine::Engine;

/// Hash a buffer using BLAKE3 and return base64-encoded result
pub fn hash(buf: &[u8]) -> String {
	let hash = blake3::hash(buf);
	hash_to_base64(hash.as_bytes())
}

/// Hash a buffer using BLAKE3 and return binary result
pub fn hash_binary(buf: &[u8]) -> [u8; 32] {
	*blake3::hash(buf).as_bytes()
}

/// Convert binary hash to base64 string
pub fn hash_to_base64(hash: &[u8; 32]) -> String {
	base64::engine::general_purpose::STANDARD.encode(hash)
}

/// Convert base64 string to binary hash
pub fn base64_to_hash(b64: &str) -> Result<[u8; 32], Box<dyn std::error::Error>> {
	let bytes = base64::engine::general_purpose::STANDARD.decode(b64)?;
	if bytes.len() != 32 {
		return Err(format!("Hash must be 32 bytes, got {}", bytes.len()).into());
	}
	let mut hash = [0u8; 32];
	hash.copy_from_slice(&bytes);
	Ok(hash)
}

#[cfg(test)]
mod test {
	use super::*;

	#[test]
	fn test_hash_simple() {
		let src: [u8; 2] = [b'1', b'2'];
		let res = hash(&src);
		// BLAKE3 hashes are 44 base64 characters (32 bytes encoded)
		assert_eq!(res.len(), 44);
		// Verify the hash is consistent
		let res2 = hash(&src);
		assert_eq!(res, res2);
	}

	#[test]
	fn test_hash_empty() {
		let src: [u8; 0] = [];
		let res = hash(&src);
		// BLAKE3 hashes are 44 base64 characters
		assert_eq!(res.len(), 44);
		// Verify empty input produces consistent hash
		let res2 = hash(&src);
		assert_eq!(res, res2);
	}

	#[test]
	fn test_hash_longer_text() {
		let src = b"The quick brown fox jumps over the lazy dog";
		let res = hash(src);
		// BLAKE3 hashes are 44 base64 characters (32 bytes encoded)
		assert_eq!(res.len(), 44);
	}

	#[test]
	fn test_hash_binary() {
		let src: [u8; 4] = [0x00, 0xFF, 0xDE, 0xAD];
		let res = hash(&src);
		// BLAKE3 is 44 base64 chars
		assert_eq!(res.len(), 44);
	}

	#[test]
	fn test_hash_consistency() {
		let src = b"test data";
		let res1 = hash(src);
		let res2 = hash(src);
		assert_eq!(res1, res2, "Hash should be deterministic");
	}

	#[test]
	fn test_hash_different_inputs() {
		let src1 = b"test1";
		let src2 = b"test2";
		let res1 = hash(src1);
		let res2 = hash(src2);
		assert_ne!(res1, res2, "Different inputs should produce different hashes");
	}
}

// vim: ts=4
