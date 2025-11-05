use async_std::sync::Mutex;
use async_std::{fs as afs, prelude::*};
use futures::future;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io::{self, Read};
use std::{path, pin::Pin};
use termios::{tcsetattr, Termios, ECHO, ICANON, TCSANOW};

use crate::connect;
use crate::types::{Config, FileData, FileType, HashChunk, PreviousSyncState};

//////////
// Sync //
//////////

// RAII guard to restore terminal settings on drop (prevents broken terminal on panic)
struct TerminalGuard {
	fd: i32,
	original: Termios,
}

impl TerminalGuard {
	fn new() -> Result<Self, Box<dyn Error>> {
		let fd = 0; // stdin
		let original = Termios::from_fd(fd)?;
		let mut new_termios = original;
		new_termios.c_lflag &= !(ICANON | ECHO);
		tcsetattr(fd, TCSANOW, &new_termios)?;
		Ok(TerminalGuard { fd, original })
	}
}

impl Drop for TerminalGuard {
	fn drop(&mut self) {
		// Restore terminal even if panic occurs
		let _ = tcsetattr(self.fd, TCSANOW, &self.original);
	}
}

struct NodeState {
	id: u8,
	send: Mutex<async_process::ChildStdin>,
	recv: Mutex<async_std::io::BufReader<async_process::ChildStdout>>,
	dir: BTreeMap<path::PathBuf, Box<FileData>>,
	chunks: BTreeSet<String>,
	missing: Mutex<BTreeSet<String>>,
}

impl PartialEq for NodeState {
	fn eq(&self, other: &Self) -> bool {
		self.id == other.id
	}
}

