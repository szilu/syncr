use async_std::{fs as afs, prelude::*, task};
use base64::{engine::general_purpose, Engine as _};
use glob;
use rollsum::Bup;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::error::Error;
use std::io::Write;
use std::os::unix::{fs::MetadataExt, prelude::PermissionsExt};
use std::{env, fs, io, path, pin::Pin};
//use std::{thread, time};

use crate::config;
use crate::types::{FileChunk, FileData, FileType, HashChunk};
use crate::util;

///////////
// Utils //
///////////
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
	pub chunks: BTreeMap<String, Vec<Box<FileChunk>>>,
	pub missing: RefCell<BTreeMap<String, Vec<Box<FileChunk>>>>,
	pub rename: RefCell<BTreeMap<path::PathBuf, path::PathBuf>>,
}

impl DumpState {
	// Helper to safely parse protocol fields with validation
	fn parse_protocol_line<'a>(
		buf: &'a str,
		expected_fields: usize,
	) -> Result<Vec<&'a str>, Box<dyn Error>> {
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

	fn add_chunk(&mut self, hash: String, path: path::PathBuf, offset: u64, size: usize) {
		let v = self.chunks.entry(hash).or_insert(Vec::new());
		if v.iter().position(|p| &p.path == &path).is_none() {
			v.push(Box::new(FileChunk { path, offset, size }));
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
				f.read(&mut buf).await?;

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
		buf: &Vec<u8>,
	) -> Result<(), Box<dyn Error>> {
		// Verify hash before writing to detect corruption during transfer
		let computed_hash = util::hash(buf);
		if computed_hash != chunk.hash {
			return Err(format!(
				"Hash mismatch for chunk {}: expected {}, got {}",
				chunk.hash, chunk.hash, computed_hash
			)
			.into());
		}

		let mut f = afs::OpenOptions::new().write(true).create(true).open(&path).await?;
		f.seek(io::SeekFrom::Start(chunk.offset)).await?;
		f.write_all(&buf).await?;
		Ok(())
	}
}

