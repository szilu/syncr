pub mod state;

pub use self::state::NodeState;

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io::{self, Read};
use std::path;
use tokio::fs as afs;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

// Use the old connect module directly (not the new connection.rs library API)
use crate::cache::ChildCache;
use crate::config::Config;
use crate::conflict::{Conflict, ConflictResolver, ConflictType, FileVersion};
use crate::connect;
use crate::exclusion::{ExcludeConfig, ExclusionEngine};
use crate::logging::*;
#[allow(unused_imports)]
use crate::metadata_utils;
use crate::types::{FileData, PreviousSyncState, SyncPhase};
use crate::utils::setup_signal_handlers;
use crate::utils::terminal::{restore_terminal_state, TerminalGuard};
use tracing::error;

//////////
// Callbacks trait for sync progress notification //
//////////

/// Progress statistics for callbacks
#[derive(Debug, Clone)]
pub struct ProgressUpdate {
	pub phase: SyncPhase,
	pub files_processed: usize,
	pub files_total: usize,
	pub bytes_transferred: u64,
	pub bytes_total: u64,
	pub transfer_rate: f64,
}

/// Comprehensive sync event type covering all callback needs
#[derive(Debug, Clone)]
pub enum SyncCallbackEvent {
	/// Phase lifecycle: phase name and is_starting (true) or completing (false)
	PhaseChanged {
		phase: SyncPhase,
		is_starting: bool,
	},

	/// Progress update during phase
	Progress(ProgressUpdate),

	/// Node connection event
	NodeConnecting {
		node_id: usize,
		location: String,
	},

	NodeReady {
		node_id: usize,
		location: String,
	},

	/// Protocol version selected during negotiation
	ProtocolVersionSelected {
		version: u32,
	},

	NodeDisconnecting {
		node_id: usize,
	},

	/// Node statistics update (e.g., after collection completes)
	NodeStats {
		node_id: usize,
		files_known: usize, // Files catalogued/scanned from this node
		bytes_known: u64,
	},

	/// File discovered on a node during sync processing
	FileDiscovered {
		path: String,
		node_id: usize,
		exists: bool, // true if file exists on this node, false if missing
	},

	/// File operation event
	FileOperation {
		path: String,
		operation: &'static str, // "create", "update", "delete"
		is_starting: bool,
		file_size: u64,
		from_node: usize, // Source node (where file comes from)
		to_node: usize,   // Destination node (where file is being sent)
	},

	/// Conflict event
	Conflict {
		path: String,
		is_detected: bool, // true for detected, false for resolved
		num_versions: usize,
		winner: Option<usize>, // Some(node_id) if resolved
		/// Modification times for each node's version (None = file doesn't exist on that node)
		node_mtimes: Option<Vec<Option<u32>>>,
	},
}

/// Trait for receiving sync events as a unified callback
pub trait SyncProgressCallback: Send + Sync {
	/// Called for all sync events - simple, unified interface
	fn on_event(&self, _event: SyncCallbackEvent) {}
}

impl<T: Fn(SyncCallbackEvent) + Send + Sync> SyncProgressCallback for T {
	fn on_event(&self, event: SyncCallbackEvent) {
		self(event);
	}
}

//////////
// Sync Metrics and State //
//////////

/// Track metrics during sync for final reporting
#[derive(Debug, Clone, Default)]
struct SyncMetrics {
	bytes_transferred: u64,
	chunks_transferred: usize,
	files_synced: usize,
	dirs_created: usize,
	files_deleted: usize,
	conflicts_encountered: usize,
	conflicts_resolved: usize,
}

/// Protocol version for handshake and compatibility checking
struct SyncState {
	nodes: Vec<NodeState>,
	//tree: BTreeMap<path::PathBuf, u8>
	//tree: BTreeMap<path::PathBuf, &FileData>
}

