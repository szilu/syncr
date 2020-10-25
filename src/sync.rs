use async_process;
use async_std::{prelude::*, io as aio};
use futures::future;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::{path, pin::Pin};

use crate::types::{HashChunk, FileData};
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
        if trans_data {
            writeln!(self.send.borrow_mut(), "FD:{}:{}:{}:{}:{}:{}", file.path.to_str().expect(""), file.mode, file.user, file.group, file.size, file.mtime).await?;
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
            writeln!(self.send.borrow_mut(), "FM:{}:{}:{}:{}:{}:{}", file.path.to_str().expect(""), file.mode, file.user, file.group, file.size, file.mtime).await?;
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
                        path: path.clone(),
                        mode: fields[2].parse().expect("Child parse error"),
                        user: fields[3].parse().expect("Child parse error"),
                        group: fields[4].parse().expect("Child parse error"),
                        size: fields[5].parse().expect("Child parse error"),
                        mtime: fields[6].parse().expect("Child parse error"),
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
                        Some(data) => { &data.chunks.push(hc); },
                        None => { panic!("FIXME"); }
                    }
                    self.chunks.insert(String::from(fields[3]));
                    //file_data.chunks.push(hc);
                },
                _ => panic!("Child parse error: {}", buf.trim())
                //_ => return Err("Child parse error").into()
                //_ => return Result::new(Box::new(Err("Child parse error")))
            }
        }

        Ok(())
    }
}

struct SyncState {
    nodes: Vec<Box<NodeState>>
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

pub async fn sync(dirs: Vec<&str>) -> Result<(), Box<dyn Error>> {
    let mut state = SyncState { nodes: Vec::new() };

    eprintln!("Initializing processes...");
    for dir in dirs {
        let mut conn = connect::connect(dir).await?;
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
    let mut diff: BTreeMap<&path::Path, Option<u8>> = BTreeMap::new();
    for node in &state.nodes {
        for (path, _) in &node.dir {
            diff.entry(&path).or_insert_with(|| {
                let mut files: Vec<Option<&Box<FileData>>> = state.nodes.iter().map(|n| n.dir.get(path)).collect();
                let mut latest: Option<u8> = None;

                // Compare files on all nodes and find where to sync from
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
            });
        }
    }
    //println!("DIFF: {:?}", diff);

    // Do write meta
    eprintln!("Sending metadata...");
    for node in &state.nodes {
        node.send("WRITE").await?;
    }
    for (path, to_do) in diff {
        if let Some(todo) = to_do {
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
