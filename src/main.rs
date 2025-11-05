use async_std::task;
use clap::{Arg, ArgAction, Command};
use std::error::Error;
use std::{env, fs, path};

use crate::types::Config;

mod config;
mod connect;
mod serve;
mod sync;
mod types;
mod util;

///////////////////////
// Utility functions //
///////////////////////

fn init_syncr_dir() -> path::PathBuf {
	match env::var("HOME") {
		Ok(home) => {
			let syncr_dir = path::PathBuf::from(home).join(".syncr");
			println!("rcfile: {:?}", syncr_dir);

			match fs::metadata(&syncr_dir) {
				Ok(meta) => {
					if meta.is_dir() {
						return syncr_dir;
					} else {
						eprintln!(
							"{} exists, but it is not a directory!",
							syncr_dir.to_str().unwrap()
						);
						panic!()
					}
				}
				Err(_err) => {
					// Not exists
					if let Err(err) = fs::create_dir(&syncr_dir) {
						panic!("Cannot create directory: {:?}", err);
					}
					return syncr_dir;
				}
			}
		}
		Err(_e) => {
			eprintln!("Could not determine HOME directory!");
			panic!()
		}
	}
}

fn main() -> Result<(), Box<dyn Error>> {
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
		let dir = matches.get_one::<String>("dir").expect("ERROR");
		return serve::serve(dir);
	} else if let Some(matches) = matches.subcommand_matches("dump") {
		let dir = matches.get_one::<String>("dir").expect("ERROR");
		env::set_current_dir(dir)?;
		let dump_state = serve::serve_list(path::PathBuf::from("."))?;

		for (h, p) in &dump_state.chunks {
			eprintln!("{}: {:?}", h, p);
		}
	} else if let Some(sub_matches) = matches.subcommand_matches("sync") {
		let config = Config {
			syncr_dir: init_syncr_dir(),
			profile: matches
				.get_one::<String>("profile")
				.map(|s| s.as_str())
				.unwrap_or("default")
				.to_string(),
		};

		let dirs: Vec<&str> = sub_matches
			.get_many::<String>("dir")
			.expect("ERROR")
			.map(|s| s.as_str())
			.collect();
		let _ = task::block_on(sync::sync(config, dirs));
	}

	Ok(())
}

// vim: ts=4
