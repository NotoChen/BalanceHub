//! Native files are opened through the workbench boundary before decoding.
use std::{
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
};

pub(crate) fn compressed(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".jsonl.zst"))
}
pub(crate) fn json_lines(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "jsonl")
        || compressed(path)
}
pub(crate) fn logical_path(path: &Path) -> PathBuf {
    if compressed(path) {
        path.with_extension("")
    } else {
        path.to_path_buf()
    }
}
pub(crate) fn existing_variant(path: &Path) -> PathBuf {
    if path.exists() {
        return path.to_path_buf();
    }
    let sibling = if compressed(path) {
        logical_path(path)
    } else {
        PathBuf::from(format!("{}.zst", path.to_string_lossy()))
    };
    if sibling.exists() {
        sibling
    } else {
        path.to_path_buf()
    }
}
pub(crate) fn reader(path: &Path) -> io::Result<Box<dyn BufRead + Send>> {
    let file = super::workbench::open_session_file(path)?;
    if compressed(path) {
        Ok(Box::new(BufReader::new(zstd::stream::read::Decoder::new(
            file,
        )?)))
    } else {
        Ok(Box::new(BufReader::new(file)))
    }
}
