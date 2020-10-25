use async_std::{task};
use clap::{Arg, App, SubCommand};
use std::{env, path};
use std::error::Error;

mod config;
mod connect;
mod serve;
mod sync;
mod types;
mod util;

///////////////////////
// Utility functions //
///////////////////////

fn main() -> Result<(), Box<dyn Error>> {
    let matches = App::new("SyncR").version("0.1").author("Szilard Hajba <szilard@symbion.hu>")
        .about("2-way directory sync utility")
        .arg(Arg::with_name("profile")
            .short("p").long("profile").takes_value(true).help("Profile"))
        .subcommand(SubCommand::with_name("serve")
            .about("Serving mode (used internally)")
            .arg(Arg::with_name("dir").required(true))
        )
        .subcommand(SubCommand::with_name("dump")
            .about("Dump directory data")
            .arg(Arg::with_name("dir").required(true))
        )
        .subcommand(SubCommand::with_name("sync")
            .about("Sync directories")
            .arg(Arg::with_name("dir").required(true).multiple(true))
        )
        .get_matches();

    if let Some(matches) = matches.subcommand_matches("serve") {
        let dir = matches.value_of("dir").expect("ERROR");
        return serve::serve(&dir)
    } else if let Some(matches) = matches.subcommand_matches("dump") {
        let dir = matches.value_of("dir").expect("ERROR");
        env::set_current_dir(&dir)?;
        let dump_state = serve::serve_list(path::PathBuf::from("."))?;

        for (h, p) in &dump_state.chunks {
            println!("{}: {:?}", h, p);
        }
    } else if let Some(matches) = matches.subcommand_matches("sync") {
        let dirs: Vec<&str> = matches.values_of("dir").expect("ERROR").collect();
        return task::block_on(sync::sync(dirs));
    }

    Ok(())
}

// vim: ts=4
