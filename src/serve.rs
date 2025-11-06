use base64::{engine::general_purpose, Engine as _};
use rollsum::Bup;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::error::Error;
use std::os::unix::{fs::MetadataExt, prelude::PermissionsExt};
use std::{env, fs, io, path, pin::Pin};
use tokio::fs as afs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
//use std::{thread, time};

use crate::config;
use crate::logging::*;
#[allow(unused_imports)]
use crate::metadata_utils;
use crate::protocol_utils;
use crate::types::{FileChunk, HashChunk};
use crate::util;

/// Protocol version for handshake and compatibility checking
const PROTOCOL_VERSION: u8 = 1;

// Type alias for complex async function return type
type BoxedAsyncResult<'a> =
	Pin<Box<dyn std::future::Future<Output = Result<(), Box<dyn Error>>> + 'a>>;

/// Macro for resilient protocol output - ignores broken pipe errors
/// This prevents child processes from panicking when parent closes the pipe
macro_rules! protocol_println {
	($($arg:tt)*) => {{
		use std::io::Write;
		let result = writeln!(std::io::stdout(), $($arg)*);
		match result {
			Ok(_) => {
				// Flush to ensure message is sent immediately
				let _ = std::io::stdout().flush();
			}
			Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
				// Parent closed pipe or not reading yet - silently ignore
				// Child will be stopped via QUIT command
			}
			Err(e) => {
				// Other errors - continue but log to stderr
				eprintln!("Protocol write error: {}", e);
			}
		}
	}};
}

///////////
// Utils //
///////////

/// Helper function for resilient stdout writes
fn write_stdout(data: &[u8]) -> io::Result<()> {
	use std::io::Write;
	let mut stdout = io::stdout();
	match stdout.write_all(data) {
		Ok(()) => {
			stdout.flush()?;
			Ok(())
		}
		Err(e) if e.kind() == io::ErrorKind::BrokenPipe => {
			// Parent closed pipe or not reading yet - pretend success
			Ok(())
		}
		Err(e) => Err(e),
	}
}

/// Validate that a path is safe to use (prevents directory traversal/injection attacks)
/// Only allows relative paths without parent directory references
fn validate_path(path: &path::Path) -> Result<(), Box<dyn Error>> {
	// Reject absolute paths
	if path.is_absolute() {
		return Err("Absolute paths are not allowed".into());
	}

	// Reject paths with parent directory components (..)
	for component in path.components() {
		if component == std::path::Component::ParentDir {
			return Err("Parent directory traversal (..) is not allowed".into());
		}
	}

	// Reject current directory references (.) - they're redundant and could be suspicious
	if path.as_os_str().is_empty() {
		return Err("Empty paths are not allowed".into());
	}

	// Additional check: reject paths that are exactly "."
	if path == path::Path::new(".") {
		return Err("Current directory reference (.) is not allowed".into());
	}

	Ok(())
}

#[allow(dead_code)]
fn tmp_filename(path: &path::Path) -> path::PathBuf {
	let mut filepath = path::PathBuf::from(path);
	let mut filename = path.file_name().expect("Protocol error!").to_os_string();
	filename.push(".SyNcR-TmP");
	filepath.set_file_name(filename);
	filepath
}

//////////
// List //
//////////
pub struct DumpState {
	pub exclude: Vec<glob::Pattern>,
	pub chunks: BTreeMap<String, Vec<FileChunk>>,
	pub missing: RefCell<BTreeMap<String, Vec<FileChunk>>>,
	pub rename: RefCell<BTreeMap<path::PathBuf, path::PathBuf>>,
}

impl DumpState {
	fn add_chunk(&mut self, hash: String, path: path::PathBuf, offset: u64, size: usize) {
		let v = self.chunks.entry(hash).or_default();
		if !v.iter().any(|p| p.path == path) {
			v.push(FileChunk { path, offset, size });
		}
	}

