//! Main TUI application and event loop

use crossterm::{
	event::{self, DisableMouseCapture, EnableMouseCapture, Event as CEvent},
	execute,
	terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::error::Error;
use std::io::{self, Write};
use tokio::sync::{broadcast, mpsc};

use crate::config::Config;

use super::{
	event::{SyncEvent, TickGenerator},
	state::{AppState, LogLevel, ViewType},
	views,
};

/// RAII guard for TUI terminal state
/// Ensures terminal is properly cleaned up even if panic occurs or signal is received
struct TuiGuard;

impl TuiGuard {
	/// Setup terminal in raw mode with alternate screen
	fn new() -> Result<Self, Box<dyn Error>> {
		enable_raw_mode()?;
		Ok(TuiGuard)
	}
}

impl Drop for TuiGuard {
	fn drop(&mut self) {
		// Restore terminal even if panic occurs or signal is received
		// This is critical because signal handlers no longer call exit()

		// Disable raw mode
		let _ = disable_raw_mode();

		// Restore alternate screen and mouse
		let mut stdout = io::stdout();
		let _ = execute!(stdout, LeaveAlternateScreen, DisableMouseCapture);

		// Show cursor if it's hidden
		let _ = write!(io::stdout(), "\x1B[?25h");
		let _ = io::stdout().flush();
	}
}

/// Commands sent from TUI to sync engine
#[derive(Debug, Clone)]
pub enum TuiCommand {
	#[allow(dead_code)]
	StartSync,
	#[allow(dead_code)]
	PauseSync,
	#[allow(dead_code)]
	ResumeSync,
	#[allow(dead_code)]
	AbortSync,
	#[allow(dead_code)]
	ResolveConflict { conflict_id: u64, chosen_index: usize },
}

/// Main TUI application
pub struct TuiApp {
	state: AppState,
	event_rx: broadcast::Receiver<SyncEvent>,
	event_tx: broadcast::Sender<SyncEvent>,
	config: Config,
	command_tx: mpsc::Sender<TuiCommand>,
	sync_thread: Option<std::thread::JoinHandle<()>>,
}

impl TuiApp {
	/// Create a new TUI application
	pub fn new(
		config: Config,
		locations: Vec<String>,
		event_rx: broadcast::Receiver<SyncEvent>,
		event_tx: broadcast::Sender<SyncEvent>,
		command_tx: mpsc::Sender<TuiCommand>,
	) -> Self {
		TuiApp {
			state: AppState::new(config.clone(), locations),
			event_rx,
			event_tx,
			config,
			command_tx,
			sync_thread: None,
		}
	}

	/// Run the TUI application event loop
	pub async fn run<B: ratatui::backend::Backend>(
		&mut self,
		terminal: &mut Terminal<B>,
	) -> Result<(), Box<dyn Error>> {
		// Start sync immediately
		self.state.change_view(ViewType::Sync);
		self.spawn_sync()?;

		let tick_gen = TickGenerator::new(60); // 60 FPS

		loop {
			// Increment animation frame for UI animations
			self.state.ui.animation_frame = self.state.ui.animation_frame.wrapping_add(1);

			// Render current state
			terminal.draw(|f| self.render(f))?;
			// Wait for next event with timeout
			// Use biased to prioritize ticks for animation
			tokio::select! {
				biased;

				// Tick for animations - check this FIRST to ensure animation continues
				_ = tick_gen.next_tick() => {
					// Animation frame already incremented at top of loop
				}

				// Check for sync events (broadcast)
				result = self.event_rx.recv() => {
					match result {
						Ok(sync_event) => {
							self.handle_sync_event(sync_event).await?;
						}
						Err(broadcast::error::RecvError::Lagged(_)) => {
							// Receiver lagged, drop old messages
						}
						Err(broadcast::error::RecvError::Closed) => {
							// Channel closed, sync engine stopped
							break;
						}
					}
				}

				// Check for terminal input
				result = async {
					if event::poll(std::time::Duration::from_millis(10)).unwrap_or(false) {
						event::read().ok()
					} else {
						None
					}
				} => {
					if let Some(cevent) = result {
						match cevent {
							CEvent::Key(key) => self.handle_key(key).await?,
							CEvent::Mouse(_mouse) => {
								// TODO: Handle mouse events
							}
							CEvent::Resize(_, _) => {
								// Terminal resized, will redraw automatically
							}
							_ => {}
						}
					}
				}
			}

			// Check if should quit
			if self.state.should_quit {
				break;
			}
		}

		// Wait for sync thread to finish (cleanup, lock file deletion, etc.)
		// This ensures proper cleanup even if the sync thread is still running
		if let Some(handle) = self.sync_thread.take() {
			// Drop the conflict resolution sender to unblock the sync thread if it's waiting
			// This ensures the receiver in the sync thread gets Err(RecvError::Disconnected)
			// allowing it to break out of the conflict resolution wait loop
			self.state.sync.conflict_resolution_tx = None;

			eprintln!("Waiting for sync thread to finish...");
			let _ = handle.join();
			eprintln!("Sync thread finished");
		}

		Ok(())
	}

	/// Handle keyboard input
	async fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Result<(), Box<dyn Error>> {
		use crossterm::event::{KeyCode, KeyModifiers};

		// Global shortcuts
		match (key.code, key.modifiers) {
			(KeyCode::Char('c'), KeyModifiers::CONTROL)
			| (KeyCode::Char('q'), KeyModifiers::NONE) => {
				self.state.should_quit = true;
				return Ok(());
			}
			(KeyCode::Char('?'), KeyModifiers::NONE) => {
				self.state.change_view(ViewType::Help);
				return Ok(());
			}
			_ => {}
		}

		// Track view before handling input
		let prev_view = self.state.current_view;

		// View-specific handling
		match self.state.current_view {
			ViewType::Setup => {
				views::setup::handle_key(&mut self.state, key, &self.command_tx).await?
			}
			ViewType::Sync => {
				views::tabs::handle_key(&mut self.state, key, &self.command_tx).await?
			}
			ViewType::Help => views::help::handle_key(&mut self.state, key).await?,
		}

		// If view changed from Setup to Sync, spawn the sync task
		if prev_view == ViewType::Setup
			&& self.state.current_view == ViewType::Sync
			&& !self.state.sync.is_running
		{
			self.spawn_sync()?;
		}

		Ok(())
	}

	/// Handle sync events from the sync engine
	async fn handle_sync_event(&mut self, event: SyncEvent) -> Result<(), Box<dyn Error>> {
		match event {
			SyncEvent::PhaseStarted { phase } => {
				use crate::tui::state::TabType;
				use crate::types::SyncPhase;

				self.state.sync.phase = Some(phase);
				self.state.add_log(LogLevel::Info, format!("Phase started: {}", phase));

				// Auto-switch to appropriate tab for the phase
				if self.state.current_view == ViewType::Sync {
					match phase {
						SyncPhase::Collecting => {
							self.state.sync.active_tab = TabType::Nodes;
						}
						_ => {
							// Keep current tab for other phases
						}
					}
				}
			}

			SyncEvent::PhaseCompleted { phase } => {
				self.state.add_log(LogLevel::Info, format!("Phase completed: {}", phase));
			}

			SyncEvent::PhaseChanged { phase } => {
				self.state.sync.phase = Some(phase);
				self.state.add_log(LogLevel::Info, format!("Phase: {:?}", phase));
			}

			SyncEvent::Progress { stats } => {
				self.state.sync.progress = Some(stats);
			}

			SyncEvent::NodeConnected { index, location } => {
				if let Some(node) = self.state.sync.nodes.get_mut(index) {
					node.connected = true;
				}
				self.state.add_log(LogLevel::Success, format!("Connected: {}", location));
			}

			SyncEvent::NodeConnectionFailed { index, location, error } => {
				if let Some(node) = self.state.sync.nodes.get_mut(index) {
					node.connected = false;
					node.error = Some(error.clone());
				}
				self.state.add_log(
					LogLevel::Error,
					format!("Connection failed: {} - {}", location, error),
				);
			}

			SyncEvent::NodeReady { index, location } => {
				if let Some(node) = self.state.sync.nodes.get_mut(index) {
					node.connected = true;
				}
				self.state.add_log(LogLevel::Success, format!("Node ready: {}", location));
			}

			SyncEvent::NodeDisconnecting { index } => {
				if let Some(node) = self.state.sync.nodes.get_mut(index) {
					node.connected = false;
				}
				self.state.add_log(LogLevel::Info, format!("Node {} disconnecting", index));
			}

			SyncEvent::NodeStats { index, files_known, bytes_known } => {
				if let Some(node) = self.state.sync.nodes.get_mut(index) {
					node.files_collected = files_known;
					node.bytes_collected = bytes_known;
				}
			}

			SyncEvent::FileDiscovered { path, node_index, exists } => {
				use crate::tui::state::FileNodeStatus;

				let status = if exists { FileNodeStatus::Exists } else { FileNodeStatus::Missing };
				self.state.track_file(path, node_index, status, false);
			}

			SyncEvent::FileOperationStarted { path, operation } => {
				self.state.add_log(
					LogLevel::Info,
					format!("File operation started: {} ({})", path.display(), operation),
				);
				// Track active operation
				use crate::tui::state::FileOperation;
				self.state.sync.active_operations.push(FileOperation {
					path: path.clone(),
					from_node: 0,
					to_node: 0,
					progress: 0.0,
					bytes_total: 0,
					bytes_done: 0,
				});
			}

			SyncEvent::FileOperationCompleted { path, operation, size, from_node, to_node } => {
				self.state.add_log(
					LogLevel::Success,
					format!(
						"File operation completed: {} ({}) - {} bytes",
						path.display(),
						operation,
						size
					),
				);
				// Remove from active operations
				self.state.sync.active_operations.retain(|op| op.path != path);

				// Track sent/received statistics (skip deletes as they don't transfer data)
				if operation != "delete" && size > 0 {
					// Update sent count for source node
					if let Some(source_node) = self.state.sync.nodes.get_mut(from_node) {
						source_node.files_sent += 1;
						source_node.bytes_sent += size;
					}
					// Update received count for destination node
					if let Some(dest_node) = self.state.sync.nodes.get_mut(to_node) {
						dest_node.files_received += 1;
						dest_node.bytes_received += size;
					}
				}

				// Track file status
				use crate::tui::state::FileNodeStatus;
				if operation == "delete" {
					self.state.track_file(path.clone(), to_node, FileNodeStatus::Missing, false);
				} else {
					// Mark as exists on source, synced on destination
					self.state.track_file(path.clone(), from_node, FileNodeStatus::Exists, false);
					self.state.track_file(path, to_node, FileNodeStatus::Synced, false);
				}
			}

			SyncEvent::FileSync { path, from_node, to_nodes } => {
				use crate::tui::state::FileNodeStatus;

				let msg =
					format!("Synced: {} (node{} → node{:?})", path.display(), from_node, to_nodes);
				self.state.add_log(LogLevel::Info, msg);

				// Mark file as exists on source node
				self.state.track_file(path.clone(), from_node, FileNodeStatus::Exists, false);

				// Mark file as pending/syncing on target nodes
				for &node in &to_nodes {
					self.state.track_file(path.clone(), node, FileNodeStatus::Pending, false);
				}
			}

			SyncEvent::FileDelete { path, node } => {
				use crate::tui::state::FileNodeStatus;

				let msg = format!("Deleted from node{}: {}", node, path.display());
				self.state.add_log(LogLevel::Info, msg);

				// Mark file as missing on this node
				self.state.track_file(path, node, FileNodeStatus::Missing, false);
			}

			SyncEvent::DirCreate { path, node } => {
				let msg = format!("Created dir on node{}: {}", node, path.display());
				self.state.add_log(LogLevel::Info, msg);
			}

			SyncEvent::ConflictDetected { path, description: _, node_mtimes } => {
				use crate::tui::state::{ConflictEntry, TabType};

				self.state
					.add_log(LogLevel::Warning, format!("Conflict detected: {}", path.display()));

				// Add conflict to the list
				self.state.sync.conflicts.push(ConflictEntry {
					path: path.clone(),
					resolution: None,
					node_mtimes,
				});

				// Auto-select first conflict if none selected
				if self.state.sync.selected_conflict_index.is_none() {
					self.state.sync.selected_conflict_index = Some(0);
				}

				// Mark file as a conflict in the files list (status already set by FileDiscovered events)
				if let Some(file) = self.state.sync.files.iter_mut().find(|f| f.path == path) {
					file.is_conflict = true;
				}

				// Auto-switch to Conflicts tab if in Sync view
				if self.state.current_view == ViewType::Sync {
					self.state.sync.active_tab = TabType::Conflicts;
				}
			}

			SyncEvent::ConflictResolved { path, winner } => {
				let msg = if let Some(node) = winner {
					format!("Conflict resolved: {} (chose node{})", path.display(), node)
				} else {
					format!("Conflict skipped: {}", path.display())
				};
				self.state.add_log(LogLevel::Success, msg);
			}

			SyncEvent::Error { error } => {
				self.state.add_log(LogLevel::Error, error.clone());
				eprintln!("Non-fatal sync error: {}", error);
			}

			SyncEvent::Log { level, message } => {
				self.state.add_log(level, message);
			}

			SyncEvent::Completed { result } => {
				use crate::tui::state::TabType;

				self.state.sync.result = Some(result);
				self.state.sync.is_running = false;
				self.state.sync.conflict_resolution_tx = None; // Clear the channel
				self.state
					.add_log(LogLevel::Success, "Sync completed successfully!".to_string());
				eprintln!("Sync completed successfully");

				// Switch to Nodes tab to show completion statistics
				if self.state.current_view == ViewType::Sync {
					self.state.sync.active_tab = TabType::Nodes;
				}
			}

			SyncEvent::Failed { error } => {
				use crate::tui::state::TabType;

				self.state.sync.is_running = false;
				self.state.sync.conflict_resolution_tx = None; // Clear the channel
				self.state.add_log(LogLevel::Error, format!("Sync failed: {}", error));
				eprintln!("Critical sync failure: {}", error);

				// Switch to Logs tab to show the error
				if self.state.current_view == ViewType::Sync {
					self.state.sync.active_tab = TabType::Logs;
				}
			}
		}

		Ok(())
	}

	/// Render the current view
	fn render(&mut self, frame: &mut ratatui::Frame) {
		match self.state.current_view {
			ViewType::Setup => views::setup::render(frame, &self.state),
			ViewType::Sync => views::tabs::render(frame, &self.state),
			ViewType::Help => views::help::render(frame, &self.state),
		}
	}

	/// Spawn the sync task in the background
	pub fn spawn_sync(&mut self) -> Result<(), Box<dyn std::error::Error>> {
		// Mark sync as running
		self.state.sync.is_running = true;
		self.state.change_view(ViewType::Sync);

		// Create conflict resolution channel (using blocking channel for simplicity)
		let (conflict_tx, conflict_rx) = std::sync::mpsc::channel();

		// Store the sender in state so the UI can send resolutions
		self.state.sync.conflict_resolution_tx = Some(conflict_tx);

		// Convert locations to owned Vec<String> for the task
		let locations: Vec<String> = self.state.locations.clone();

		// Clone what we need for the spawned task
		let config = self.config.clone();
		let event_tx = self.event_tx.clone();

		// Spawn the sync task in a separate thread with its own runtime
		// This avoids runtime nesting issues
		let handle = std::thread::spawn(move || {
			eprintln!("Sync task started in dedicated thread");

			// Create a bridge that will send events through the broadcast channel
			let bridge = crate::tui::bridge::TuiBridge::new(event_tx.clone());

			// Convert Vec<String> to Vec<&str> for the sync function
			let location_refs: Vec<&str> = locations.iter().map(|s| s.as_str()).collect();

			// Run sync with callbacks and conflict resolution receiver
			// Create a dedicated single-threaded runtime for this sync
			let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
				let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
					Ok(rt) => rt,
					Err(e) => {
						eprintln!("Failed to create runtime: {}", e);
						let _ = event_tx.send(crate::tui::SyncEvent::Failed {
							error: format!("Failed to create runtime: {}", e),
						});
						return Err(format!("Failed to create runtime: {}", e).into());
					}
				};

				rt.block_on(async {
					crate::sync_impl::sync_with_callbacks(
						config,
						location_refs,
						Box::new(bridge),
						Some(conflict_rx),
					)
					.await
				})
			}));

			let result = match result {
				Ok(r) => r,
				Err(e) => {
					// Try to extract panic message
					let panic_msg = if let Some(s) = e.downcast_ref::<&str>() {
						s.to_string()
					} else if let Some(s) = e.downcast_ref::<String>() {
						s.clone()
					} else {
						format!("{:?}", e)
					};
					eprintln!("PANIC in sync task: {}", panic_msg);
					let _ = event_tx.send(crate::tui::SyncEvent::Failed {
						error: format!("Sync panicked: {}", panic_msg),
					});
					return;
				}
			};

			match result {
				Ok(sync_result) => {
					// Send completion event with actual statistics
					let _ = event_tx.send(crate::tui::SyncEvent::Completed { result: sync_result });
					eprintln!("Sync completed successfully");
				}
				Err(e) => {
					let _ = event_tx.send(crate::tui::SyncEvent::Failed { error: e.to_string() });
					eprintln!("Sync failed: {}", e);
				}
			}
		});
		// Store the thread handle so we can wait for it to finish on shutdown
		self.sync_thread = Some(handle);

		Ok(())
	}
}

