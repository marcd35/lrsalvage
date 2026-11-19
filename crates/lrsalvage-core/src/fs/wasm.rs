//! The `wasm32-unknown-unknown` backend: every operation, honestly refused.
//!
//! This is not the browser implementation, which is its own slice (#117). It is
//! the difference between a target that cannot answer and one that answers
//! wrongly: `std::fs` on this target compiles, and `Path::exists` is `false`
//! whatever is at the path, which is how a present `previews.db` was reported as
//! `Missing` (`docs/browser-spike.md` §1).
//!
//! So each function below returns [`io::ErrorKind::Unsupported`] with one
//! message. A caller that wants a `bool` and no reason writes `unwrap_or(false)`
//! and gets the old silence; every caller that can tell the difference gets it.
//! When the browser backend lands, this file is what it replaces, and the seam's
//! tests are the specification it has to satisfy.

use std::io;
use std::path::{Path, PathBuf};

/// The one refusal this target gives, until a browser backend answers.
const NOT_AVAILABLE: &str = "the filesystem is not available on this target";

fn not_available() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, NOT_AVAILABLE)
}

/// A scratch directory that this target cannot create; never constructed.
pub(super) struct ScratchDir(PathBuf);

impl ScratchDir {
    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

pub(super) fn read(_path: &Path) -> io::Result<Vec<u8>> {
    Err(not_available())
}

pub(super) fn read_prefix(_path: &Path, _len: usize) -> io::Result<Vec<u8>> {
    Err(not_available())
}

pub(super) fn exists(_path: &Path) -> io::Result<bool> {
    Err(not_available())
}

pub(super) fn is_file(_path: &Path) -> io::Result<bool> {
    Err(not_available())
}

pub(super) fn list_files(_dir: &Path) -> io::Result<Vec<PathBuf>> {
    Err(not_available())
}

pub(super) fn walk_files(_root: &Path) -> io::Result<Vec<PathBuf>> {
    Err(not_available())
}

pub(super) fn create_dir_all(_dir: &Path) -> io::Result<()> {
    Err(not_available())
}

pub(super) fn write(_path: &Path, _bytes: &[u8]) -> io::Result<()> {
    Err(not_available())
}

pub(super) fn create_new(_path: &Path) -> io::Result<()> {
    Err(not_available())
}

pub(super) fn write_atomic(_path: &Path, _bytes: &[u8]) -> io::Result<()> {
    Err(not_available())
}

pub(super) fn remove_file(_path: &Path) -> io::Result<()> {
    Err(not_available())
}

pub(super) fn copy(_from: &Path, _to: &Path) -> io::Result<()> {
    Err(not_available())
}

pub(super) fn scratch_dir(_prefix: &str) -> io::Result<ScratchDir> {
    Err(not_available())
}

pub(super) fn sha256_file(_path: &Path) -> io::Result<String> {
    Err(not_available())
}

pub(super) fn canonicalize(_path: &Path) -> io::Result<PathBuf> {
    Err(not_available())
}

pub(super) fn current_dir() -> io::Result<PathBuf> {
    Err(not_available())
}

pub(super) fn set_mtime(_path: &Path, _unix_seconds: i64) -> io::Result<()> {
    Err(not_available())
}

pub(super) fn available_space(_path: &Path) -> io::Result<u64> {
    Err(not_available())
}
