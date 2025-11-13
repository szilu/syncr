use std::error::Error;
use std::path;

use crate::protocol_utils;
use crate::types::{FileData, FileType, HashChunk};

/// Parse file metadata from F: protocol line
/// Format: F:path:mode:user:group:ctime:mtime:size
/// Returns: FileData with FileType::File
#[allow(dead_code)]
pub fn parse_file_metadata(buf: &str) -> Result<Box<FileData>, Box<dyn Error>> {
	let fields = protocol_utils::parse_protocol_line(buf, 8)?;
	let path = path::PathBuf::from(fields[1]);

	let fd = Box::new(
		FileData::builder(FileType::File, path)
			.mode(fields[2].parse().map_err(|e| format!("Invalid mode '{}': {}", fields[2], e))?)
			.user(fields[3].parse().map_err(|e| format!("Invalid user '{}': {}", fields[3], e))?)
			.group(fields[4].parse().map_err(|e| format!("Invalid group '{}': {}", fields[4], e))?)
			.ctime(fields[5].parse().map_err(|e| format!("Invalid ctime '{}': {}", fields[5], e))?)
			.mtime(fields[6].parse().map_err(|e| format!("Invalid mtime '{}': {}", fields[6], e))?)
			.size(fields[7].parse().map_err(|e| format!("Invalid size '{}': {}", fields[7], e))?)
			.build(),
	);

	Ok(fd)
}

/// Parse directory metadata from D: protocol line
/// Format: D:path:mode:user:group:ctime:mtime
/// Returns: FileData with FileType::Dir and size=0
#[allow(dead_code)]
pub fn parse_dir_metadata(buf: &str) -> Result<Box<FileData>, Box<dyn Error>> {
	let fields = protocol_utils::parse_protocol_line(buf, 7)?;
	let path = path::PathBuf::from(fields[1]);

	let fd = Box::new(
		FileData::builder(FileType::Dir, path)
			.mode(fields[2].parse().map_err(|e| format!("Invalid mode '{}': {}", fields[2], e))?)
			.user(fields[3].parse().map_err(|e| format!("Invalid user '{}': {}", fields[3], e))?)
			.group(fields[4].parse().map_err(|e| format!("Invalid group '{}': {}", fields[4], e))?)
			.ctime(fields[5].parse().map_err(|e| format!("Invalid ctime '{}': {}", fields[5], e))?)
			.mtime(fields[6].parse().map_err(|e| format!("Invalid mtime '{}': {}", fields[6], e))?)
			.size(0)
			.build(),
	);

	Ok(fd)
}

/// Parse symlink metadata from L: protocol line
/// Format: L:path:mode:user:group:ctime:mtime:target
/// Returns: FileData with FileType::SymLink and size=0
#[allow(dead_code)]
pub fn parse_symlink_metadata(buf: &str) -> Result<Box<FileData>, Box<dyn Error>> {
	let fields = protocol_utils::parse_protocol_line(buf, 8)?;
	let path = path::PathBuf::from(fields[1]);
	let target = if fields[7].is_empty() { None } else { Some(path::PathBuf::from(fields[7])) };

	let fd = Box::new(
		FileData::builder(FileType::SymLink, path)
			.mode(fields[2].parse().map_err(|e| format!("Invalid mode '{}': {}", fields[2], e))?)
			.user(fields[3].parse().map_err(|e| format!("Invalid user '{}': {}", fields[3], e))?)
			.group(fields[4].parse().map_err(|e| format!("Invalid group '{}': {}", fields[4], e))?)
			.ctime(fields[5].parse().map_err(|e| format!("Invalid ctime '{}': {}", fields[5], e))?)
			.mtime(fields[6].parse().map_err(|e| format!("Invalid mtime '{}': {}", fields[6], e))?)
			.size(0)
			.target(target)
			.build(),
	);

	Ok(fd)
}

/// Parse chunk metadata from C: protocol line
/// Format: C:offset:size:hash (hash is base64-encoded BLAKE3)
/// Returns: HashChunk for this chunk
#[allow(dead_code)]
pub fn parse_chunk_metadata(buf: &str) -> Result<HashChunk, Box<dyn Error>> {
	let fields = protocol_utils::parse_protocol_line(buf, 4)?;

	let hash_b64 = fields[3];
	let hash =
		crate::util::base64_to_hash(hash_b64).map_err(|e| format!("Invalid hash base64: {}", e))?;

	let hc = HashChunk {
		hash,
		offset: fields[1]
			.parse()
			.map_err(|e| format!("Invalid offset '{}': {}", fields[1], e))?,
		size: fields[2].parse().map_err(|e| format!("Invalid size '{}': {}", fields[2], e))?,
	};

	Ok(hc)
}
#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_parse_symlink_metadata_with_target() {
		let buf = "L:link:511:1000:1000:1234567890:1234567890:target";
		let result = parse_symlink_metadata(buf);
		assert!(result.is_ok());

		let fd = result.unwrap();
		assert_eq!(fd.tp, FileType::SymLink);
		assert_eq!(fd.path, path::PathBuf::from("link"));
		assert_eq!(fd.mode, 511); // 0o777 in decimal
		assert_eq!(fd.user, 1000);
		assert_eq!(fd.group, 1000);
		assert_eq!(fd.ctime, 1234567890);
		assert_eq!(fd.mtime, 1234567890);
		assert_eq!(fd.size, 0);
		assert_eq!(fd.target, Some(path::PathBuf::from("target")));
		assert_eq!(fd.chunks.len(), 0);
	}

	#[test]
	fn test_parse_symlink_metadata_without_target() {
		let buf = "L:link:511:1000:1000:1234567890:1234567890:";
		let result = parse_symlink_metadata(buf);
		assert!(result.is_ok());

		let fd = result.unwrap();
		assert_eq!(fd.tp, FileType::SymLink);
		assert_eq!(fd.path, path::PathBuf::from("link"));
		assert_eq!(fd.target, None);
	}

	#[test]
	fn test_parse_symlink_metadata_relative_target() {
		let buf = "L:link:511:1000:1000:1234567890:1234567890:../target";
		let result = parse_symlink_metadata(buf);
		assert!(result.is_ok());

		let fd = result.unwrap();
		assert_eq!(fd.target, Some(path::PathBuf::from("../target")));
	}

	#[test]
	fn test_parse_symlink_metadata_absolute_target() {
		let buf = "L:link:511:1000:1000:1234567890:1234567890:/etc/config";
		let result = parse_symlink_metadata(buf);
		assert!(result.is_ok());

		let fd = result.unwrap();
		assert_eq!(fd.target, Some(path::PathBuf::from("/etc/config")));
	}

	#[test]
	fn test_parse_file_metadata() {
		let buf = "F:file.txt:420:1000:1000:1234567890:1234567890:1024";
		let result = parse_file_metadata(buf);
		assert!(result.is_ok());

		let fd = result.unwrap();
		assert_eq!(fd.tp, FileType::File);
		assert_eq!(fd.path, path::PathBuf::from("file.txt"));
		assert_eq!(fd.size, 1024);
		assert_eq!(fd.target, None);
	}

	#[test]
	fn test_parse_dir_metadata() {
		let buf = "D:mydir:493:1000:1000:1234567890:1234567890";
		let result = parse_dir_metadata(buf);
		assert!(result.is_ok());

		let fd = result.unwrap();
		assert_eq!(fd.tp, FileType::Dir);
		assert_eq!(fd.path, path::PathBuf::from("mydir"));
		assert_eq!(fd.size, 0);
		assert_eq!(fd.target, None);
	}
}

// vim: ts=4