	async fn read_chunk(
		&self,
		dir: &path::Path,
		hash: &str,
	) -> Result<Option<Vec<u8>>, Box<dyn Error>> {
		let fc_vec_opt = self.chunks.get(hash);

		match fc_vec_opt {
			Some(fc_vec) => {
				let fc = &fc_vec[0];
				let path = dir.join(&fc.path);
				let mut f = afs::File::open(&path).await?;
				let mut buf: Vec<u8> = vec![0; fc.size];
				f.seek(io::SeekFrom::Start(fc.offset)).await?;
				f.read_exact(&mut buf).await?;

				// Verify hash to detect corruption
				let computed_hash = util::hash(&buf);
				if computed_hash != hash {
					return Err(format!(
						"Hash mismatch for chunk {}: expected {}, got {}",
						hash, hash, computed_hash
					)
					.into());
				}

				Ok(Some(buf))
			}
			None => Ok(None),
		}
	}

	async fn write_chunk(
		&self,
		path: &path::Path,
		chunk: &HashChunk,
		buf: &[u8],
	) -> Result<(), Box<dyn Error>> {
		// Verify hash before writing to detect corruption during transfer
		let computed_hash = util::hash_binary(buf);
		if computed_hash != chunk.hash {
			let expected_b64 = crate::util::hash_to_base64(&chunk.hash);
			let computed_b64 = crate::util::hash_to_base64(&computed_hash);
			return Err(format!(
				"Hash mismatch for chunk: expected {}, got {}",
				expected_b64, computed_b64
			)
			.into());
		}

		let mut f = afs::OpenOptions::new()
			.write(true)
			.create(true)
			.truncate(false)
			.open(&path)
			.await?;
		f.seek(io::SeekFrom::Start(chunk.offset)).await?;
		f.write_all(buf).await?;
		Ok(())
	}
}

fn traverse_dir<'a>(state: &'a mut DumpState, dir: path::PathBuf) -> BoxedAsyncResult<'a> {
	Box::pin(async move {
		for entry in fs::read_dir(&dir)? {
			let entry = entry?;
			let path = entry.path();
			if state.exclude[0].matches_path(&path) {
				continue;
			}

			let meta = fs::symlink_metadata(&path)?;

			if meta.is_file() {
				protocol_println!(
					"F:{}:{}:{}:{}:{}:{}:{}",
					&path.to_string_lossy(),
					meta.mode(),
					meta.uid(),
					meta.gid(),
					meta.ctime(),
					meta.mtime(),
					meta.size()
				);

				let mut f = afs::File::open(&path).await?;
				let mut buf: Vec<u8> = vec![0; config::MAX_CHUNK_SIZE];

				let mut n = f.read(&mut buf).await?;

				let mut offset: u64 = 0;
				//let mut bup = Bup::new_with_chunk_bits(config::CHUNK_BITS);
				while n > 0 {
					let mut bup = Bup::new_with_chunk_bits(config::CHUNK_BITS);
					let mut endofs = config::MAX_CHUNK_SIZE;
					if endofs > n {
						endofs = n
					}
					if let Some((count, _hash)) = bup.find_chunk_edge(&buf[..endofs]) {
						let h = util::hash(&buf[..count]);
						protocol_println!("C:{}:{}:{}", offset, count, &h);
						//state.chunks.insert(h, path.clone());
						state.add_chunk(h, path.clone(), offset, count);
						unsafe {
							std::ptr::copy(buf[count..].as_mut_ptr(), buf.as_mut_ptr(), n - count);
						}
						offset += count as u64;
						n -= count;
					} else {
						let count = endofs;
						let h = util::hash(&buf[..count]);
						protocol_println!("C:{}:{}:{}", offset, count, &h);
						//state.chunks.insert(h, path.clone());
						state.add_chunk(h, path.clone(), offset, count);
						offset += count as u64;
						n -= count;
					}
					n += f.read(&mut buf[n..]).await?;
				}
			}
			if meta.file_type().is_symlink() {
				let target = match fs::read_link(&path) {
					Ok(target_path) => target_path.to_string_lossy().to_string(),
					Err(_) => String::new(),
				};
				protocol_println!(
					"L:{}:{}:{}:{}:{}:{}:{}",
					&path.to_string_lossy(),
					meta.mode(),
					meta.uid(),
					meta.gid(),
					meta.ctime(),
					meta.mtime(),
					target
				);
			}
			if meta.is_dir() {
				protocol_println!(
					"D:{}:{}:{}:{}:{}:{}",
					&path.to_string_lossy(),
					meta.mode(),
					meta.uid(),
					meta.gid(),
					meta.ctime(),
					meta.mtime()
				);
				//protocol_println!("D:{}:{}:{}", path.to_str().unwrap(), meta.uid(), meta.gid());
				traverse_dir(state, path).await?
			}
		}
		Ok(())
	})
}

