use clap::{Arg, ArgAction, Command};
use std::error::Error;
use std::{env, fs, path};

use crate::types::Config;

mod cache;
mod config;
mod conflict;
mod connect;
mod logging;
mod metadata_utils;
mod node_labels;
mod progress;
mod protocol;
mod protocol_utils;
mod serve;
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
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
			Command::new("dump")
				.about("Dump directory data")
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
		logging::init_protocol_propagation();
		let dir = matches.get_one::<String>("dir").ok_or("serve: directory argument required")?;
		return serve::serve(dir);
	} else if let Some(matches) = matches.subcommand_matches("dump") {
		// Dump mode: use standard tracing for stderr output
		logging::init_tracing();
		let dir = matches.get_one::<String>("dir").ok_or("dump: directory argument required")?;
		env::set_current_dir(dir)?;
		let dump_state = serve::serve_list(path::PathBuf::from("."))?;

		for (h, p) in &dump_state.chunks {
			info!("{}: {:?}", h, p);
		}
	} else if let Some(sub_matches) = matches.subcommand_matches("sync") {
		let config = Config {
			syncr_dir: init_syncr_dir()?,
			profile: matches
				.get_one::<String>("profile")
				.map(|s| s.as_str())
				.unwrap_or("default")
				.to_string(),
		};

		let dirs: Vec<&str> = sub_matches
			.get_many::<String>("dir")
			.ok_or("sync: at least one directory argument required")?
			.map(|s| s.as_str())
			.collect();

		// Check if TUI mode requested
		#[cfg(feature = "tui")]
		if sub_matches.get_flag("tui") {
			// TUI mode: Tracing will be initialized inside run_tui with broadcast channel
			return tui::run_tui(config, dirs).await;
		}

		#[cfg(not(feature = "tui"))]
		if sub_matches.get_flag("tui") {
			eprintln!("TUI not available. Rebuild with: cargo build --features tui");
			return Err("TUI support not compiled".into());
		}

		// CLI sync mode: determine sync behavior based on flags
		let show_progress = sub_matches.get_flag("progress");
		let quiet = sub_matches.get_flag("quiet");
		let skip_conflicts = sub_matches.get_flag("skip-conflicts");

		// Initialize logging (unless quiet mode)
		if !quiet {
			logging::init_tracing();
		}

		// Select sync mode based on flag combination
		// Note: quiet mode suppresses everything
		if quiet {
			// Quiet mode: no output, skip conflicts
			sync_impl::sync(config, dirs).await?;
		} else if show_progress && skip_conflicts {
			// Progress display, but skip conflicts
			sync_impl::sync_with_cli_progress(config, dirs).await?;
		} else if show_progress && !skip_conflicts {
			// Progress display with interactive conflict resolution
			sync_impl::sync_with_progress_and_conflicts(config, dirs).await?;
		} else if !show_progress && skip_conflicts {
			// No progress, skip conflicts
			sync_impl::sync(config, dirs).await?;
		} else {
			// Default mode: interactive conflict resolution, no progress display
			// (will still show logs if verbosity is set)
			sync_impl::sync_with_conflicts(config, dirs).await?;
		}
	} else if let Some(unlock_matches) = matches.subcommand_matches("unlock") {
		// Unlock mode
		logging::init_tracing();

		let config = Config {
			syncr_dir: init_syncr_dir()?,
			profile: matches
				.get_one::<String>("profile")
				.map(|s| s.as_str())
				.unwrap_or("default")
				.to_string(),
		};

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