/// Entry point for TUI mode
pub async fn run_tui(config: Config, dirs: Vec<&str>) -> Result<(), Box<dyn Error>> {
	// Setup terminal with automatic cleanup on drop
	// This guard ensures the terminal is restored even if panic occurs or signal is received
	let _tui_guard = TuiGuard::new()?;

	// Create broadcast channel for sync events FIRST
	// This must happen before initializing tracing
	let (event_tx, event_rx) = broadcast::channel(100);

	// Initialize tracing subscriber that forwards to TUI
	// Parent tracing events (including re-emitted child logs) go through this channel
	crate::logging::init_tui_tracing(event_tx.clone());

	// Setup alternate screen and mouse capture
	let mut stdout = io::stdout();
	execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
	let backend = CrosstermBackend::new(stdout);
	let mut terminal = Terminal::new(backend)?;

	// Create command channel
	let (command_tx, _command_rx) = mpsc::channel(32);

	// Create TUI app
	let locations: Vec<String> = dirs.iter().map(|s| s.to_string()).collect();
	let mut app = TuiApp::new(
		config.clone(),
		locations.clone(),
		event_rx,
		event_tx.clone(),
		command_tx.clone(),
	);

	eprintln!("TUI started - ready for sync");
	eprintln!("Locations: {:?}", locations);

	// Sync will be spawned when user presses Enter on Setup screen
	// See handle_key() -> spawn_sync() flow (triggered by Setup -> Sync view transition)

	// Run TUI event loop
	// Terminal cleanup (raw mode, alternate screen) happens automatically when _tui_guard drops
	app.run(&mut terminal).await
}

// vim: ts=4