pub fn serve_list(dir: path::PathBuf) -> Result<DumpState, Box<dyn Error>> {
	let mut state = DumpState {
		exclude: vec![glob::Pattern::new("**/*.SyNcR-TmP")?],
		chunks: BTreeMap::new(),
		missing: RefCell::new(BTreeMap::new()),
		rename: RefCell::new(BTreeMap::new()),
	};
	tokio::task::block_in_place(|| {
		tokio::runtime::Handle::current().block_on(traverse_dir(&mut state, dir))
	})?;

	protocol_println!(".");
	Ok(state)
}

async fn serve_read(dir: path::PathBuf, dump_state: &DumpState) -> Result<(), Box<dyn Error>> {
	let mut chunks: Vec<String> = Vec::new();
	let mut buf = String::new();
	loop {
		buf.clear();
		io::stdin()
			.read_line(&mut buf)
			.map_err(|e| format!("Failed to read chunk hashes: {}", e))?;
		if buf.trim() == "." {
			break;
		}
		chunks.push(String::from(buf.trim()));
	}

	for chunk in &chunks {
		let &fc_vec_opt = &dump_state.chunks.get(chunk);

		if let Some(fc_vec) = &fc_vec_opt {
			let fc = &fc_vec[0];
			let path = dir.join(&fc.path);
			let mut f = afs::File::open(&path).await?;
			let mut buf: Vec<u8> = vec![0; fc.size];
			f.seek(io::SeekFrom::Start(fc.offset)).await?;
			f.read_exact(&mut buf).await?;
			let encoded = general_purpose::STANDARD.encode(buf);
			protocol_println!("C:{}", chunk);
			for line in encoded.into_bytes().chunks(config::BASE64_LINE_LENGTH) {
				write_stdout(line)?;
				write_stdout(b"\n")?;
			}
			protocol_println!(".");
		}
	}
	protocol_println!(".");
	Ok(())
}

