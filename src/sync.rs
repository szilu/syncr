use async_process;
use async_std::{prelude::*, fs as afs};
use futures::future;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io::{self, Read};
use std::{path, pin::Pin};
use termios::{Termios, TCSANOW, ECHO, ICANON, tcsetattr};
use serde_json;

use crate::types::{Config, HashChunk, FileType, FileData};
use crate::connect;

//////////
// Sync //
//////////
struct NodeState {
    id: u8,
    send: RefCell<async_process::ChildStdin>,
    recv: RefCell<async_std::io::BufReader<async_process::ChildStdout>>,
    dir: BTreeMap<path::PathBuf, Box<FileData>>,
    chunks: BTreeSet<String>,
    missing: RefCell<BTreeSet<String>>
}

impl PartialEq for NodeState {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl NodeState {
    async fn write_file(&self, file: &FileData, trans_data: bool) -> Result<(), Box<dyn Error>> {
        match file.tp {
            FileType::File => {
                if trans_data {
                    writeln!(self.send.borrow_mut(), "FD:{}:{}:{}:{}:{}:{}:{}", file.path.to_str().expect(""), file.mode, file.user, file.group, file.ctime, file.mtime, file.size).await?;
                    for chunk in &file.chunks {
                        if self.chunks.get(&chunk.hash).is_none() {
                            // Chunk needs transfer
                            writeln!(self.send.borrow_mut(), "RC:{}:{}:{}", chunk.offset, chunk.size, chunk.hash).await?;
                            self.missing.borrow_mut().insert(chunk.hash.clone());
                        } else {
                            // Chunk is available locally
                            writeln!(self.send.borrow_mut(), "LC:{}:{}:{}", chunk.offset, chunk.size, chunk.hash).await?;
                        }
                    }
                    writeln!(self.send.borrow_mut(), ".").await?;
                } else {
                    writeln!(self.send.borrow_mut(), "FM:{}:{}:{}:{}:{}:{}:{}", file.path.to_str().expect(""), file.mode, file.user, file.group, file.ctime, file.mtime, file.size).await?;
                }
            },
            FileType::SymLink => {
                writeln!(self.send.borrow_mut(), "L:{}:{}:{}:{}:{}:{}", file.path.to_str().expect(""), file.mode, file.user, file.group, file.ctime, file.mtime).await?;
            },
            FileType::Dir => {
                writeln!(self.send.borrow_mut(), "D:{}:{}:{}:{}:{}:{}", file.path.to_str().expect(""), file.mode, file.user, file.group, file.ctime, file.mtime).await?;
            }
        }
        Ok(())
    }

    async fn send(&self, buf: &str) -> Result<(), Box<dyn Error>> {
        self.send.borrow_mut().write_all(&[&buf, &"\n"[..]].concat().as_bytes()).await?;
        Ok(())
    }

