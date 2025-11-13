use clap::{Arg, ArgAction, ArgMatches, Command};
use std::error::Error;
use std::str::FromStr;
use std::{env, fs, path};

use crate::config::Config;
use crate::conflict::ConflictResolver;
use crate::strategies::{ConflictResolution, DeleteMode};

mod cache;
mod chunking; // Renamed from config to avoid conflict with new config/ module
mod config; // New comprehensive config module (config/)
mod conflict;
mod connect;
mod delete;
mod exclusion; // Exclusion and filtering engine
mod logging;
mod metadata;
mod metadata_utils;
mod node_labels;
mod progress;
mod protocol;
mod protocol_utils;
mod serve;
mod strategies; // Consolidated strategy enums
pub mod sync_impl; // Sync implementation (types are public for callbacks)
mod types;
mod util;
mod utils;

use logging::*;

// TUI module (only compiled with 'tui' feature)
#[cfg(feature = "tui")]
mod tui;

///////////////////////
// Utility functions //
///////////////////////

fn init_syncr_dir() -> Result<path::PathBuf, Box<dyn Error>> {
	match env::var("HOME") {
		Ok(home) => {
			let syncr_dir = path::PathBuf::from(home).join(".syncr");
			debug!("rcfile: {:?}", syncr_dir);

			match fs::metadata(&syncr_dir) {
				Ok(meta) => {
					if meta.is_dir() {
						Ok(syncr_dir)
					} else {
						Err(format!("{} exists, but it is not a directory!", syncr_dir.display())
							.into())
					}
				}
				Err(_err) => {
					// Not exists
					fs::create_dir(&syncr_dir)
						.map_err(|err| format!("Cannot create directory: {}", err))?;
					Ok(syncr_dir)
				}
			}
		}
		Err(_e) => Err("Could not determine HOME directory!".into()),
	}
}

/// Build unified Config from CLI arguments
#[allow(clippy::field_reassign_with_default, clippy::clone_on_copy)]
fn build_config_from_cli(
	syncr_dir: path::PathBuf,
	profile: String,
	matches: &ArgMatches,
) -> Result<Config, Box<dyn Error>> {
	let mut config = Config::default();
	config.syncr_dir = syncr_dir;
	config.profile = profile;

	// Dry run
	config.dry_run = matches.get_flag("dry-run");

	// Exclude patterns
	if let Some(patterns) = matches.get_many::<String>("exclude") {
		config.exclude_patterns = patterns.map(|s| s.to_string()).collect();
	}

	// Include patterns
	if let Some(patterns) = matches.get_many::<String>("include") {
		config.include_patterns = patterns.map(|s| s.to_string()).collect();
	}

	// Delete mode
	if let Some(mode_str) = matches.get_one::<String>("delete") {
		config.delete_mode = DeleteMode::from_str(mode_str)?;
	}

	// Conflict resolution strategy
	if let Some(strategy_str) = matches.get_one::<String>("conflict") {
		config.conflict_resolution = ConflictResolution::from_str(strategy_str)?;
	}

	// Handle --skip-conflicts flag (backward compatibility)
	if matches.get_flag("skip-conflicts") {
		config.conflict_resolution = ConflictResolution::Skip;
	}

	// Checksum
	config.always_checksum = matches.get_flag("checksum");

	// Compress
	config.compress = matches.get_flag("compress");

	// Log level
	if let Some(level) = matches.get_one::<String>("log-level") {
		config.log_level = level.to_string();
	}

	Ok(config)
}

