use clap::{Arg, ArgAction, Command};
use std::error::Error;
use std::{env, fs, path};

use crate::types::Config;

mod config;
mod connect;
mod metadata_utils;
mod protocol_utils;
mod serve;
mod sync_impl; // Private sync implementation
mod types;
mod util;

///////////////////////
// Utility functions //
///////////////////////

fn init_syncr_dir() -> Result<path::PathBuf, Box<dyn Error>> {
	match env::var("HOME") {
		Ok(home) => {
			let syncr_dir = path::PathBuf::from(home).join(".syncr");
			eprintln!("rcfile: {:?}", syncr_dir);

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
		.version("0.1.0")
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
				.arg(Arg::new("dir").required(true).action(ArgAction::Append).num_args(1..)),
		)
		.get_matches();

	if let Some(matches) = matches.subcommand_matches("serve") {
		let dir = matches.get_one::<String>("dir").ok_or("serve: directory argument required")?;
		return serve::serve(dir);
	} else if let Some(matches) = matches.subcommand_matches("dump") {
		let dir = matches.get_one::<String>("dir").ok_or("dump: directory argument required")?;
		env::set_current_dir(dir)?;
		let dump_state = serve::serve_list(path::PathBuf::from("."))?;

		for (h, p) in &dump_state.chunks {
			eprintln!("{}: {:?}", h, p);
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
		let _ = sync_impl::sync(config, dirs).await;
	}

	Ok(())
}

// vim: ts=4