async fn serve_write(dir: path::PathBuf, dump_state: &DumpState) -> Result<(), Box<dyn Error>> {
	let mut buf = String::new();

	let mut file: Option<afs::File> = None;
	let mut filepath = path::PathBuf::from("");
	loop {
		buf.clear();
		io::stdin().read_line(&mut buf)?;

		let fields = protocol_utils::parse_protocol_line(&buf, 1)?;
		let cmd = fields[0];

		match cmd {
			"DEL" => {
				// Delete command: remove file from node
				let fields = protocol_utils::parse_protocol_line(&buf, 2)?;
				let path = path::PathBuf::from(fields[1]);

				// Validate path to prevent directory traversal attacks
				validate_path(&path)?;

				debug!("DELETE: {:?}", &path);

				// Remove the file
				if afs::metadata(&path).await.is_ok() {
					if afs::metadata(&path).await?.is_file() {
						afs::remove_file(&path).await?;
					} else if afs::metadata(&path).await?.is_dir() {
						afs::remove_dir_all(&path).await?;
					}
				}
			}
			"L" => {
				// Symlink command: create a symlink
				let fd = metadata_utils::parse_symlink_metadata(&buf)?;
				let path = fd.path.clone();
				validate_path(&path)?;

				// Remove existing file if it exists
				if afs::metadata(&path).await.is_ok() {
					afs::remove_file(&path).await.ok();
				}

				// Create the symlink with the target
				if let Some(target) = &fd.target {
					if let Err(e) = afs::symlink(target, &path).await {
						error!("Failed to create symlink {:?} -> {:?}: {}", path, target, e);
					}
				} else {
					warn!("Symlink {:?} has no target", path);
				}
			}
			"FM" | "FD" => {
				let fd = metadata_utils::parse_file_metadata(&buf)?;
				let path = fd.path.clone();

				// Validate path to prevent directory traversal attacks
				validate_path(&path)?;
				if cmd == "FD" {
					filepath = path.clone();
					let filename = path.file_name().ok_or("Path has no filename")?.to_os_string();
					let mut tmp_filename = filename;
					tmp_filename.push(".SyNcR-TmP");
					filepath.set_file_name(tmp_filename);
					//eprintln!("CREATE {:?}", &filepath);
					file = Some(afs::File::create(&filepath).await?);
					afs::set_permissions(&filepath, std::fs::Permissions::from_mode(fd.mode))
						.await?;
					dump_state.rename.borrow_mut().insert(filepath.clone(), path.clone());
				}
			}
			"D" => {
				let fd = metadata_utils::parse_dir_metadata(&buf)?;
				let path = fd.path.clone();
				filepath = path.clone();
				let filename = path.file_name().ok_or("Path has no filename")?;
				// FIXME: Should create temp dir, but then all paths must be altered!
				//filename.push(".SyNcR-TmP");
				filepath.set_file_name(filename);
				debug!("MKDIR {:?}", &filepath);
				// Try to create directory, but ignore AlreadyExists error
				match afs::create_dir(&filepath).await {
					Ok(_) => {}
					Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
						debug!("Directory already exists: {:?}", &filepath);
					}
					Err(e) => return Err(e.into()),
				}
				afs::set_permissions(&filepath, std::fs::Permissions::from_mode(fd.mode)).await?;
				dump_state.rename.borrow_mut().insert(filepath.clone(), path.clone());
			}
			"LC" | "RC" => {
				let fields = protocol_utils::parse_protocol_line(&buf, 4)?;
				if file.is_none() {
					return Err("Protocol error: chunk command without file".into());
				}
				let hash = crate::util::base64_to_hash(fields[3])
					.map_err(|e| format!("Invalid hash: {}", e))?;
				let hc = HashChunk {
					hash,
					offset: fields[1]
						.parse()
						.map_err(|e| format!("Invalid offset '{}': {}", fields[1], e))?,
					size: fields[2]
						.parse::<u32>()
						.map_err(|e| format!("Invalid size '{}': {}", fields[2], e))?,
				};
				if cmd == "LC" {
					// Local chunk, copy it locally
					let buf = dump_state
						.read_chunk(&dir, fields[3])
						.await?
						.ok_or_else(|| format!("Chunk not found: {}", fields[3]))?;
					if let Err(e) = dump_state.write_chunk(&filepath, &hc, &buf).await {
						protocol_println!("ERROR {}", e);
					}
				} else {
					// Remote chunk, add to wait list
					let mut missing = dump_state.missing.borrow_mut();
					let v = missing.entry(String::from(fields[3])).or_default();
					v.push(FileChunk {
						path: filepath.clone(),
						offset: fields[1]
							.parse()
							.map_err(|e| format!("Invalid offset '{}': {}", fields[1], e))?,
						size: fields[2]
							.parse()
							.map_err(|e| format!("Invalid size '{}': {}", fields[2], e))?,
					});
				}
			}
			"C" => {
				let fields = protocol_utils::parse_protocol_line(&buf, 2)?;
				let mut buf = String::new();
				let hash_b64 = fields[1];
				let hash = crate::util::base64_to_hash(hash_b64)
					.map_err(|e| format!("Invalid hash: {}", e))?;
				let mut chunk: Vec<u8> = Vec::new();
				loop {
					buf.clear();
					io::stdin().read_line(&mut buf)?;
					if buf.trim() == "." {
						break;
					}
					//eprintln!("DECODE: [{:?}]", &buf.trim());
					chunk.append(&mut general_purpose::STANDARD.decode(buf.trim())?);
				}
				//eprintln!("DECODED CHUNK: {:?}", chunk);
				let fc_vec_opt = {
					let missing = dump_state.missing.borrow();
					missing.get(hash_b64).cloned()
				};
				if let Some(fc_vec) = fc_vec_opt {
					for fc in fc_vec {
						let hc = HashChunk { hash, offset: fc.offset, size: fc.size as u32 };
						//let filepath = tmp_filename(&fc.path);
						if let Err(e) = dump_state.write_chunk(&fc.path, &hc, &chunk).await {
							error!("ERROR WRITING {}", e);
						}
					}
					dump_state.missing.borrow_mut().remove(hash_b64);
				}
			}
			"." => {
				if file.is_some() {
					file = None;
				} else {
					break;
				}
			}
			_ => return Err(format!("Unknown command: {}", cmd).into()),
		}
	}
	protocol_println!("OK");
	Ok(())
}