// TODO: Config file loading will be reimplemented in Phase 3
// For now, we only support CLI options and defaults

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
	// Debug: Log all arguments to a file
	use std::io::Write;
	if let Ok(mut f) = std::fs::OpenOptions::new()
		.create(true)
		.append(true)
		.open("/tmp/syncr_debug.log")
	{
		let args: Vec<String> = std::env::args().collect();
		let _ = writeln!(f, "Arguments: {:?}", args);
		let _ = writeln!(f, "Number of args: {}", args.len());
	}

	let matches = Command::new("SyncR")
		.version(env!("CARGO_PKG_VERSION"))
		.author("Szilard Hajba <szilard@symbion.hu>")
		.about("2-way directory sync utility")
		.subcommand_required(true)
		.arg(
			Arg::new("profile")
				.short('p')
				.long("profile")
				.value_name("PROFILE")
				.help("Profile"),
		)
		.subcommand(
			Command::new("serve")
				.about("Serving mode (used internally)")
				.arg(Arg::new("dir").required(true)),
		)
		.subcommand(
			Command::new("sync")
				.about("Sync directories")
				.arg(Arg::new("dir").required(true).action(ArgAction::Append).num_args(1..))
				.arg(
					Arg::new("tui")
						.long("tui")
						.help("Use terminal UI (requires 'tui' feature)")
						.action(ArgAction::SetTrue),
				)
				.arg(
					Arg::new("progress")
						.short('p')
						.long("progress")
						.help("Show progress display during sync")
						.action(ArgAction::SetTrue),
				)
				.arg(
					Arg::new("quiet")
						.short('q')
						.long("quiet")
						.help("Suppress all output (no progress, minimal logs)")
						.action(ArgAction::SetTrue),
				)
				.arg(
					Arg::new("skip-conflicts")
						.long("skip-conflicts")
						.help("Skip conflicts instead of prompting for resolution")
						.action(ArgAction::SetTrue),
				)
				// Phase 1 Critical CLI flags
				.arg(
					Arg::new("dry-run")
						.short('n')
						.long("dry-run")
						.help("Show what would be done without making changes")
						.action(ArgAction::SetTrue),
				)
				.arg(
					Arg::new("exclude")
						.long("exclude")
						.value_name("PATTERN")
						.help("Exclude files matching PATTERN (can be repeated)")
						.action(ArgAction::Append),
				)
				.arg(
					Arg::new("include")
						.long("include")
						.value_name("PATTERN")
						.help("Include files matching PATTERN, overrides exclude (can be repeated)")
						.action(ArgAction::Append),
				)
				.arg(
					Arg::new("delete")
						.long("delete")
						.value_name("MODE")
						.help("Delete mode: sync, no-delete, delete-after, delete-excluded, trash")
						.value_parser(["sync", "no-delete", "delete-after", "delete-excluded", "trash"]),
				)
				.arg(
					Arg::new("conflict")
						.long("conflict")
						.value_name("STRATEGY")
						.help("Conflict resolution: ask, skip, newest, oldest, largest, smallest, first, last, fail")
						.value_parser(["ask", "skip", "newest", "oldest", "largest", "smallest", "first", "last", "fail"]),
				)
				.arg(
					Arg::new("checksum")
						.short('c')
						.long("checksum")
						.help("Always use checksums for comparison (skip based on checksum, not mod-time)")
						.action(ArgAction::SetTrue),
				)
				.arg(
					Arg::new("compress")
						.short('z')
						.long("compress")
						.help("Compress data during transfer")
						.action(ArgAction::SetTrue),
				)
				.arg(
					Arg::new("log-level")
						.long("log-level")
						.value_name("LEVEL")
						.help("Log level: error, warn, info, debug, trace")
						.value_parser(["error", "warn", "info", "debug", "trace"]),
				),
		)
		.subcommand(
			Command::new("unlock")
				.about("Release locks on paths (dangerous, use with caution!)")
				.arg(Arg::new("path").required(true).help("Path to unlock"))
				.arg(
					Arg::new("force")
						.long("force")
						.help("Force unlock even if process appears alive")
						.action(ArgAction::SetTrue),
				),
		)
		.get_matches();

	if let Some(matches) = matches.subcommand_matches("serve") {
		// Serve mode: use protocol propagation for logs
		// Note: init_protocol_propagation() is called INSIDE serve() after ready signal
		let dir = matches.get_one::<String>("dir").ok_or("serve: directory argument required")?;
		return serve::serve(dir).await;
	} else if let Some(sub_matches) = matches.subcommand_matches("sync") {
		let syncr_dir = init_syncr_dir()?;
		let profile = matches
			.get_one::<String>("profile")
			.map(|s| s.as_str())
			.unwrap_or("default")
			.to_string();

		let dirs: Vec<&str> = sub_matches
			.get_many::<String>("dir")
			.ok_or("sync: at least one directory argument required")?
			.map(|s| s.as_str())
			.collect();

		// Build config from CLI arguments
		let config = build_config_from_cli(syncr_dir.clone(), profile.clone(), sub_matches)?;

		// TODO: Phase 3: Load config file and merge with CLI options

		// Check if TUI mode requested
		#[cfg(feature = "tui")]
		if sub_matches.get_flag("tui") {
			// TUI mode: Tracing will be initialized inside run_tui with broadcast channel
			return tui::run_tui(config, dirs).await;
		}

		#[cfg(not(feature = "tui"))]
		if sub_matches.get_flag("tui") {
			warn!("TUI not available. Rebuild with: cargo build --features tui");
			return Err("TUI support not compiled".into());
		}

		// CLI sync mode: determine sync behavior based on flags
		let show_progress = sub_matches.get_flag("progress");
		let quiet = sub_matches.get_flag("quiet");

		// Check if conflict resolution strategy is automatic (not Interactive)
		// Default to Interactive if not specified
		let is_automatic_resolution = ConflictResolver::is_automatic(&config.conflict_resolution);

		// Initialize logging (unless quiet mode)
		// Apply log level from CLI if specified
		if !quiet {
			if !config.log_level.is_empty() {
				let log_level = &config.log_level;
				env::set_var("RUST_LOG", log_level);
			}
			logging::init_tracing();
		}

		// Show dry-run message if enabled
		if config.dry_run {
			info!("DRY RUN MODE: No changes will be made");
		}

		// Select sync mode based on flag combination
		// Note: quiet mode suppresses everything
		if quiet {
			// Quiet mode: no output, no interaction
			sync_impl::sync(config, dirs).await?;
		} else if is_automatic_resolution {
			// Automatic conflict resolution (Skip, Newest, etc.)
			if show_progress {
				// Progress display with automatic conflict resolution
				sync_impl::sync_with_cli_progress(config, dirs).await?;
			} else {
				// No progress, automatic resolution
				sync_impl::sync(config, dirs).await?;
			}
		} else {
			// Interactive conflict resolution (Interactive strategy)
			if show_progress {
				// Progress display with interactive conflict prompts
				sync_impl::sync_with_progress_and_conflicts(config, dirs).await?;
			} else {
				// No progress, interactive conflict resolution
				sync_impl::sync_with_conflicts(config, dirs).await?;
			}
		}
	} else if let Some(unlock_matches) = matches.subcommand_matches("unlock") {
		// Unlock mode
		logging::init_tracing();

		let syncr_dir = init_syncr_dir()?;
		let profile = matches
			.get_one::<String>("profile")
			.map(|s| s.as_str())
			.unwrap_or("default")
			.to_string();

		// SyncCliOptions removed - use Config directly
		let config = Config { syncr_dir, profile, ..Config::default() };

		let path = unlock_matches
			.get_one::<String>("path")
			.ok_or("unlock: path argument required")?;
		let force = unlock_matches.get_flag("force");

		// Open the cache database for the default profile
		let db_path = config.syncr_dir.join(format!("{}.db", config.profile));
		let cache = cache::ChildCache::open(&db_path)?;

		// Get lock info before removing
		if let Ok(Some(lock_info)) = cache.get_lock_info(path) {
			info!(
				"Lock found for '{}': PID {}, started {}",
				path, lock_info.pid, lock_info.started
			);

			// Check if lock is stale or force is set
			if force {
				info!("Force unlocking path: {}", path);
			} else if lock_info.is_stale() {
				info!("Lock is stale (process {} is dead), removing it", lock_info.pid);
			} else {
				return Err(format!(
					"Cannot unlock '{}': locked by active process {} (PID). Use --force to override.",
					path, lock_info.pid
				)
				.into());
			}

			// Remove the lock from the database
			let write_txn = cache.db.begin_write()?;
			{
				let mut table = write_txn.open_table(cache::ACTIVE_SYNCS_TABLE)?;
				table.remove(path.as_str())?;
			}
			write_txn.commit()?;
			info!("Successfully unlocked path: {}", path);
		} else {
			info!("No lock found for path: {}", path);
		}
	}

	Ok(())
}

// vim: ts=4