    async fn do_collect(&mut self) -> Result<(), Box<dyn Error>> {
        let mut buf = String::new();
        let mut file_data: Option<&mut Box<FileData>> = None;

        loop {
            buf.clear();
            self.recv.get_mut().read_line(&mut buf).await?;
            if buf.trim() == "." { break; }
            //eprintln!("[{}]HDR: {}", self.id, buf.trim());
        }

        self.send.get_mut().write_all(b"LIST\n").await?;
        loop {
            buf.clear();
            self.recv.get_mut().read_line(&mut buf).await?;
            if buf.trim() == "." { break; }
            //println!("[{}]LINE: {}", self.id, buf.trim());
            let fields: Vec<&str> = buf.trim().split(':').collect();

            match fields[0] {
                "F" => {
                    let path = path::PathBuf::from(fields[1]);
                    let fd = Box::new(FileData {
                        tp: FileType::File,
                        path: path.clone(),
                        mode: fields[2].parse().expect("Child parse error"),
                        user: fields[3].parse().expect("Child parse error"),
                        group: fields[4].parse().expect("Child parse error"),
                        ctime: fields[5].parse().expect("Child parse error"),
                        mtime: fields[6].parse().expect("Child parse error"),
                        size: fields[7].parse().expect("Child parse error"),
                        chunks: vec![]
                    });
                    //file_data = &fd;
                    self.dir.insert(fd.path.clone(), fd);
                    file_data = self.dir.get_mut(&path);
                    //self.dir.insert(path.clone(), file_data);
                },
                "C" => {
                    let hc = Box::new(HashChunk {
                        hash: String::from(fields[3]),
                        offset: fields[1].parse().expect("Child parse error"),
                        size: fields[2].parse().expect("Child parse error")
                    });
                    match &mut file_data {
                        Some(data) => { data.chunks.push(hc); },
                        None => { panic!("FIXME"); }
                    }
                    self.chunks.insert(String::from(fields[3]));
                    //file_data.chunks.push(hc);
                },
                "L" => {
                    let path = path::PathBuf::from(fields[1]);
                    let fd = Box::new(FileData {
                        tp: FileType::SymLink,
                        path: path.clone(),
                        mode: fields[2].parse().expect("Child parse error"),
                        user: fields[3].parse().expect("Child parse error"),
                        group: fields[4].parse().expect("Child parse error"),
                        ctime: fields[5].parse().expect("Child parse error"),
                        mtime: fields[6].parse().expect("Child parse error"),
                        size: 0,
                        chunks: vec![]
                    });
                    //file_data = &fd;
                    self.dir.insert(fd.path.clone(), fd);
                    file_data = self.dir.get_mut(&path);
                    //self.dir.insert(path.clone(), file_data);
                },
                "D" => {
                    let path = path::PathBuf::from(fields[1]);
                    let fd = Box::new(FileData {
                        tp: FileType::Dir,
                        path: path.clone(),
                        mode: fields[2].parse().expect("Child parse error"),
                        user: fields[3].parse().expect("Child parse error"),
                        group: fields[4].parse().expect("Child parse error"),
                        ctime: fields[5].parse().expect("Child parse error"),
                        mtime: fields[6].parse().expect("Child parse error"),
                        size: 0,
                        chunks: vec![]
                    });
                    //file_data = &fd;
                    self.dir.insert(fd.path.clone(), fd);
                    file_data = self.dir.get_mut(&path);
                    //self.dir.insert(path.clone(), file_data);
                },
                _ => panic!("Child parse error: {}", buf.trim())
            }
        }

        Ok(())
    }
}

struct SyncState {
    nodes: Vec<Box<NodeState>>,
    //tree: BTreeMap<path::PathBuf, u8>
    //tree: BTreeMap<path::PathBuf, &FileData>
}

impl SyncState {
    fn add_node(&mut self, send: async_process::ChildStdin, recv: async_std::io::BufReader<async_process::ChildStdout>) {
        let node = Box::new(NodeState {
            id: self.nodes.len() as u8 + 1,
            send: RefCell::new(send),
            recv: RefCell::new(recv),
            dir: BTreeMap::new(),
            chunks: BTreeSet::new(),
            missing: RefCell::new(BTreeSet::new())
        });
        self.nodes.push(node);
    }
}

#[derive(Debug)]
enum SyncOption<T> {
    None,
    Conflict,
    Some(T)
}

impl<T> SyncOption<T> {
    pub fn is_none(&self) -> bool {
        match &self { SyncOption::None => true, _ => false }
    }

    pub fn is_conflict(&self) -> bool {
        match &self { SyncOption::Conflict => true, _ => false }
    }