impl NodeState {
	// Helper to safely parse protocol fields with validation
	fn parse_protocol_line(buf: &str, expected_fields: usize) -> Result<Vec<&str>, Box<dyn Error>> {
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

	async fn write_file(&self, file: &FileData, trans_data: bool) -> Result<(), Box<dyn Error>> {
		match file.tp {
			FileType::File => {
				if trans_data {
					writeln!(
						self.send.lock().await,
						"FD:{}:{}:{}:{}:{}:{}:{}",
						file.path.to_str().expect(""),
						file.mode,
						file.user,
						file.group,
						file.ctime,
						file.mtime,
						file.size
					)
					.await?;
					for chunk in &file.chunks {
						if !self.chunks.contains(&chunk.hash) {
							// Chunk needs transfer
							writeln!(
								self.send.lock().await,
								"RC:{}:{}:{}",
								chunk.offset,
								chunk.size,
								chunk.hash
							)
							.await?;
							self.missing.lock().await.insert(chunk.hash.clone());
						} else {
							// Chunk is available locally
							writeln!(
								self.send.lock().await,
								"LC:{}:{}:{}",
								chunk.offset,
								chunk.size,
								chunk.hash
							)
							.await?;
						}
					}
					writeln!(self.send.lock().await, ".").await?;
				} else {
					writeln!(
						self.send.lock().await,
						"FM:{}:{}:{}:{}:{}:{}:{}",
						file.path.to_str().expect(""),
						file.mode,
						file.user,
						file.group,
						file.ctime,
						file.mtime,
						file.size
					)
					.await?;
				}
			}
			FileType::SymLink => {
				writeln!(
					self.send.lock().await,
					"L:{}:{}:{}:{}:{}:{}",
					file.path.to_str().expect(""),
					file.mode,
					file.user,
					file.group,
					file.ctime,
					file.mtime
				)
				.await?;
			}
			FileType::Dir => {
				writeln!(
					self.send.lock().await,
					"D:{}:{}:{}:{}:{}:{}",
					file.path.to_str().expect(""),
					file.mode,
					file.user,
					file.group,
					file.ctime,
					file.mtime
				)
				.await?;
			}
		}
		Ok(())
	}

	async fn send(&self, buf: &str) -> Result<(), Box<dyn Error>> {
		self.send.lock().await.write_all([buf, "\n"].concat().as_bytes()).await?;
		Ok(())
	}

	async fn do_collect(&mut self) -> Result<(), Box<dyn Error>> {
		let mut buf = String::new();
		let mut file_data: Option<&mut Box<FileData>> = None;

		loop {
			buf.clear();
			self.recv.lock().await.read_line(&mut buf).await?;
			if buf.trim() == "." {
				break;
			}
			//eprintln!("[{}]HDR: {}", self.id, buf.trim());
		}

		self.send.lock().await.write_all(b"LIST\n").await?;
		loop {
			buf.clear();
			self.recv.lock().await.read_line(&mut buf).await?;
			if buf.trim() == "." {
				break;
			}
			//println!("[{}]LINE: {}", self.id, buf.trim());

			let fields = Self::parse_protocol_line(&buf, 1)?;
			let cmd = fields[0];

			match cmd {
				"F" => {
					let fields = Self::parse_protocol_line(&buf, 8)?;
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
					self.dir.insert(fd.path.clone(), fd);
					file_data = self.dir.get_mut(&path);
				}
				"C" => {
					let fields = Self::parse_protocol_line(&buf, 4)?;
					let hc = HashChunk {
						hash: String::from(fields[3]),
						offset: fields[1]
							.parse()
							.map_err(|e| format!("Invalid offset '{}': {}", fields[1], e))?,
						size: fields[2]
							.parse()
							.map_err(|e| format!("Invalid size '{}': {}", fields[2], e))?,
					};
					match &mut file_data {
						Some(data) => {
							data.chunks.push(hc);
						}
						None => {
							return Err("Protocol error: chunk without file".into());
						}
					}
					self.chunks.insert(String::from(fields[3]));
				}
				"L" => {
					let fields = Self::parse_protocol_line(&buf, 7)?;
					let path = path::PathBuf::from(fields[1]);
					let fd = Box::new(FileData {
						tp: FileType::SymLink,
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
					self.dir.insert(fd.path.clone(), fd);
					file_data = self.dir.get_mut(&path);
				}
				"D" => {
					let fields = Self::parse_protocol_line(&buf, 7)?;
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
					self.dir.insert(fd.path.clone(), fd);
					file_data = self.dir.get_mut(&path);
				}
				_ => return Err(format!("Unknown command in protocol: {}", cmd).into()),
			}
		}

		Ok(())
	}
}

struct SyncState {
	nodes: Vec<NodeState>,
	//tree: BTreeMap<path::PathBuf, u8>
	//tree: BTreeMap<path::PathBuf, &FileData>
}

impl SyncState {
	fn add_node(
		&mut self,
		send: async_process::ChildStdin,
		recv: async_std::io::BufReader<async_process::ChildStdout>,
	) {
		let node = NodeState {
			id: self.nodes.len() as u8 + 1,
			send: Mutex::new(send),
			recv: Mutex::new(recv),
			dir: BTreeMap::new(),
			chunks: BTreeSet::new(),
			missing: Mutex::new(BTreeSet::new()),
		};
		self.nodes.push(node);
	}
}

#[derive(Debug)]
enum SyncOption<T> {
	None,
	Conflict,
	Some(T),
}

impl<T> SyncOption<T> {
	pub fn is_none(&self) -> bool {
		matches!(self, SyncOption::None)
	}

	pub fn is_conflict(&self) -> bool {
		matches!(self, SyncOption::Conflict)
	}

	pub fn is_some(&self) -> bool {
		matches!(self, SyncOption::Some(_))
	}
}

// Load previous sync state from JSON file for three-way merge detection
async fn load_previous_state(config: &Config) -> Result<Option<PreviousSyncState>, Box<dyn Error>> {
	let state_file = config.syncr_dir.join(format!("{}.profile.json", config.profile));

	// If file doesn't exist, this is the first sync
	if !state_file.exists() {
		return Ok(None);
	}

	// Try to read and parse the state
	let contents = afs::read_to_string(&state_file).await?;
	let file_map: BTreeMap<String, FileData> = serde_json::from_str(&contents)?;

	// Get current timestamp
	let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();

	Ok(Some(PreviousSyncState { files: file_map, timestamp }))
}

pub async fn sync(config: Config, dirs: Vec<&str>) -> Result<(), Box<dyn Error>> {
	let mut state = SyncState { nodes: Vec::new() };
	//let mut tree: BTreeMap<path::PathBuf, u8> = BTreeMap::new();
	let mut tree: BTreeMap<path::PathBuf, Box<FileData>> = BTreeMap::new();

	// Load previous sync state for three-way merge detection
	eprintln!("Loading previous state...");
	let previous_state = load_previous_state(&config).await?;
	if previous_state.is_some() {
		eprintln!(
			"Loaded previous state with {} files",
			previous_state.as_ref().unwrap().files.len()
		);
	} else {
		eprintln!("No previous state found (first sync)");
	}

	eprintln!("Initializing processes...");
	for dir in dirs {
		let conn = connect::connect(dir).await?;
		state.add_node(conn.send, conn.recv);
	}

	eprintln!("Collecting...");
	let mut futs: Vec<Pin<Box<dyn future::Future<Output = _>>>> = vec![];
	for node in &mut state.nodes {
		futs.push(Box::pin(node.do_collect()));
	}
	future::join_all(futs).await;

	// Do diffing
	eprintln!("Running diff...");

	// Configure terminal for key input (RAII guard ensures cleanup on panic)
	let _terminal_guard = TerminalGuard::new()?;

	let mut diff: BTreeMap<&path::Path, SyncOption<u8>> = BTreeMap::new();
	for node in &state.nodes {
		for path in node.dir.keys() {
			// Get previous state for three-way merge
			let prev_file = previous_state
				.as_ref()
				.and_then(|ps| ps.files.get(&path.to_string_lossy().to_string()));

			diff.entry(path).or_insert_with(|| {
				let mut files: Vec<Option<&Box<FileData>>> =
					state.nodes.iter().map(|n| n.dir.get(path)).collect();
				//let mut latest: Option<u8> = None;
				let mut winner: SyncOption<u8> = SyncOption::None;

				// Compare files on all nodes and find where to sync from
				//eprintln!("File: {:?}", &path);
				for (idx, file) in files.iter().enumerate() {
					if let Some(f) = file {
						// Three-way merge: compare with previous state if available
						if let Some(prev) = prev_file {
							if f.tp != prev.tp
								|| f.mode != prev.mode || f.user != prev.user
								|| f.group != prev.group || f.chunks != prev.chunks
							{
								eprintln!(
									"diff {} {} {} {} {} (modified since last sync)",
									f.tp != prev.tp,
									f.mode != prev.mode,
									f.user != prev.user,
									f.group != prev.group,
									f.chunks != prev.chunks
								);
								if winner.is_none() {
									winner = SyncOption::Some(idx as u8);
								} else if winner.is_some() {
									winner = SyncOption::Conflict;
								}
							}
						} else {
							// No previous state - use old logic
							if winner.is_none() {
								winner = SyncOption::Some(idx as u8);
							} else if let SyncOption::Some(win) = winner {
								let w = files[win as usize].unwrap();
								if f.tp != w.tp
									|| f.mode != w.mode || f.user != w.user
									|| f.group != w.group || f.chunks != w.chunks
								{
									winner = SyncOption::Conflict;
								}
							}
						}
					} else {
						eprintln!("Node: {} <missing>", idx);
					}
				}
				files.dedup();
				if files.len() <= 1 {
					winner = SyncOption::None;
				}
				if winner.is_conflict() {
					eprintln!("File: {:?}", winner);
					for (idx, file) in files.iter().enumerate() {
						if let Some(f) = file {
							eprintln!("    {}: {:?}", idx + 1, f);
						}
					}
					loop {
						eprint!("? ");
						let mut buf = [0; 1];
						let keypress = io::stdin().read(&mut buf).map(|_| buf[0]);
						if let Ok(key) = keypress {
							eprintln!("{:?}", key);
							if b'1' <= key && key <= b'0' + files.len() as u8 {
								winner = SyncOption::Some(key - b'1');
								break;
							} else if key == b's' {
								winner = SyncOption::None;
								break;
							}
						}
					}
				}
				if let SyncOption::Some(win) = winner {
					let w = files[win as usize].unwrap();
					//state.tree.insert(path.clone(), win);
					//tree.insert(path.clone(), win);
					tree.insert(path.clone(), w.clone());
				}
				winner

				/*
				// FIXME: This algorithm always syncs from the latest modification
				for (idx, file) in files.iter().enumerate() {
					if let Some(f) = file {
						if latest.is_none() || f.mtime > files[latest.unwrap() as usize].unwrap().mtime {
							latest = Some(idx as u8);
						}
					}
				}
				files.dedup();
				if files.len() <= 1 {
					latest = None;
				}
				latest
				*/
			});
		}
	}
	//println!("DIFF: {:?}", diff);

	// Detect deleted files: files that existed in previous sync but are missing from all nodes
	let mut deleted_files: Vec<String> = Vec::new();
	if let Some(prev_state) = &previous_state {
		for prev_path in prev_state.files.keys() {
			let path_buf = path::PathBuf::from(prev_path);
			// Check if this file exists on any node
			let exists_on_any_node =
				state.nodes.iter().any(|node| node.dir.contains_key(&path_buf));

			if !exists_on_any_node {
				// File was in previous state but is missing from all nodes
				eprintln!("File was deleted: {}", prev_path);
				deleted_files.push(prev_path.clone());
			}
		}
	}
	eprintln!("Found {} deleted files", deleted_files.len());

	// Terminal will be automatically restored by TerminalGuard when it goes out of scope

	/*
	let mut json: String = "".to_owned();
	for (file, node) in tree {
		json.push('"');
		json.push_str(&file.to_str().unwrap());
		json.push_str("\":");
		json.push_str(serde_json::to_string(state.nodes[node as usize].dir.get(&file).unwrap()).unwrap().as_str());
	}
	*/
	let json = serde_json::to_string(&tree).unwrap();
	eprintln!("JSON: {}", json);
	let fname = config.syncr_dir.clone().join("test.profile.json");
	let mut f = afs::File::create(&fname).await?;
	f.write_all(json.as_bytes()).await?;

	// Do write meta
	eprintln!("Sending metadata...");
	for node in &state.nodes {
		node.send("WRITE").await?;
	}

	for (path, to_do) in diff {
		if let SyncOption::Some(todo) = to_do {
			let files: Vec<Option<&Box<FileData>>> =
				state.nodes.iter().map(|n| n.dir.get(path)).collect();
			let lfile = &files[todo as usize].unwrap();

			for (idx, file) in files.iter().enumerate() {
				if idx != todo as usize {
					let mut trans_meta = false;
					let mut trans_data = false;
					if let Some(file) = file {
						if file != lfile {
							trans_meta = true;
							if file.chunks != lfile.chunks {
								trans_data = true;
							}
						}
					} else {
						trans_meta = true;
						trans_data = true;
					}
					if trans_meta {
						let node = &state.nodes[idx];
						node.write_file(lfile, trans_data).await?;
					}
				}
			}
		}
	}

	// Send delete commands for files that were deleted
	eprintln!("Sending delete commands...");
	for deleted_path in &deleted_files {
		for node in &state.nodes {
			node.send(&format!("DEL:{}", deleted_path)).await?;
		}
	}

	// Do chunk transfers
	eprintln!("Transfering data chunks...");
	let mut done: BTreeSet<String> = BTreeSet::new();
	for srcnode in &state.nodes {
		eprintln!("  - NODE {}", srcnode.id);
		srcnode.send(".\nREAD").await?;
		for dstnode in &state.nodes {
			if dstnode != srcnode {
				let missing = dstnode.missing.lock().await;
				for chunk in missing.iter() {
					if !done.contains(chunk) {
						//eprintln!("MISSING CHUNK: {} {:?}", chunk, srcnode.chunks.get(chunk));
						if srcnode.chunks.contains(chunk) {
							srcnode.send(chunk).await?;
							done.insert(String::from(chunk));
						}
					}
				}
			}
		}
		srcnode.send(".").await?;
		let mut buf = String::new();
		let mut chunk = String::new();
		let mut chunkdata = String::new();
		loop {
			buf.clear();
			srcnode.recv.lock().await.read_line(&mut buf).await?;
			if chunk.is_empty() && &buf[..2] == "C:" {
				chunk.clear();
				chunk.push_str(&buf.trim()[2..]);
				chunkdata.clear();
			} else if chunk.is_empty() && buf.trim() == "." {
				break;
			} else if buf.trim() == "." {
				chunkdata.push('.');
				let data = &["C:", &chunk, "\n", &chunkdata].join("");
				for dstnode in &state.nodes {
					if dstnode != srcnode {
						let mut missing = dstnode.missing.lock().await;
						if missing.get(&chunk).is_some() {
							// Send chunk
							dstnode.send(data).await?;
							missing.remove(&chunk);
						}
					}
				}
				chunk.clear();
				chunkdata.clear();
			} else {
				chunkdata += &buf;
			}
		}
		srcnode.send("WRITE").await?;
	}

	// Close WRITE sessions
	for node in &state.nodes {
		node.send(".").await?;
	}

	// Commit modifications (do renames)
	eprintln!("Commiting changes...");
	for node in &state.nodes {
		node.send("COMMIT").await?;
		// Wait for response and check for errors
		let mut buf = String::new();
		node.recv.lock().await.read_line(&mut buf).await?;
		let response = buf.trim();
		if response.starts_with("ERROR:") {
			return Err(format!("Node {} failed to commit: {}", node.id, response).into());
		} else if response != "OK" {
			return Err(
				format!("Node {} returned unexpected response: {}", node.id, response).into()
			);
		}
	}

	// Quit children
	for node in &state.nodes {
		node.send("QUIT").await?;
		let mut buf = String::new();
		loop {
			buf.clear();
			let n = node.recv.lock().await.read_line(&mut buf).await?;
			if n == 0 || buf.trim() == "." {
				break;
			}
			//eprintln!("QUIT: {}", buf.trim());
		}
	}

	Ok(())
}