async fn serve_commit(
	_fixme_dir: path::PathBuf,
	dump_state: &DumpState,
) -> Result<(), Box<dyn Error>> {
	{
		let missing = dump_state.missing.borrow();
		if !missing.is_empty() {
			let missing_hashes: Vec<&String> = missing.keys().collect();
			error!("Cannot commit - {} missing chunks", missing.len());
			for hash in &missing_hashes {
				error!("  Missing chunk: {}", hash);
			}
			protocol_println!("ERROR:Cannot commit with {} missing chunks", missing.len());
			return Err(format!("Cannot commit: {} chunks still missing", missing.len()).into());
		}
	}

	let renames_to_do: Vec<(path::PathBuf, path::PathBuf)> = {
		let rename = dump_state.rename.borrow();
		rename.iter().map(|(src, dst)| (src.clone(), dst.clone())).collect()
	};

	for (src, dst) in renames_to_do {
		//eprintln!("RENAME: {:?} -> {:?}", src, dst);
		afs::rename(&src, &dst).await?;
		//fs::rename(&src, &dst)?;
	}
	protocol_println!("OK");
	Ok(())
}

// Clean up orphaned temporary files from interrupted syncs
fn cleanup_temp_files(dir: &path::Path) -> Result<(), Box<dyn Error>> {
	info!("Cleaning up orphaned temporary files...");
	let mut count = 0;

	// Walk directory tree looking for .SyNcR-TmP files (synchronous version)
	fn scan_dir(dir: &path::Path, count: &mut u32) -> Result<(), Box<dyn Error>> {
		for entry in fs::read_dir(dir)? {
			let entry = entry?;
			let path = entry.path();
			let metadata = fs::symlink_metadata(&path)?;

			if let Some(name) = path.file_name() {
				if let Some(name_str) = name.to_str() {
					if name_str.ends_with(".SyNcR-TmP") {
						debug!("Removing orphaned temp file: {:?}", path);
						let remove_result = if metadata.is_file() {
							fs::remove_file(&path)
						} else if metadata.is_dir() {
							fs::remove_dir_all(&path)
						} else {
							Ok(())
						};

						// Ignore "not found" errors - file may have been deleted already
						match remove_result {
							Ok(_) => *count += 1,
							Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
								debug!("Temp file already removed: {:?}", path);
							}
							Err(e) => return Err(e.into()),
						}
					}
				}
			}

			// Recursively scan subdirectories
			if metadata.is_dir() {
				scan_dir(&path, count)?;
			}
		}
		Ok(())
	}

	scan_dir(dir, &mut count)?;
	info!("Cleaned up {} temporary files", count);
	Ok(())
}