    pub fn is_some(&self) -> bool {
        match &self { SyncOption::Some(_) => true, _ => false }
    }
}

pub async fn sync(config: Config, dirs: Vec<&str>) -> Result<(), Box<dyn Error>> {
    let mut state = SyncState { nodes: Vec::new() };
    //let mut tree: BTreeMap<path::PathBuf, u8> = BTreeMap::new();
    let mut tree: BTreeMap<path::PathBuf, Box<FileData>> = BTreeMap::new();

    eprintln!("Initializing processes...");
    for dir in dirs {
        let conn = connect::connect(dir).await?;
        state.add_node(conn.send, conn.recv);
    }

    eprintln!("Collecting...");
    let mut futs: Vec<Pin<Box<dyn future::Future<Output=_>>>> = vec![];
    for node in &mut state.nodes {
        futs.push(Box::pin(node.do_collect()));
    }
    future::join_all(futs).await;

    // Do diffing
    eprintln!("Running diff...");

    // Configure terminal for key input
    let stdin = 0;
    let termios = Termios::from_fd(stdin).unwrap();
    let mut new_termios = termios.clone();
    new_termios.c_lflag &= !(ICANON | ECHO);
    tcsetattr(stdin, TCSANOW, &mut new_termios).unwrap();

    let mut diff: BTreeMap<&path::Path, SyncOption<u8>> = BTreeMap::new();
    for node in &state.nodes {
        for (path, _) in &node.dir {
            //let last = state.tree.get(path);
            //eprintln!("kast {:?}", last);
            let last: Option<&FileData> = None;

            diff.entry(&path).or_insert_with(|| {
                let mut files: Vec<Option<&Box<FileData>>> = state.nodes.iter().map(|n| n.dir.get(path)).collect();
                //let mut latest: Option<u8> = None;
                let mut winner: SyncOption<u8> = SyncOption::None;

                // Compare files on all nodes and find where to sync from
                //eprintln!("File: {:?}", &path);
                for (idx, file) in files.iter().enumerate() {
                    if let Some(f) = file {
                        //eprintln!("Node: {} {:?}", idx, last);
                        //eprintln!("         {:?}", f);
                        if let Some(last) = last {
                            if f.tp != last.tp || f.mode != last.mode || f.user != last.user || f.group != last.group || f.chunks != last.chunks {
                                eprintln!("diff {} {} {} {} {}", f.tp != last.tp, f.mode != last.mode, f.user != last.user, f.group != last.group, f.chunks != last.chunks);
                                if winner.is_none() {
                                    winner = SyncOption::Some(idx as u8);
                                } else if winner.is_some() {
                                    winner = SyncOption::Conflict;
                                }
                            }
                        } else {
                            if winner.is_none() {
                                winner = SyncOption::Some(idx as u8);
                            } else if let SyncOption::Some(win) = winner {
                                let w = files[win as usize].unwrap();
                                if f.tp != w.tp || f.mode != w.mode || f.user != w.user || f.group != w.group || f.chunks != w.chunks {
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
                        let keypress = io::stdin().bytes().next().unwrap();
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

    // Reset terminal
    tcsetattr(stdin, TCSANOW, & termios).unwrap();

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
            let files: Vec<Option<&Box<FileData>>> = state.nodes.iter().map(|n| n.dir.get(path)).collect();
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
                        node.write_file(&lfile, trans_data).await?;
                    }
                }
            }
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
                let missing = dstnode.missing.borrow_mut();
                for chunk in missing.iter() {
                    if done.get(chunk).is_none() {
                        //eprintln!("MISSING CHUNK: {} {:?}", chunk, srcnode.chunks.get(chunk));
                        if srcnode.chunks.get(chunk).is_some() {
                            srcnode.send(chunk).await?;
                            done.insert(String::from(chunk));
                        }
                    }
                }
            }
        }
        srcnode.send(".").await?;
        let mut recv = srcnode.recv.borrow_mut();
        let mut buf = String::new();
        let mut chunk = String::new();
        let mut chunkdata = String::new();
        loop {
            buf.clear();
            recv.read_line(&mut buf).await?;
            if chunk == "" && &buf[..2] == "C:" {
                chunk.clear();
                chunk.push_str(&buf.trim()[2..]);
                chunkdata.clear();
            } else if &chunk == "" && buf.trim() == "." {
                break;
            } else if buf.trim() == "." {
                chunkdata.push('.');
                let data = &["C:", &chunk, "\n", &chunkdata].join("");
                for dstnode in &state.nodes {
                    if dstnode != srcnode && dstnode.missing.borrow().get(&chunk).is_some() {
                        // Send chunk
                        dstnode.send(&data).await?;
                        dstnode.missing.borrow_mut().remove(&chunk);
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
    }

    // Quit children
    for node in &state.nodes {
        node.send("QUIT").await?;
        let mut buf = String::new();
        loop {
            buf.clear();
            let n = node.recv.borrow_mut().read_line(&mut buf).await?;
            if n == 0 || buf.trim() == "." { break; }
            //eprintln!("QUIT: {}", buf.trim());
        }
    }

    Ok(())
}