fn traverse_dir<'a>(
	mut state: &'a mut DumpState,
	dir: path::PathBuf,
) -> Pin<Box<dyn Future<Output = Result<(), Box<dyn Error>>> + 'a>> {
	Box::pin(async move {
		for entry in fs::read_dir(&dir)? {
			let entry = entry?;
			let path = entry.path();
			if state.exclude[0].matches_path(&path) {
				continue;
			}

			let meta = fs::symlink_metadata(&path)?;

			if meta.is_file() {
				println!(
					"F:{}:{}:{}:{}:{}:{}:{}",
					&path.to_str().unwrap(),
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
						println!("C:{}:{}:{}", offset, count, &h);
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
						println!("C:{}:{}:{}", offset, count, &h);
						//state.chunks.insert(h, path.clone());
						state.add_chunk(h, path.clone(), offset, count);
						offset += count as u64;
						n -= count;
					}
					n += f.read(&mut buf[n..]).await?;
				}
			}
			if meta.file_type().is_symlink() {
				println!(
					"L:{}:{}:{}:{}:{}:{}",
					&path.to_str().unwrap(),
					meta.mode(),
					meta.uid(),
					meta.gid(),
					meta.ctime(),
					meta.mtime()
				);
			}
			if meta.is_dir() {
				println!(
					"D:{}:{}:{}:{}:{}:{}",
					&path.to_str().unwrap(),
					meta.mode(),
					meta.uid(),
					meta.gid(),
					meta.ctime(),
					meta.mtime()
				);
				//println!("D:{}:{}:{}", path.to_str().unwrap(), meta.uid(), meta.gid());
				traverse_dir(&mut state, path).await?
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
	task::block_on(traverse_dir(&mut state, dir))?;

	println!(".");
	Ok(state)
}

async fn serve_read(dir: path::PathBuf, dump_state: &DumpState) -> Result<(), Box<dyn Error>> {
	let mut chunks: Vec<String> = Vec::new();
	let mut buf = String::new();
	loop {
		buf.clear();
		io::stdin().read_line(&mut buf).expect("Failed to read");
		if buf.trim() == "." {
			break;
		}
		chunks.push(String::from(buf.trim()));
	}

	for chunk in &chunks {
		let &fc_vec_opt = &dump_state.chunks.get(chunk);

		match &fc_vec_opt {
			Some(fc_vec) => {
				let fc = &fc_vec[0];
				let path = dir.join(&fc.path);
				let mut f = afs::File::open(&path).await?;
				let mut buf: Vec<u8> = vec![0; fc.size];
				f.seek(io::SeekFrom::Start(fc.offset)).await?;
				f.read(&mut buf).await?;
				let encoded = general_purpose::STANDARD.encode(buf);
				println!("C:{}", chunk);
				for line in encoded.into_bytes().chunks(config::BASE64_LINE_LENGTH) {
					io::stdout().write_all(line)?;
					io::stdout().write_all(b"\n")?;
				}
				println!(".");
			}
			None => {}
		}
	}
	println!(".");
	Ok(())
}

async fn serve_write(dir: path::PathBuf, dump_state: &DumpState) -> Result<(), Box<dyn Error>> {
	let mut buf = String::new();

	let mut file: Option<afs::File> = None;
	let mut filepath = path::PathBuf::from("");
	loop {
		buf.clear();
		io::stdin().read_line(&mut buf)?;

		let fields = DumpState::parse_protocol_line(&buf, 1)?;
		let cmd = fields[0];

		match cmd {
			"FM" | "FD" => {
				let fields = DumpState::parse_protocol_line(&buf, 8)?;
				let path = path::PathBuf::from(fields[1]);
				let fd = Box::new(FileData {
					tp: FileType::File,
					path: path.clone(),
					mode: fields[2]
						.parse()
						.map_err(|e| format!("Invalid mode '{}': {}", fields[2], e))?,
					user: fields[3]
						.parse()
						.map_err(|e| format!("Invalid user '{}': {}", fields[3], e))?,
					group: fields[4]
						.parse()
						.map_err(|e| format!("Invalid group '{}': {}", fields[4], e))?,
					ctime: fields[5]
						.parse()
						.map_err(|e| format!("Invalid ctime '{}': {}", fields[5], e))?,
					mtime: fields[6]
						.parse()
						.map_err(|e| format!("Invalid mtime '{}': {}", fields[6], e))?,
					size: fields[7]
						.parse()
						.map_err(|e| format!("Invalid size '{}': {}", fields[7], e))?,
					chunks: vec![],
				});
				if cmd == "FD" {
					filepath = path.clone();
					let filename = path.file_name().ok_or("Path has no filename")?.to_os_string();
					let mut tmp_filename = filename;
					tmp_filename.push(".SyNcR-TmP");
					filepath.set_file_name(tmp_filename);
					//eprintln!("CREATE {:?}", &filepath);
					file = Some(afs::File::create(&filepath).await?);
					afs::set_permissions(&filepath, afs::Permissions::from_mode(fd.mode)).await?;
					dump_state.rename.borrow_mut().insert(filepath.clone(), path.clone());
				}
			}
			"D" => {
				let fields = DumpState::parse_protocol_line(&buf, 7)?;
				let path = path::PathBuf::from(fields[1]);
				let fd = Box::new(FileData {
					tp: FileType::Dir,
					path: path.clone(),
					mode: fields[2]
						.parse()
						.map_err(|e| format!("Invalid mode '{}': {}", fields[2], e))?,
					user: fields[3]
						.parse()
						.map_err(|e| format!("Invalid user '{}': {}", fields[3], e))?,
					group: fields[4]
						.parse()
						.map_err(|e| format!("Invalid group '{}': {}", fields[4], e))?,
					ctime: fields[5]
						.parse()
						.map_err(|e| format!("Invalid ctime '{}': {}", fields[5], e))?,
					mtime: fields[6]
						.parse()
						.map_err(|e| format!("Invalid mtime '{}': {}", fields[6], e))?,
					size: 0,
					chunks: vec![],
				});
				filepath = path.clone();
				let filename = path.file_name().ok_or("Path has no filename")?;
				// FIXME: Should create temp dir, but then all paths must be altered!
				//filename.push(".SyNcR-TmP");
				filepath.set_file_name(filename);
				eprintln!("MKDIR {:?}", &filepath);
				afs::create_dir(&filepath).await?;
				afs::set_permissions(&filepath, afs::Permissions::from_mode(fd.mode)).await?;
				dump_state.rename.borrow_mut().insert(filepath.clone(), path.clone());
			}
			"LC" | "RC" => {
				let fields = DumpState::parse_protocol_line(&buf, 4)?;
				if file.is_none() {
					return Err("Protocol error: chunk command without file".into());
				}
				let hc = Box::new(HashChunk {
					hash: String::from(fields[3]),
					offset: fields[1]
						.parse()
						.map_err(|e| format!("Invalid offset '{}': {}", fields[1], e))?,
					size: fields[2]
						.parse()
						.map_err(|e| format!("Invalid size '{}': {}", fields[2], e))?,
				});
				if cmd == "LC" {
					// Local chunk, copy it locally
					let buf = dump_state
						.read_chunk(&dir, fields[3])
						.await?
						.ok_or_else(|| format!("Chunk not found: {}", fields[3]))?;
					if let Err(e) = dump_state.write_chunk(&filepath, &hc, &buf).await {
						println!("ERROR {}", e);
					}
				} else {
					// Remote chunk, add to wait list
					let mut missing = dump_state.missing.borrow_mut();
					let v = missing.entry(String::from(fields[3])).or_insert(Vec::new());
					v.push(Box::new(FileChunk {
						path: filepath.clone(),
						offset: fields[1]
							.parse()
							.map_err(|e| format!("Invalid offset '{}': {}", fields[1], e))?,
						size: fields[2]
							.parse()
							.map_err(|e| format!("Invalid size '{}': {}", fields[2], e))?,
					}));
				}
			}
			"C" => {
				let fields = DumpState::parse_protocol_line(&buf, 2)?;
				let mut buf = String::new();
				let hash = fields[1];
				let mut chunk: Vec<u8> = Vec::new();
				loop {
					buf.clear();
					io::stdin().read_line(&mut buf)?;
					if buf.trim() == "." {
						break;
					}
					//eprintln!("DECODE: [{:?}]", &buf.trim());
					chunk.append(&mut general_purpose::STANDARD.decode(&buf.trim())?);
				}
				//eprintln!("DECODED CHUNK: {:?}", chunk);
				let mut missing = dump_state.missing.borrow_mut();
				match missing.get(hash) {
					Some(fc_vec) => {
						for fc in fc_vec {
							let hc = HashChunk {
								hash: String::from(hash),
								offset: fc.offset,
								size: fc.size,
							};
							//let filepath = tmp_filename(&fc.path);
							if let Err(e) = dump_state.write_chunk(&fc.path, &hc, &chunk).await {
								eprintln!("ERROR WRITING {}", e);
							}
						}
						missing.remove(hash);
					}
					None => {}
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
	println!("OK");
	Ok(())
}

async fn serve_commit(
	_fixme_dir: path::PathBuf,
	dump_state: &DumpState,
) -> Result<(), Box<dyn Error>> {
	let missing = dump_state.missing.borrow();
	if missing.len() > 0 {
		let missing_hashes: Vec<&String> = missing.keys().collect();
		eprintln!("ERROR: Cannot commit - {} missing chunks", missing.len());
		for hash in &missing_hashes {
			eprintln!("  Missing chunk: {}", hash);
		}
		println!("ERROR:Cannot commit with {} missing chunks", missing.len());
		return Err(format!("Cannot commit: {} chunks still missing", missing.len()).into());
	}
	drop(missing); // Release the borrow before rename operations

	for (src, dst) in dump_state.rename.borrow().iter() {
		//eprintln!("RENAME: {:?} -> {:?}", src, dst);
		afs::rename(&src, &dst).await?;
		//fs::rename(&src, &dst)?;
	}
	println!("OK");
	Ok(())
}

pub fn serve(dir: &str) -> Result<(), Box<dyn Error>> {
	env::set_current_dir(&dir)?;
	println!("VERSION:1");
	println!(".");

	let mut dump_state: Option<DumpState> = None;

	loop {
		let mut cmdline = String::new();
		io::stdin().read_line(&mut cmdline).expect("Failed to read command");

		match &cmdline.trim()[..] {
			"LIST" => dump_state = Some(serve_list(path::PathBuf::from("."))?),
			"READ" => match &dump_state {
				Some(state) => task::block_on(serve_read(path::PathBuf::from("."), &state))?,
				None => {
					println!("!Use LIST command first!");
				}
			},
			"WRITE" => match &dump_state {
				Some(state) => task::block_on(serve_write(path::PathBuf::from("."), &state))?,
				None => {
					println!("!Use LIST command first!");
				}
			},
			"COMMIT" => match &dump_state {
				Some(state) => task::block_on(serve_commit(path::PathBuf::from("."), &state))?,
				None => {
					println!("!Use LIST command first!");
				}
			},
			"QUIT" => break,
			_ => println!("E:UNK-CMD: Unknown command: {}", &cmdline.trim()),
		}
	}
	Ok(())
}