pub fn serve(dir: &str) -> Result<(), Box<dyn Error>> {
	env::set_current_dir(dir)?;

	// Clean up orphaned temp files from any interrupted previous syncs
	cleanup_temp_files(path::Path::new("."))?;

	// Signal that we're ready to receive commands (do NOT send VERSION here,
	// wait for the client to initiate the handshake with VERSION)
	use std::io::Write;
	writeln!(std::io::stdout(), ".")?;
	std::io::stdout().flush()?;

	let mut dump_state: Option<DumpState> = None;

	loop {
		let mut cmdline = String::new();
		match io::stdin().read_line(&mut cmdline) {
			Ok(0) => break, // EOF reached
			Ok(_) => {}
			Err(e) => {
				error!("Failed to read command: {}", e);
				break;
			}
		}

		match cmdline.trim() {
			cmd if cmd.starts_with("VERSION") => {
				// Handle version handshake
				let version = if let Some(v) = cmd.strip_prefix("VERSION:") {
					v.parse::<u8>().unwrap_or(0)
				} else {
					0
				};

				if version == PROTOCOL_VERSION {
					protocol_println!("VERSION:{}", PROTOCOL_VERSION);
				} else {
					error!(
						"Protocol version mismatch: client={}, server={}",
						version, PROTOCOL_VERSION
					);
					protocol_println!(
						"ERROR:Protocol version mismatch: expected {}, got {}",
						PROTOCOL_VERSION,
						version
					);
				}
			}
			"LIST" => dump_state = Some(serve_list(path::PathBuf::from("."))?),
			"READ" => match &dump_state {
				Some(state) => {
					tokio::task::block_in_place(|| {
						tokio::runtime::Handle::current()
							.block_on(serve_read(path::PathBuf::from("."), state))
					})?;
				}
				None => {
					protocol_println!("!Use LIST command first!");
				}
			},
			"WRITE" => match &dump_state {
				Some(state) => {
					tokio::task::block_in_place(|| {
						tokio::runtime::Handle::current()
							.block_on(serve_write(path::PathBuf::from("."), state))
					})?;
				}
				None => {
					protocol_println!("!Use LIST command first!");
				}
			},
			"COMMIT" => match &dump_state {
				Some(state) => {
					tokio::task::block_in_place(|| {
						tokio::runtime::Handle::current()
							.block_on(serve_commit(path::PathBuf::from("."), state))
					})?;
				}
				None => {
					protocol_println!("!Use LIST command first!");
				}
			},
			"QUIT" => break,
			"." => {
				// Dot command outside of WRITE mode - ignore it
				// This is sent by cleanup to ensure we exit any active WRITE sessions
			}
			_ => protocol_println!("E:UNK-CMD: Unknown command: {}", &cmdline.trim()),
		}
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_validate_path_allows_relative_paths() {
		assert!(validate_path(path::Path::new("file.txt")).is_ok());
		assert!(validate_path(path::Path::new("dir/file.txt")).is_ok());
		assert!(validate_path(path::Path::new("a/b/c/d.txt")).is_ok());
	}

	#[test]
	fn test_validate_path_rejects_absolute_paths() {
		assert!(validate_path(path::Path::new("/etc/passwd")).is_err());
		assert!(validate_path(path::Path::new("/home/user/file.txt")).is_err());
		assert!(validate_path(path::Path::new("/")).is_err());
	}

	#[test]
	fn test_validate_path_rejects_parent_traversal() {
		assert!(validate_path(path::Path::new("../etc/passwd")).is_err());
		assert!(validate_path(path::Path::new("file/../../../etc/passwd")).is_err());
		assert!(validate_path(path::Path::new("..")).is_err());
		assert!(validate_path(path::Path::new("dir/..")).is_err());
	}

	#[test]
	fn test_validate_path_rejects_current_dir() {
		assert!(validate_path(path::Path::new(".")).is_err());
	}

	#[test]
	fn test_validate_path_rejects_empty_paths() {
		assert!(validate_path(path::Path::new("")).is_err());
	}
}

// vim: ts=4
