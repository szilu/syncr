use std::path;

#[derive(Debug)]
pub struct FileChunk {
    pub path: path::PathBuf,
    pub offset: u64,
    pub size: usize
}

#[derive(PartialEq, Debug)]
pub struct HashChunk {
    pub hash: String,
    pub offset: u64,
    pub size: usize
}

#[derive(PartialEq, Debug)]
pub struct FileData {
    pub path: path::PathBuf,
    pub mode: u32,
    pub user: u32,
    pub group: u32,
    pub size: u64,
    pub mtime: u32,
    pub chunks: Vec<Box<HashChunk>>
}
