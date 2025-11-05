use std::error::Error;
use std::path;

use crate::protocol_utils;
use crate::types::{FileChunk, FileData, FileType, HashChunk};

/// Parse file metadata from F: protocol line
/// Format: F:path:mode:user:group:ctime:mtime:size
/// Returns: FileData with FileType::File
#[allow(dead_code)]
pub fn parse_file_metadata(buf: &str) -> Result<Box<FileData>, Box<dyn Error>> {
	let fields = protocol_utils::parse_protocol_line(buf, 8)?;
	let path = path::PathBuf::from(fields[1]);

	let fd = Box::new(FileData {
		tp: FileType::File,
		path: path.clone(),
		mode: fields[2].parse().map_err(|e| format!("Invalid mode '{}': {}", fields[2], e))?,
		user: fields[3].parse().map_err(|e| format!("Invalid user '{}': {}", fields[3], e))?,
		group: fields[4].parse().map_err(|e| format!("Invalid group '{}': {}", fields[4], e))?,
		ctime: fields[5].parse().map_err(|e| format!("Invalid ctime '{}': {}", fields[5], e))?,
		mtime: fields[6].parse().map_err(|e| format!("Invalid mtime '{}': {}", fields[6], e))?,
		size: fields[7].parse().map_err(|e| format!("Invalid size '{}': {}", fields[7], e))?,
		chunks: vec![],
	});

	Ok(fd)
}

/// Parse directory metadata from D: protocol line
/// Format: D:path:mode:user:group:ctime:mtime
/// Returns: FileData with FileType::Dir and size=0
#[allow(dead_code)]
pub fn parse_dir_metadata(buf: &str) -> Result<Box<FileData>, Box<dyn Error>> {
	let fields = protocol_utils::parse_protocol_line(buf, 7)?;
	let path = path::PathBuf::from(fields[1]);

	let fd = Box::new(FileData {
		tp: FileType::Dir,
		path: path.clone(),
		mode: fields[2].parse().map_err(|e| format!("Invalid mode '{}': {}", fields[2], e))?,
		user: fields[3].parse().map_err(|e| format!("Invalid user '{}': {}", fields[3], e))?,
		group: fields[4].parse().map_err(|e| format!("Invalid group '{}': {}", fields[4], e))?,
		ctime: fields[5].parse().map_err(|e| format!("Invalid ctime '{}': {}", fields[5], e))?,
		mtime: fields[6].parse().map_err(|e| format!("Invalid mtime '{}': {}", fields[6], e))?,
		size: 0,
		chunks: vec![],
	});

	Ok(fd)
}

/// Parse symlink metadata from L: protocol line
/// Format: L:path:mode:user:group:ctime:mtime
/// Returns: FileData with FileType::SymLink and size=0
#[allow(dead_code)]
pub fn parse_symlink_metadata(buf: &str) -> Result<Box<FileData>, Box<dyn Error>> {
	let fields = protocol_utils::parse_protocol_line(buf, 7)?;
	let path = path::PathBuf::from(fields[1]);

	let fd = Box::new(FileData {
		tp: FileType::SymLink,
		path: path.clone(),
		mode: fields[2].parse().map_err(|e| format!("Invalid mode '{}': {}", fields[2], e))?,
		user: fields[3].parse().map_err(|e| format!("Invalid user '{}': {}", fields[3], e))?,
		group: fields[4].parse().map_err(|e| format!("Invalid group '{}': {}", fields[4], e))?,
		ctime: fields[5].parse().map_err(|e| format!("Invalid ctime '{}': {}", fields[5], e))?,
		mtime: fields[6].parse().map_err(|e| format!("Invalid mtime '{}': {}", fields[6], e))?,
		size: 0,
		chunks: vec![],
	});

	Ok(fd)
}

/// Parse chunk metadata from C: protocol line
/// Format: C:offset:size:hash
/// Returns: HashChunk for this chunk
#[allow(dead_code)]
pub fn parse_chunk_metadata(buf: &str) -> Result<HashChunk, Box<dyn Error>> {
	let fields = protocol_utils::parse_protocol_line(buf, 4)?;

	let hc = HashChunk {
		hash: String::from(fields[3]),
		offset: fields[1]
			.parse()
			.map_err(|e| format!("Invalid offset '{}': {}", fields[1], e))?,
		size: fields[2].parse().map_err(|e| format!("Invalid size '{}': {}", fields[2], e))?,
	};

	Ok(hc)
}

/// Parse file chunk metadata from LC:/RC: protocol line for missing chunks
/// Format: LC/RC:offset:size:hash
/// Returns: FileChunk to track where chunk should be written
#[allow(dead_code)]
pub fn parse_file_chunk_metadata(
	buf: &str,
	filepath: &path::Path,
) -> Result<FileChunk, Box<dyn Error>> {
	let fields = protocol_utils::parse_protocol_line(buf, 4)?;

	let fc = FileChunk {
		path: filepath.to_path_buf(),
		offset: fields[1]
			.parse()
			.map_err(|e| format!("Invalid offset '{}': {}", fields[1], e))?,
		size: fields[2].parse().map_err(|e| format!("Invalid size '{}': {}", fields[2], e))?,
	};

	Ok(fc)
}
