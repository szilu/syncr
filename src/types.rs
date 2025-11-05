use serde::ser::{Serialize, Serializer, SerializeStruct};
use std::path;

#[derive(Debug)]
pub struct Config {
    pub syncr_dir: path::PathBuf,
    pub profile: String
}

#[derive(Debug)]
pub struct FileChunk {
    pub path: path::PathBuf,
    pub offset: u64,
    pub size: usize
}

#[derive(Clone, PartialEq, Debug)]
pub struct HashChunk {
    pub hash: String,
    pub offset: u64,
    pub size: usize
}

#[derive(Clone, PartialEq, Debug)]
pub enum FileType {
    File,
    Dir,
    SymLink
}

#[derive(Clone, PartialEq, Debug)]
pub struct FileData {
    pub tp: FileType,
    pub path: path::PathBuf,
    pub mode: u32,
    pub user: u32,
    pub group: u32,
    pub ctime: u32,
    pub mtime: u32,
    pub size: u64,
    pub chunks: Vec<Box<HashChunk>>
}

impl Serialize for FileData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("File", 2)?;
        match &self.tp {
            FileType::File => state.serialize_field("type", "F")?,
            FileType::SymLink => state.serialize_field("type", "L")?,
            FileType::Dir => state.serialize_field("type", "D")?
        };
        state.serialize_field("path", &self.path.to_str())?;
        state.end()
    }
}
