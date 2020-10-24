use async_std::{prelude::*, task, fs as afs};
use base64;
use glob;
use rollsum::Bup;
use std::cell::RefCell;
use std::collections::{BTreeMap};
use std::{env, fs, path, io, pin::Pin};
use std::error::Error;
use std::io::{Write};
use std::os::unix::{fs::MetadataExt, prelude::PermissionsExt};
//use std::{thread, time};

use crate::config;
use crate::util;
use crate::types::{FileChunk, HashChunk, FileData};

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
    pub rename: RefCell<BTreeMap<path::PathBuf, path::PathBuf>>
}

impl DumpState {
    fn add_chunk(self: &mut DumpState, hash: String, path: path::PathBuf, offset: u64, size: usize) {
        let v = self.chunks.entry(hash).or_insert(Vec::new());
        if v.iter().position(|p| &p.path == &path).is_none() {
            v.push(Box::new(FileChunk {path, offset, size}));
        }
    }

    async fn read_chunk(&self, dir: &path::Path, hash: &str) -> Result<Option<Vec<u8>>, Box<dyn Error>> {
        let fc_vec_opt = self.chunks.get(hash);

        match fc_vec_opt {
            Some(fc_vec) => {
                let fc = &fc_vec[0];
                let path = dir.join(&fc.path);
                let mut f = afs::File::open(&path).await?;
                let mut buf: Vec<u8> = vec![0; fc.size];
                f.seek(io::SeekFrom::Start(fc.offset)).await?;
                f.read(&mut buf).await?;
                // FIXME: Hash check, error handling
                //eprintln!("Hash check: {} ?= {}", util::hash(&buf), hash);
                Ok(Some(buf))
            },
            None => Ok(None)
        }
    }

    async fn write_chunk(&self, path: &path::Path, chunk: &HashChunk, buf: &Vec<u8>) -> Result<(), Box<dyn Error>> {
        let mut f = afs::OpenOptions::new().write(true).create(true).open(&path).await?;
        f.seek(io::SeekFrom::Start(chunk.offset)).await?;
        f.write_all(&buf).await?;
        Ok(())
    }
}

fn traverse_dir<'a>(mut state: &'a mut DumpState, dir: path::PathBuf) -> Pin<Box<dyn Future<Output=Result<(), Box<dyn Error>>> + 'a>> {
Box::pin(async move {
	for entry in fs::read_dir(&dir)? {
		let entry = entry?;
		let path = entry.path();
        if state.exclude[0].matches_path(&path) {
            continue;
        }

		let meta = fs::metadata(&path)?;

		if meta.is_file() {
            println!("F:{}:{}:{}:{}:{}:{}", &path.to_str().unwrap(), meta.mode(), meta.uid(), meta.gid(), meta.size(), meta.mtime());

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
                if let Some(count) = bup.find_chunk_edge(&buf[..endofs]) {
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
        if meta.is_dir() {
            println!("D:{}:{}:{}", path.to_str().unwrap(), meta.uid(), meta.gid());
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
        rename: RefCell::new(BTreeMap::new())
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
        if buf.trim() == "." { break; }
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
                let encoded = base64::encode(buf);
                println!("C:{}", chunk);
                for line in encoded.into_bytes().chunks(config::BASE64_LINE_LENGTH) {
                    io::stdout().write_all(line)?;
                    io::stdout().write_all(b"\n")?;
                }
                println!(".");
            },
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
        io::stdin().read_line(&mut buf).expect("Failed to read");
        let fields: Vec<&str> = buf.trim().split(':').collect();

        match fields[0] {
            "FM" | "FD" => {
                let path = path::PathBuf::from(fields[1]);
                let fd = Box::new(FileData {
                    path: path.clone(),
                    mode: fields[2].parse().expect("Child parse error"),
                    user: fields[3].parse().expect("Child parse error"),
                    group: fields[4].parse().expect("Child parse error"),
                    size: fields[5].parse().expect("Child parse error"),
                    mtime: fields[6].parse().expect("Child parse error"),
                    chunks: vec![]
                });
                if fields[0] == "FD" {
                    filepath = path.clone();
                    let mut filename = path.file_name().expect("Protocol error!").to_os_string();
                    filename.push(".SyNcR-TmP");
                    filepath.set_file_name(filename);
                    //eprintln!("CREATE {:?}", &filepath);
                    file = Some(afs::File::create(&filepath).await?);
                    afs::set_permissions(&filepath, afs::Permissions::from_mode(fd.mode)).await?;
                    dump_state.rename.borrow_mut().insert(filepath.clone(), path.clone());
                }
            },
            "LC" | "RC" => {
                if file.is_none() {
                    panic!("Protocol error!");
                }
                let hc = Box::new(HashChunk {
                    hash: String::from(fields[3]),
                    offset: fields[1].parse().expect("Child parse error"),
                    size: fields[2].parse().expect("Child parse error")
                });
                if fields[0] == "LC" {
                    // Local chunk, copy it locally
                    let buf = dump_state.read_chunk(&dir, fields[3]).await?.expect("Chunk not found");
                    if let Err(e) = dump_state.write_chunk(&filepath, &hc, &buf).await {
                        println!("ERROR {}", e);
                    }
                } else {
                    // Remote chunk, add to wait list
                    let mut missing = dump_state.missing.borrow_mut();
                    let v = missing.entry(String::from(fields[3])).or_insert(Vec::new());
                    v.push(Box::new(FileChunk {
                        path: filepath.clone(),
                        offset: fields[1].parse().expect("Child parse error"),
                        size: fields[2].parse().expect("Child parse error")
                    }));
                }
            },
            "C" => {
                let mut buf = String::new();
                let hash = fields[1];
                let mut chunk: Vec<u8> = Vec::new();
                loop {
                    buf.clear();
                    io::stdin().read_line(&mut buf).expect("Failed to read");
                    if buf.trim() == "." {
                        break;
                    }
                    //eprintln!("DECODE: [{:?}]", &buf.trim());
                    chunk.append(&mut base64::decode(&buf.trim())?);
                }
                //eprintln!("DECODED CHUNK: {:?}", chunk);
                let mut missing = dump_state.missing.borrow_mut();
                match missing.get(hash) {
                    Some(fc_vec) => {
                        for fc in fc_vec {
                            let hc = HashChunk {
                                hash: String::from(hash),
                                offset: fc.offset,
                                size: fc.size
                            };
                            //let filepath = tmp_filename(&fc.path);
                            if let Err(e) = dump_state.write_chunk(&fc.path, &hc, &chunk).await {
                                eprintln!("ERROR WRITING {}", e);
                            }
                        }
                        missing.remove(hash);
                    },
                    None => {}
                }
            },
            "." => {
                if file.is_some() {
                    file = None;
                } else {
                    break;
                }
            }
            _ => panic!("Child parse error: {}", buf.trim())
        }
    }
    println!("OK");
    Ok(())
}

async fn serve_commit(_FIXME_dir: path::PathBuf, dump_state: &DumpState) -> Result<(), Box<dyn Error>> {
    if dump_state.missing.borrow().len() > 0 {
        eprintln!("FIXME: ERROR");
    }
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
            _ => println!("E:UNK-CMD: Unknown command: {}", &cmdline.trim())
        }
    }
    Ok(())
}