impl SyncState {
	fn add_node(&mut self, protocol: Box<dyn crate::protocol::ProtocolClient>) {
		let node = NodeState {
			id: self.nodes.len() as u8 + 1,
			protocol: Mutex::new(protocol),
			dir: BTreeMap::new(),
			chunks: BTreeSet::new(),
			missing: Mutex::new(BTreeSet::new()),
			capabilities: None,
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
	let file_map: BTreeMap<String, FileData> = json5::from_str(&contents)?;

	// Get current timestamp
	let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();

	Ok(Some(PreviousSyncState { files: file_map, timestamp }))
}

/// Message for resolving a conflict - contains path and chosen node index
pub struct ConflictResolution {
	pub path: String,
	pub chosen_node: usize,
}

/// Sync with callbacks for progress and event notification
/// If conflict_rx is provided, the sync will wait for conflict resolutions from the channel
/// instead of prompting on stdin
pub async fn sync_with_callbacks(
	config: Config,
	dirs: Vec<&str>,
	callbacks: Box<dyn SyncProgressCallback>,
	conflict_rx: Option<std::sync::mpsc::Receiver<ConflictResolution>>,
) -> Result<crate::types::SyncResult, Box<dyn Error>> {
	sync_impl(config, dirs, Some(callbacks), conflict_rx).await
}

pub async fn sync(config: Config, dirs: Vec<&str>) -> Result<(), Box<dyn Error>> {
	sync_impl(config, dirs, None, None).await?;
	Ok(())
}

/// Sync with progress display (but no interactive conflict resolution)
pub async fn sync_with_cli_progress(config: Config, dirs: Vec<&str>) -> Result<(), Box<dyn Error>> {
	eprintln!("Starting sync with progress display...");

	// Create callback for progress display with smart node labels
	let callback = crate::progress::CliProgressCallback::with_addresses(dirs.clone());

	// No conflict channel - conflicts will be skipped
	sync_impl(config, dirs, Some(Box::new(callback)), None).await?;
	eprintln!(); // Final newline after progress
	eprintln!("Sync complete!");
	Ok(())
}

/// CLI-based conflict prompt callback
/// Handles interactive conflict resolution by prompting the user on stdin
struct ConflictPrompt {
	conflict_tx: std::sync::Mutex<std::sync::mpsc::Sender<ConflictResolution>>,
}

impl ConflictPrompt {
	/// Create a new conflict prompt callback
	fn new(tx: std::sync::mpsc::Sender<ConflictResolution>) -> Self {
		Self { conflict_tx: std::sync::Mutex::new(tx) }
	}
}

impl SyncProgressCallback for ConflictPrompt {
	fn on_event(&self, event: SyncCallbackEvent) {
		if let SyncCallbackEvent::Conflict { path, is_detected, num_versions, winner: _, .. } =
			event
		{
			if is_detected {
				// Clear progress line and show conflict
				eprintln!();
				eprintln!("⚠️  Conflict detected: {}", path);
				eprintln!("   {} versions exist on different nodes", num_versions);

				// Prompt for resolution
				loop {
					// Write prompt to stdout so it's visible and in line with stdin
					use std::io::Write;
					print!("   Choose node (1-{}): ", num_versions);
					let _ = std::io::stdout().flush();

					let mut input = String::new();
					match std::io::stdin().read_line(&mut input) {
						Ok(bytes_read) => {
							if bytes_read == 0 {
								eprintln!("   Error: EOF reached. Skipping conflict.");
								break;
							}
							let trimmed = input.trim();
							match trimmed.parse::<usize>() {
								Ok(choice) => {
									if choice > 0 && choice <= num_versions {
										let node_idx = choice - 1;
										eprintln!(
											"   Resolving to node {} (index {})",
											choice, node_idx
										);
										if let Ok(tx) = self.conflict_tx.lock() {
											let _ = tx.send(ConflictResolution {
												path: path.clone(),
												chosen_node: node_idx,
											});
										}
										break;
									} else {
										eprintln!(
											"   Invalid choice: {} (must be 1-{}). Try again.",
											choice, num_versions
										);
									}
								}
								Err(_) => {
									eprintln!(
										"   Invalid input: '{}' (must be a number 1-{}). Try again.",
										trimmed, num_versions
									);
								}
							}
						}
						Err(e) => {
							eprintln!("   Error reading input: {}. Skipping conflict.", e);
							break;
						}
					}
				}
				eprintln!();
			}
		}
	}
}

/// Sync with interactive conflict resolution (but no progress display)
pub async fn sync_with_conflicts(config: Config, dirs: Vec<&str>) -> Result<(), Box<dyn Error>> {
	eprintln!("Starting sync with conflict resolution...");

	// Create a channel for conflict resolution
	let (conflict_tx, conflict_rx) = std::sync::mpsc::channel();

	// Create callback for conflict handling only
	let callback = ConflictPrompt::new(conflict_tx);

	sync_impl(config, dirs, Some(Box::new(callback)), Some(conflict_rx)).await?;
	eprintln!("Sync complete!");
	Ok(())
}

/// Composite callback that handles both progress and conflicts
struct CompositeCallback {
	progress: Box<dyn SyncProgressCallback>,
	conflict: Box<dyn SyncProgressCallback>,
}

impl SyncProgressCallback for CompositeCallback {
	fn on_event(&self, event: SyncCallbackEvent) {
		// Forward event to both callbacks
		self.progress.on_event(event.clone());
		self.conflict.on_event(event);
	}
}

/// Sync with both progress display and interactive conflict resolution
pub async fn sync_with_progress_and_conflicts(
	config: Config,
	dirs: Vec<&str>,
) -> Result<(), Box<dyn Error>> {
	eprintln!("Starting sync with progress display and conflict resolution...");

	// Create a channel for conflict resolution
	let (conflict_tx, conflict_rx) = std::sync::mpsc::channel();

	// Create progress callback with smart node labels
	let progress_callback =
		Box::new(crate::progress::CliProgressCallback::with_addresses(dirs.clone()));

	// Create conflict callback
	let conflict_callback = Box::new(ConflictPrompt::new(conflict_tx));

	// Combine both callbacks
	let composite = CompositeCallback { progress: progress_callback, conflict: conflict_callback };

	// Run sync with combined callback
	sync_impl(config, dirs, Some(Box::new(composite)), Some(conflict_rx)).await?;

	eprintln!(); // Final newline after progress
	eprintln!("Sync complete!");
	Ok(())
}

pub async fn sync_impl(
	config: Config,
	dirs: Vec<&str>,
	callbacks: Option<Box<dyn SyncProgressCallback>>,
	conflict_rx: Option<std::sync::mpsc::Receiver<ConflictResolution>>,
) -> Result<crate::types::SyncResult, Box<dyn Error>> {
	// Setup signal handlers for graceful cleanup
	setup_signal_handlers();

	// Acquire path-level locks to prevent concurrent syncs on same paths
	info!("Acquiring path-level locks...");
	let cache_db_path = config.syncr_dir.join("cache.db");
	let cache = ChildCache::open(&cache_db_path)?;

	// Extract remote nodes from paths
	let remote_nodes: Vec<String> = dirs
		.iter()
		.filter(|dir| {
			dir.contains(':')
				&& !dir.starts_with('/')
				&& !dir.starts_with('.')
				&& !dir.starts_with('~')
		})
		.map(|dir| {
			// Extract host from "user@host:path" or "host:path"
			dir.split(':').next().unwrap_or("").to_string()
		})
		.collect::<std::collections::BTreeSet<_>>()
		.into_iter()
		.collect();

	let _path_locks = cache.acquire_locks(&dirs, &remote_nodes)?;

	let mut state = SyncState { nodes: Vec::new() };
	let mut metrics = SyncMetrics::default();
	let start_time = std::time::Instant::now();

	// Run sync logic and ensure cleanup always happens
	let result =
		run_sync_logic(&mut state, config, dirs, callbacks, conflict_rx, &mut metrics).await;

	// Always cleanup child processes, even on error
	cleanup_nodes(&state).await;

	// If sync succeeded, return a SyncResult with actual metrics
	match result {
		Ok(()) => {
			let duration = start_time.elapsed();
			Ok(crate::types::SyncResult {
				files_synced: metrics.files_synced,
				dirs_created: metrics.dirs_created,
				files_deleted: metrics.files_deleted,
				bytes_transferred: metrics.bytes_transferred,
				chunks_transferred: metrics.chunks_transferred,
				chunks_deduplicated: 0,
				conflicts_encountered: metrics.conflicts_encountered,
				conflicts_resolved: metrics.conflicts_resolved,
				duration,
				errors: vec![],
			})
		}
		Err(e) => Err(e),
	}
}

/// Internal sync implementation - separated to ensure cleanup always runs
async fn run_sync_logic(
	state: &mut SyncState,
	config: Config,
	dirs: Vec<&str>,
	callbacks: Option<Box<dyn SyncProgressCallback>>,
	conflict_rx: Option<std::sync::mpsc::Receiver<ConflictResolution>>,
	metrics: &mut SyncMetrics,
) -> Result<(), Box<dyn Error>> {
	//let mut tree: BTreeMap<path::PathBuf, u8> = BTreeMap::new();
	let mut tree: BTreeMap<path::PathBuf, Box<FileData>> = BTreeMap::new();

	// Load previous sync state for three-way merge detection
	info!("Loading previous state...");
	let previous_state = load_previous_state(&config).await?;
	if let Some(ref pstate) = previous_state {
		info!("Loaded previous state with {} files", pstate.files.len());
	} else {
		info!("No previous state found (first sync)");
	}

	info!("Initializing processes...");

	// ─── PHASE 1: Connection & Capability Collection ───
	// We need to handle both local (in-process) and remote (subprocess) connections
	enum ProtocolState {
		// Remote connection waiting for version decision
		Remote(usize, crate::protocol::factory::ProtocolV3Waiting),
		// Local connection ready immediately (uses in-process protocol)
		Local(usize, Box<dyn crate::protocol::ProtocolClient>),
	}

	let mut protocol_states: Vec<ProtocolState> = Vec::new();
	let mut connection_capabilities: Vec<Vec<u32>> = Vec::new();

	for (idx, dir) in dirs.iter().enumerate() {
		// Notify that we're connecting
		if let Some(ref cb) = callbacks {
			cb.on_event(SyncCallbackEvent::NodeConnecting {
				node_id: idx,
				location: dir.to_string(),
			});
		}

		let conn = connect::connect(dir).await?;

		// Handle connection based on type
		match conn {
			connect::ConnectionType::Local(path) => {
				info!("Creating in-process protocol for local path: {}", path.display());
				// For local paths, create in-process protocol
				// Build exclusion patterns: always exclude .SyNcR-TmP, plus configured patterns
				let mut exclude_patterns = vec!["**/*.SyNcR-TmP".to_string()];
				exclude_patterns.extend(config.exclude_patterns.clone());

				let exclude_glob_patterns: Vec<_> = exclude_patterns
					.iter()
					.filter_map(|pattern| glob::Pattern::new(pattern).ok())
					.collect();

				let dump_state = crate::serve::DumpState {
					exclude: exclude_glob_patterns,
					chunks: Default::default(),
					missing: std::sync::Arc::new(Mutex::new(Default::default())),
					rename: std::sync::Arc::new(Mutex::new(Default::default())),
					chunk_writes: std::sync::Arc::new(Mutex::new(Default::default())),
					cache: None,
				};

				let protocol = crate::protocol::create_local_protocol(path, dump_state).await?;

				// Local protocols support the same versions as the current binary
				let server_caps = crate::protocol::negotiation::SUPPORTED_VERSIONS.to_vec();
				connection_capabilities.push(server_caps.clone());
				protocol_states.push(ProtocolState::Local(idx, protocol));

				info!(
					"Node {} (local) capabilities: {:?}",
					idx,
					crate::protocol::negotiation::SUPPORTED_VERSIONS
				);
			}
			connect::ConnectionType::Remote { send, recv } => {
				// Perform protocol handshake and capabilities exchange for remote connections
				info!("Negotiating protocol with {}...", dir);
				let waiting = crate::protocol::create_remote_protocol(send, recv).await?;
				let server_caps = waiting.server_capabilities().to_vec();

				connection_capabilities.push(server_caps.clone());
				protocol_states.push(ProtocolState::Remote(idx, waiting));

				info!("Node {} (remote) capabilities: {:?}", idx, server_caps);
			}
		}
	}

	// ─── PHASE 2: Version Decision ───
	let common_version = crate::protocol::factory::find_common_version(&connection_capabilities)?;
	info!("Selected common protocol version: {}", common_version);

	if let Some(ref cb) = callbacks {
		cb.on_event(SyncCallbackEvent::ProtocolVersionSelected { version: common_version });
	}

	// ─── PHASE 3: Version Distribution & Protocol Finalization ───
	for protocol_state in protocol_states.into_iter() {
		match protocol_state {
			ProtocolState::Remote(idx, waiting) => {
				// Remote connections need to finalize the version decision
				let protocol = waiting.finalize(common_version).await?;
				state.add_node(protocol);

				// Notify that node is ready
				if let Some(ref cb) = callbacks {
					cb.on_event(SyncCallbackEvent::NodeReady {
						node_id: idx,
						location: dirs[idx].to_string(),
					});
				}
			}
			ProtocolState::Local(idx, protocol) => {
				// Local connections are ready immediately
				state.add_node(protocol);

				// Notify that node is ready
				if let Some(ref cb) = callbacks {
					cb.on_event(SyncCallbackEvent::NodeReady {
						node_id: idx,
						location: dirs[idx].to_string(),
					});
				}
			}
		}
	}

	info!("All nodes ready with protocol version {}", common_version);

	// Notify callbacks that we're collecting
	if let Some(ref cb) = callbacks {
		cb.on_event(SyncCallbackEvent::PhaseChanged {
			phase: SyncPhase::Collecting,
			is_starting: true,
		});
		cb.on_event(SyncCallbackEvent::Progress(ProgressUpdate {
			phase: SyncPhase::Collecting,
			files_processed: 0,
			files_total: state.nodes.len(),
			bytes_transferred: 0,
			bytes_total: 0,
			transfer_rate: 0.0,
		}));
	}

	info!("Collecting...");

	// Collect from all nodes in parallel to speed up collection
	// Each node sends progress updates during collection
	use futures::future::try_join_all;

	let collection_tasks: Vec<_> = state
		.nodes
		.iter_mut()
		.enumerate()
		.map(|(idx, node)| {
			// Share callback across tasks via Arc (no Mutex needed - trait is Sync)
			let callback_ref = callbacks.as_ref().map(|cb| {
				// Cast to trait object and wrap in Arc for sharing
				std::sync::Arc::from(cb.as_ref())
			});
			let node_id = idx;

			async move {
				// Collect with progress callback
				node.do_collect(|files_count, bytes_count| {
					// Send incremental progress update
					if let Some(ref cb_arc) = callback_ref {
						cb_arc.on_event(SyncCallbackEvent::NodeStats {
							node_id,
							files_known: files_count,
							bytes_known: bytes_count,
						});
					}
				})
				.await?;

				// Send final statistics after collection completes
				let files_collected = node.dir.len();
				let bytes_collected: u64 = node
					.dir
					.values()
					.filter(|fd| matches!(fd.tp, crate::types::FileType::File))
					.map(|fd| fd.size)
					.sum();

				debug!(
					"Node {} collected {} files, {} bytes",
					node_id, files_collected, bytes_collected
				);

				// Send final NodeStats to ensure we have the accurate final count
				if let Some(ref cb_arc) = callback_ref {
					cb_arc.on_event(SyncCallbackEvent::NodeStats {
						node_id,
						files_known: files_collected,
						bytes_known: bytes_collected,
					});
				}

				Ok::<(), Box<dyn std::error::Error>>(())
			}
		})
		.collect();

	// Wait for all collections to complete
	try_join_all(collection_tasks).await?;

	// Apply exclusion filters to collected files (especially from remote nodes)
	info!("Applying exclusion filters...");
	let exclude_config =
		ExcludeConfig { patterns: config.exclude_patterns.clone(), ..Default::default() };

	// Create exclusion engine for filtering (using root path for now)
	let exclusion_engine = match ExclusionEngine::new_with_includes(
		&exclude_config,
		std::path::Path::new("/"),
		&config.include_patterns,
	) {
		Ok(engine) => Some(engine),
		Err(e) => {
			warn!("Failed to create exclusion engine: {}. Proceeding without filtering.", e);
			None
		}
	};

	// Filter files in each node's dir
	if let Some(ref engine) = exclusion_engine {
		for node in state.nodes.iter_mut() {
			let initial_count = node.dir.len();
			node.dir.retain(|path, _| !engine.should_exclude(path, None));
			let final_count = node.dir.len();
			let excluded = initial_count - final_count;
			if excluded > 0 {
				info!("Node: excluded {} files based on patterns", excluded);
			}
		}
	}

	// Signal collection phase complete
	if let Some(ref cb) = callbacks {
		cb.on_event(SyncCallbackEvent::PhaseChanged {
			phase: SyncPhase::Collecting,
			is_starting: false,
		});
	}

	for node in state.nodes.iter() {
		for path in node.dir.keys() {
			debug!("  - {}", path.display());
		}
	}

	// Emit FileDiscovered events for ALL collected files so they appear in the Files tab
	if let Some(ref cb) = callbacks {
		let mut all_files: std::collections::BTreeSet<&path::Path> =
			std::collections::BTreeSet::new();
		for node in state.nodes.iter() {
			for path in node.dir.keys() {
				all_files.insert(path);
			}
		}

		for path in all_files {
			for (node_idx, node) in state.nodes.iter().enumerate() {
				let exists = node.dir.contains_key(path);
				cb.on_event(SyncCallbackEvent::FileDiscovered {
					path: path.display().to_string(),
					node_id: node_idx,
					exists,
				});
			}
		}
	}

	// Do diffing
	info!("Running diff...");
	if let Some(ref cb) = callbacks {
		cb.on_event(SyncCallbackEvent::PhaseChanged {
			phase: SyncPhase::DetectingConflicts,
			is_starting: true,
		});
	}

	// Configure terminal for key input (RAII guard ensures cleanup on panic)
	// This will be None if stdin is not a terminal (e.g., piped input)
	// NOTE: In TUI mode (when callbacks present), disable interactive terminal mode
	// and let the TUI handle conflict resolution instead
	// Also skip during tests/CI to avoid terminal corruption
	let skip_terminal_mode = cfg!(test) || std::env::var("SYNCR_SKIP_TERMINAL").is_ok();
	let _terminal_guard = if skip_terminal_mode { None } else { TerminalGuard::new() };
	let interactive_mode = _terminal_guard.is_some() && callbacks.is_none();

	// Setup panic hook to ensure terminal is always restored even on panic
	if _terminal_guard.is_some() {
		let default_hook = std::panic::take_hook();
		std::panic::set_hook(Box::new(move |panic_info| {
			// Attempt to restore terminal before printing panic
			restore_terminal_state();
			default_hook(panic_info);
		}));
	}

	// Track conflicts for async resolution (TUI mode)
	// Use RefCell to allow interior mutability in closures
	let detected_conflicts =
		std::cell::RefCell::new(Vec::<(path::PathBuf, Vec<Option<Box<FileData>>>)>::new());

	info!("Starting diff loop, conflict_rx available: {}", conflict_rx.is_some());

	let mut diff: BTreeMap<&path::Path, SyncOption<u8>> = BTreeMap::new();
	let mut file_count = 0;
	for node in &state.nodes {
		for path in node.dir.keys() {
			file_count += 1;
			if file_count % 100 == 0 {
				info!("Processing file {} in diff...", file_count);
			}

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
				for (idx, file) in files.iter().enumerate() {
					if let Some(f) = file {
						// Three-way merge: compare with previous state if available
						if let Some(prev) = prev_file {
							if f.tp != prev.tp
								|| f.mode != prev.mode || f.user != prev.user
								|| f.group != prev.group || f.chunks != prev.chunks
							{
								debug!(
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
								if let Some(Some(w)) = files.get(win as usize) {
									if f.tp != w.tp
										|| f.mode != w.mode || f.user != w.user
										|| f.group != w.group || f.chunks != w.chunks
									{
										winner = SyncOption::Conflict;
									}
								} else {
									winner = SyncOption::Conflict;
								}
							}
						}
					} else {
						debug!("Node: {} <missing>", idx);
					}
				}

				// Store original files BEFORE dedup for conflict resolution
				// (User chooses by node index, not deduped file index)
				let original_files = files.clone();

				files.dedup();
				if files.len() <= 1 {
					winner = SyncOption::None;
				}
				if winner.is_conflict() {
					metrics.conflicts_encountered += 1;

					for (idx, file) in files.iter().enumerate() {
						if let Some(f) = file {
							warn!("    {}: {:?}", idx + 1, f);
						}
					}

					// Fire conflict callback for TUI mode
					if let Some(ref cb) = callbacks {
						// First, send FileDiscovered events to track which nodes have the file
						// Use ORIGINAL files (before dedup) to match node indices
						for (node_idx, file_opt) in original_files.iter().enumerate() {
							cb.on_event(SyncCallbackEvent::FileDiscovered {
								path: path.display().to_string(),
								node_id: node_idx,
								exists: file_opt.is_some(),
							});
						}

						// Then send the conflict event
						cb.on_event(SyncCallbackEvent::Conflict {
							path: path.to_string_lossy().to_string(),
							is_detected: true,
							num_versions: files.len(),
							winner: None,
							node_mtimes: Some(
								original_files
									.iter()
									.map(|file_opt| file_opt.map(|f| f.mtime))
									.collect(),
							),
						});
					}

					// TUI mode with conflict channel: collect conflict for later async resolution
					if conflict_rx.is_some() && callbacks.is_some() {
						info!(
							"Conflict detected (TUI mode), will wait for resolution: {}",
							path.display()
						);
						info!(
							"  original_files.len() = {}, files.len() = {}",
							original_files.len(),
							files.len()
						);

						// Store ORIGINAL files (before dedup) so user can choose by node index
						match detected_conflicts.try_borrow_mut() {
							Ok(mut conflicts) => {
								let cloned_files: Vec<Option<Box<FileData>>> = original_files
									.iter()
									.enumerate()
									.map(|(i, f)| {
										f.map(|b| {
											info!("  Cloning file at index {}", i);
											Box::new((**b).clone())
										})
									})
									.collect();

								info!("  Successfully cloned {} file entries", cloned_files.len());
								conflicts.push((path.to_path_buf(), cloned_files));
							}
							Err(e) => {
								error!(
									"Failed to borrow detected_conflicts for {}: {}",
									path.display(),
									e
								);
							}
						}
						// Keep winner as Conflict for now
					} else if interactive_mode {
						// Interactive mode: prompt user for conflict resolution on stdin
						loop {
							eprint!("? ");
							let mut buf = [0; 1];
							let keypress = io::stdin().read(&mut buf).map(|_| buf[0]);
							if let Ok(key) = keypress {
								debug!("{:?}", key);
								if b'1' <= key && key <= b'0' + files.len() as u8 {
									winner = SyncOption::Some(key - b'1');
									break;
								} else if key == b's' {
									winner = SyncOption::None;
									break;
								}
							}
						}
					} else {
						// Non-interactive mode: apply configured conflict resolution strategy

						// Build FileVersion vector for the Conflict
						let mut versions = Vec::new();
						for (idx, file_opt) in original_files.iter().enumerate() {
							if let Some(f) = file_opt {
								let node_location = state.nodes.get(idx)
									.map(|n| format!("node_{}", n.id))
									.unwrap_or_else(|| format!("node_{}", idx));
								versions.push(FileVersion {
									node_index: idx,
									node_location,
									file_data: f.as_ref().clone(),
								});
							}
						}

						if versions.is_empty() {
							warn!("No valid file versions for conflict at {}", path.display());
							winner = SyncOption::None;
						} else {
							// Create Conflict object
							let conflict = Conflict::new(
								metrics.conflicts_encountered as u64,
								path.to_path_buf(),
								ConflictType::ModifyModify,
								versions,
							);

							// Create resolver with default strategy
							let resolver = ConflictResolver::new(config.conflict_resolution.clone());

							// Apply strategy
							match resolver.resolve(&conflict, None) {
								Ok(Some(winner_idx)) => {
									winner = SyncOption::Some(winner_idx as u8);
									info!(
										"Conflict resolved using strategy {:?}: node {} wins for {}",
										config.conflict_resolution,
										winner_idx,
										path.display()
									);
									metrics.conflicts_resolved += 1;
								}
								Ok(None) => {
									// Skip strategy
									winner = SyncOption::None;
									info!(
										"Conflict skipped (Skip strategy) for {}",
										path.display()
									);
								}
								Err(e) => {
									warn!(
										"Failed to resolve conflict for {} using strategy {:?}: {}. Skipping.",
										path.display(),
										config.conflict_resolution,
										e
									);
									winner = SyncOption::None;
								}
							}
						}
					}
				}
				if let SyncOption::Some(win) = winner {
					// Use original_files (not deduped) since winner is a node index
					if let Some(Some(w)) = original_files.get(win as usize) {
						//state.tree.insert(path.clone(), win);
						//tree.insert(path.clone(), win);
						tree.insert(path.clone(), (*w).clone());
					} else {
						warn!("Winner node {} doesn't have file {} or is out of bounds - skipping tree insert", win, path.display());
					}
				}
				winner
			});
		}
	}

	info!("Diff loop completed! Processed {} files", file_count);
	//println!("DIFF: {:?}", diff);

	// TUI mode: if we have conflicts, wait for all resolutions (blocking)
	let conflicts_to_resolve = detected_conflicts.into_inner();
	info!(
		"After diff: detected {} conflicts, conflict_rx present: {}",
		conflicts_to_resolve.len(),
		conflict_rx.is_some()
	);

	if let Some(conflict_rx) = conflict_rx {
		if !conflicts_to_resolve.is_empty() {
			info!("Waiting for {} conflicts to be resolved...", conflicts_to_resolve.len());

			// Track which conflicts have been resolved by path
			let mut resolved_paths = std::collections::HashSet::new();
			let total_conflicts = conflicts_to_resolve.len();

			// Wait for resolutions in any order (blocking recv)
			while resolved_paths.len() < total_conflicts {
				match conflict_rx.recv() {
					Ok(resolution) => {
						// Find the conflict that matches this resolution
						let mut found = false;
						for (conflict_path, conflict_files) in &conflicts_to_resolve {
							let path_str = conflict_path.to_string_lossy().to_string();
							if resolution.path == path_str {
								// Check if already resolved (user might press the key multiple times)
								if resolved_paths.contains(&path_str) {
									debug!(
										"Conflict {} already resolved, ignoring duplicate",
										path_str
									);
									found = true;
									break;
								}

								if resolution.chosen_node < conflict_files.len() {
									info!(
										"Conflict resolved: {} -> node {}",
										conflict_path.display(),
										resolution.chosen_node
									);
									metrics.conflicts_resolved += 1;

									// Apply the resolution to the diff
									// IMPORTANT: Only update if the chosen node actually has the file
									let winner = SyncOption::Some(resolution.chosen_node as u8);
									if let SyncOption::Some(win) = winner {
										if let Some(file) = &conflict_files[win as usize] {
											tree.insert(conflict_path.clone(), file.clone());

											// Update diff to reflect resolution
											// Only update if we successfully got the file
											if let Some(entry) =
												diff.get_mut(conflict_path.as_path())
											{
												*entry =
													SyncOption::Some(resolution.chosen_node as u8);
											}
										} else {
											warn!(
												"Chosen node {} doesn't have file {}, marking as None",
												win,
												conflict_path.display()
											);
											// Node doesn't have the file - mark as skip
											if let Some(entry) =
												diff.get_mut(conflict_path.as_path())
											{
												*entry = SyncOption::None;
											}
										}
									}

									resolved_paths.insert(path_str);
									info!(
										"Progress: {}/{} conflicts resolved",
										resolved_paths.len(),
										total_conflicts
									);
									found = true;
									break;
								} else {
									warn!(
										"Invalid node index in resolution: {}",
										resolution.chosen_node
									);
									found = true;
									break;
								}
							}
						}

						if !found {
							warn!("Received resolution for unknown conflict: {}", resolution.path);
						}
					}
					Err(_) => {
						// Channel closed - skip remaining conflicts
						warn!(
							"Conflict resolution channel closed, {} conflicts remain unresolved",
							total_conflicts - resolved_paths.len()
						);
						break;
					}
				}
			}

			info!("All conflicts resolved! Continuing with sync...");
		}
	}

	// Signal that conflict detection phase is complete
	if let Some(ref cb) = callbacks {
		cb.on_event(SyncCallbackEvent::PhaseChanged {
			phase: SyncPhase::DetectingConflicts,
			is_starting: false,
		});
	}

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
				debug!("File was deleted: {}", prev_path);
				deleted_files.push(prev_path.clone());
			}
		}
	}
	info!("Found {} deleted files", deleted_files.len());

	// Terminal will be automatically restored by TerminalGuard when it goes out of scope

	let json =
		json5::to_string(&tree).map_err(|e| format!("Failed to serialize state to JSON: {}", e))?;
	debug!("JSON: {}", json);
	let fname = config.syncr_dir.clone().join(format!("{}.profile.json", config.profile));
	let mut f = afs::File::create(&fname).await?;
	f.write_all(json.as_bytes()).await?;

	// Do write meta
	info!("Sending metadata...");
	if let Some(ref cb) = callbacks {
		cb.on_event(SyncCallbackEvent::PhaseChanged {
			phase: SyncPhase::TransferringMetadata,
			is_starting: true,
		});
	}
	for node in &state.nodes {
		node.protocol
			.lock()
			.await
			.begin_metadata_transfer()
			.await
			.map_err(|e| Box::new(e) as Box<dyn Error>)?;
	}

	// Count total files that need syncing for progress tracking
	let total_files = diff.iter().filter(|(_, to_do)| matches!(to_do, SyncOption::Some(_))).count();
	let mut files_processed = 0;

	for (path, to_do) in diff {
		if let SyncOption::Some(todo) = to_do {
			let files: Vec<Option<&Box<FileData>>> =
				state.nodes.iter().map(|n| n.dir.get(path)).collect();

			// Get the leader file from the chosen node
			let lfile =
				match files.get(todo as usize).and_then(|f| f.as_ref()) {
					Some(f) => f,
					None => {
						error!("Diff points to node {} for file {} but node doesn't have it - skipping",
						todo, path.display());
						continue;
					}
				};

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
						if !config.dry_run {
							node.write_file(lfile, trans_data).await?;
						}
					}
				}
			}

			// Update progress after each file
			files_processed += 1;
			metrics.files_synced = total_files; // All files in diff were processed

			if let Some(ref cb) = callbacks {
				cb.on_event(SyncCallbackEvent::Progress(ProgressUpdate {
					phase: SyncPhase::TransferringMetadata,
					files_processed,
					files_total: total_files,
					bytes_transferred: 0,
					bytes_total: 0,
					transfer_rate: 0.0,
				}));
			}
		}
	}

	// Send delete commands for files that were deleted
	info!("Sending delete commands...");
	for deleted_path in &deleted_files {
		// Notify callbacks that file deletion is starting
		if let Some(ref cb) = callbacks {
			cb.on_event(SyncCallbackEvent::FileOperation {
				path: deleted_path.clone(),
				operation: "delete",
				is_starting: true,
				file_size: 0,
				from_node: 0,
				to_node: 0, // Deletes go to all nodes
			});
		}

		if !config.dry_run {
			for node in &state.nodes {
				let path = std::path::Path::new(deleted_path.as_str());
				node.protocol
					.lock()
					.await
					.send_delete(path)
					.await
					.map_err(|e| Box::new(e) as Box<dyn Error>)?;
			}
		}

		metrics.files_deleted += 1;

		// Notify callbacks that file deletion is complete
		if let Some(ref cb) = callbacks {
			cb.on_event(SyncCallbackEvent::FileOperation {
				path: deleted_path.clone(),
				operation: "delete",
				is_starting: false,
				file_size: 0,
				from_node: 0,
				to_node: 0,
			});
		}
	}

	// Metadata phase complete - send final progress
	if let Some(ref cb) = callbacks {
		cb.on_event(SyncCallbackEvent::PhaseChanged {
			phase: SyncPhase::TransferringMetadata,
			is_starting: false,
		});
		cb.on_event(SyncCallbackEvent::Progress(ProgressUpdate {
			phase: SyncPhase::TransferringMetadata,
			files_processed: total_files,
			files_total: total_files,
			bytes_transferred: 0,
			bytes_total: 0,
			transfer_rate: 0.0,
		}));
	}

	// Sync missing chunks from protocol to NodeState
	// (Protocol's missing set was populated during send_metadata calls)
	for node in &state.nodes {
		let protocol_missing = node.protocol.lock().await.get_missing_chunks().await;
		let mut node_missing = node.missing.lock().await;
		for chunk_hash in protocol_missing {
			node_missing.insert(chunk_hash);
		}
	}

	// Do chunk transfers
	info!("Transfering data chunks...");
	if let Some(ref cb) = callbacks {
		cb.on_event(SyncCallbackEvent::PhaseChanged {
			phase: SyncPhase::TransferringChunks,
			is_starting: true,
		});
	}

	// Count total unique chunks and their sizes for progress tracking
	let mut all_missing_chunks = BTreeSet::new();
	let mut chunk_sizes: std::collections::HashMap<String, usize> =
		std::collections::HashMap::new();

	// First pass: collect all missing chunks from all nodes
	for node in &state.nodes {
		let missing = node.missing.lock().await;
		for chunk_hash in missing.iter() {
			all_missing_chunks.insert(chunk_hash.clone());
		}
	}

	// Second pass: build mapping of chunk hash to size from all nodes' files
	// This must be separate from the first pass so that chunks are added to chunk_sizes
	// even if their source node is processed before the destination node marks them as missing
	for node in &state.nodes {
		for file_data in node.dir.values() {
			for chunk in &file_data.chunks {
				let hash_b64 = crate::util::hash_to_base64(&chunk.hash);
				if all_missing_chunks.contains(&hash_b64) {
					chunk_sizes.insert(hash_b64, chunk.size as usize);
				}
			}
		}
	}

	// Calculate total bytes to transfer
	let mut total_bytes_to_transfer: u64 = 0;
	for hash in &all_missing_chunks {
		if let Some(&size) = chunk_sizes.get(hash) {
			total_bytes_to_transfer += size as u64;
		}
	}

	let total_chunks = all_missing_chunks.len();
	let mut chunks_transferred = 0;
	let mut bytes_transferred: u64 = 0;
	let chunk_transfer_start = std::time::Instant::now();
	info!(
		"Total chunks to transfer: {} ({:.2} MB)",
		total_chunks,
		total_bytes_to_transfer as f64 / 1_000_000.0
	);

	// Track bytes transferred from each source node to each destination node for statistics
	let mut node_to_node_bytes: std::collections::HashMap<(usize, usize), u64> =
		std::collections::HashMap::new();

	let mut done: BTreeSet<String> = BTreeSet::new();
	for (src_idx, srcnode) in state.nodes.iter().enumerate() {
		debug!("[CHUNK_TRANSFER] Processing node {}: {}", src_idx, srcnode.id);

		// Collect all chunks to request from this source
		let mut chunks_to_request = Vec::new();
		for (dst_idx, dstnode) in state.nodes.iter().enumerate() {
			if dst_idx != src_idx {
				let missing = dstnode.missing.lock().await;
				debug!("[CHUNK_TRANSFER] Node {} missing {} chunks", dstnode.id, missing.len());
				for chunk_b64 in missing.iter() {
					if !done.contains(chunk_b64) {
						// Convert base64 hash to binary for chunks lookup
						match crate::util::base64_to_hash(chunk_b64) {
							Ok(chunk_hash) => {
								if srcnode.chunks.contains(&chunk_hash) {
									debug!(
										"[CHUNK_TRANSFER] Source node {} has chunk {}, will request",
										srcnode.id, chunk_b64
									);
									chunks_to_request.push(chunk_b64.clone());
									done.insert(chunk_b64.clone());
								} else {
									debug!(
										"[CHUNK_TRANSFER] Source node {} does NOT have chunk {}",
										srcnode.id, chunk_b64
									);
								}
							}
							Err(e) => {
								error!(
									"[CHUNK_TRANSFER] Failed to convert base64 hash {}: {}",
									chunk_b64, e
								);
							}
						}
					}
				}
			}
		}

		info!(
			"[CHUNK_TRANSFER] Requesting {} chunks from node {} ({}) for transfer",
			chunks_to_request.len(),
			srcnode.id,
			src_idx
		);

		if chunks_to_request.is_empty() {
			debug!("[CHUNK_TRANSFER] No chunks to request from this source, skipping");
			continue;
		}

		// Close WRITE session before entering READ mode
		debug!("[CHUNK_TRANSFER] Ending metadata transfer for node {}", srcnode.id);
		srcnode.protocol.lock().await.end_metadata_transfer().await.map_err(|e| {
			error!(
				"[CHUNK_TRANSFER] Failed to end metadata transfer for node {}: {}",
				srcnode.id, e
			);
			Box::new(e) as Box<dyn Error>
		})?;

		// Begin chunk transfer and request chunks
		debug!("[CHUNK_TRANSFER] Beginning chunk transfer for node {}", srcnode.id);
		srcnode.protocol.lock().await.begin_chunk_transfer().await.map_err(|e| {
			error!(
				"[CHUNK_TRANSFER] Failed to begin chunk transfer for node {}: {}",
				srcnode.id, e
			);
			Box::new(e) as Box<dyn Error>
		})?;

		debug!(
			"[CHUNK_TRANSFER] Requesting {} chunks from node {}",
			chunks_to_request.len(),
			srcnode.id
		);
		srcnode
			.protocol
			.lock()
			.await
			.request_chunks(&chunks_to_request)
			.await
			.map_err(|e| {
				error!("[CHUNK_TRANSFER] Failed to request chunks from node {}: {}", srcnode.id, e);
				Box::new(e) as Box<dyn Error>
			})?;

		// Receive chunks
		debug!("[CHUNK_TRANSFER] Starting to receive chunks from node {}", srcnode.id);
		let mut chunks_received = 0;
		loop {
			let chunk_opt = srcnode.protocol.lock().await.receive_chunk().await.map_err(|e| {
				error!("[CHUNK_TRANSFER] Failed to receive chunk from node {}: {}", srcnode.id, e);
				Box::new(e) as Box<dyn Error>
			})?;

			let Some(chunk) = chunk_opt else {
				debug!(
					"[CHUNK_TRANSFER] Finished receiving chunks from node {} (received {})",
					srcnode.id, chunks_received
				);
				break; // No more chunks
			};

			chunks_received += 1;
			let hash_str = &chunk.hash;
			let chunk_data = chunk.data;
			let chunk_size = chunk_data.len() as u64;
			debug!(
				"[CHUNK_TRANSFER] Received chunk {} from node {} (size: {} bytes)",
				hash_str, srcnode.id, chunk_size
			);

			// Send chunk to nodes that need it
			for (dst_idx, dstnode) in state.nodes.iter().enumerate() {
				if dst_idx != src_idx {
					let mut missing = dstnode.missing.lock().await;
					if missing.contains(hash_str) {
						debug!(
							"[CHUNK_TRANSFER] Sending chunk {} to node {} ({})",
							hash_str, dstnode.id, dst_idx
						);
						dstnode
							.protocol
							.lock()
							.await
							.send_chunk(hash_str, &chunk_data)
							.await
							.map_err(|e| {
								error!(
									"[CHUNK_TRANSFER] Failed to send chunk {} to node {}: {}",
									hash_str, dstnode.id, e
								);
								Box::new(e) as Box<dyn Error>
							})?;

						missing.remove(hash_str);
						// Track bytes sent from source to destination node
						*node_to_node_bytes.entry((src_idx, dst_idx)).or_insert(0) += chunk_size;

						// Update progress for chunk transfer
						chunks_transferred += 1;
						bytes_transferred += chunk_size;

						// Update metrics
						metrics.chunks_transferred = chunks_transferred;
						metrics.bytes_transferred = bytes_transferred;

						// Send progress update (throttled to every 10 chunks or 100ms)
						if chunks_transferred % 10 == 0
							|| chunk_transfer_start.elapsed().as_millis() >= 100
						{
							let elapsed = chunk_transfer_start.elapsed().as_secs_f64();
							let transfer_rate = if elapsed > 0.0 {
								(bytes_transferred as f64 / 1_000_000.0) / elapsed
							} else {
								0.0
							};

							if let Some(ref cb) = callbacks {
								cb.on_event(SyncCallbackEvent::Progress(ProgressUpdate {
									phase: SyncPhase::TransferringChunks,
									files_processed: chunks_transferred,
									files_total: total_chunks,
									bytes_transferred,
									bytes_total: total_bytes_to_transfer,
									transfer_rate,
								}));
							}
						}
					}
				}
			}
		}

		// READ session auto-closes when server sends END marker
		// Now re-enter WRITE mode so this source can receive chunks from other sources
		srcnode
			.protocol
			.lock()
			.await
			.begin_metadata_transfer()
			.await
			.map_err(|e| Box::new(e) as Box<dyn Error>)?;
	}

	// Close all WRITE sessions
	for node in &state.nodes {
		node.protocol
			.lock()
			.await
			.end_metadata_transfer()
			.await
			.map_err(|e| Box::new(e) as Box<dyn Error>)?;
	}

	// Emit FileOperation events for actual chunk transfers
	// This properly tracks which nodes actually sent chunks to which other nodes
	for ((src_idx, dst_idx), bytes) in node_to_node_bytes.iter() {
		if *bytes > 0 {
			if let Some(ref cb) = callbacks {
				// We don't have individual file names for chunks, so use a generic name
				cb.on_event(SyncCallbackEvent::FileOperation {
					path: format!("(chunks: {} bytes)", bytes),
					operation: "chunk_transfer",
					is_starting: false,
					file_size: *bytes,
					from_node: *src_idx,
					to_node: *dst_idx,
				});
			}
		}
	}

	// Chunk transfer phase complete - send final progress
	if let Some(ref cb) = callbacks {
		cb.on_event(SyncCallbackEvent::PhaseChanged {
			phase: SyncPhase::TransferringChunks,
			is_starting: false,
		});
		cb.on_event(SyncCallbackEvent::Progress(ProgressUpdate {
			phase: SyncPhase::TransferringChunks,
			files_processed: total_chunks,
			files_total: total_chunks,
			bytes_transferred: total_bytes_to_transfer,
			bytes_total: total_bytes_to_transfer,
			transfer_rate: 0.0,
		}));
	}

	// Commit modifications (do renames)
	info!("Commiting changes...");
	if let Some(ref cb) = callbacks {
		cb.on_event(SyncCallbackEvent::PhaseChanged {
			phase: SyncPhase::Committing,
			is_starting: true,
		});
	}
	let total_nodes = state.nodes.len();

	// CRITICAL: Pre-commit verification - verify all chunks received before committing
	// This prevents data corruption if chunks didn't fully transfer
	debug!("Verifying all chunks received before commit...");
	for node in &state.nodes {
		let missing = node.missing.lock().await;
		if !missing.is_empty() {
			// Find which files would be corrupted by missing chunks
			let mut affected_files = Vec::new();
			'file_loop: for (path, file_data) in &node.dir {
				for chunk in &file_data.chunks {
					let hash_b64 = crate::util::hash_to_base64(&chunk.hash);
					if missing.contains(&hash_b64) {
						affected_files.push(path.clone());
						break 'file_loop;
					}
				}
			}

			// Create detailed error message
			let affected_list = affected_files
				.iter()
				.take(10)
				.map(|p| format!("  - {}", p.display()))
				.collect::<Vec<_>>()
				.join("\n");

			let missing_list = missing
				.iter()
				.take(5)
				.map(|h| format!("  - {}", h))
				.collect::<Vec<_>>()
				.join("\n");

			let error_msg = format!(
				"CRITICAL: Node {} has {} unreceived chunks. Cannot commit - would corrupt files:\n{}\n\
				 Missing chunks (showing first 5):\n{}",
				node.id,
				missing.len(),
				affected_list,
				missing_list
			);

			error!("{}", error_msg);
			return Err(error_msg.into());
		}
	}

	for (nodes_committed, node) in state.nodes.iter().enumerate() {
		let commit_response = node
			.protocol
			.lock()
			.await
			.commit()
			.await
			.map_err(|e| Box::new(e) as Box<dyn Error>)?;

		if !commit_response.success {
			let msg = commit_response.message.unwrap_or_else(|| "Unknown error".to_string());
			return Err(format!("Node {} failed to commit: {}", node.id, msg).into());
		}

		// Send progress after each node commits
		if let Some(ref cb) = callbacks {
			cb.on_event(SyncCallbackEvent::Progress(ProgressUpdate {
				phase: SyncPhase::Committing,
				files_processed: nodes_committed + 1,
				files_total: total_nodes,
				bytes_transferred: 0,
				bytes_total: 0,
				transfer_rate: 0.0,
			}));
		}
	}

	// Cleanup is handled by cleanup_nodes below
	Ok(())
}

/// Gracefully shut down child processes by sending QUIT and waiting for acknowledgment
async fn cleanup_nodes(state: &SyncState) {
	info!("Shutting down child processes...");
	for node in &state.nodes {
		// Close the protocol connection (sends QUIT)
		if let Err(e) = node.protocol.lock().await.close().await {
			warn!("Failed to close connection to node {}: {}", node.id, e);
		} else {
			debug!("Node {} connection closed", node.id);
		}
	}
	info!("All child processes shut down");
}

// vim: ts=4
