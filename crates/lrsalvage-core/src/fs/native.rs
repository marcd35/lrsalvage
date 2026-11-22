//! The native backend: the calls the modules already made, in one place.
//!
//! Every function here is a thin wrapper over the `std::fs`, `tempfile`,
//! `walkdir`, `filetime` or `sha2` call that used to sit in the module which
//! needed it. Nothing decides anything; the seam's doc comment in the parent
//! module is the contract.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use walkdir::WalkDir;

/// A `tempfile` directory that removes itself.
pub(super) struct ScratchDir(tempfile::TempDir);

impl ScratchDir {
    pub(super) fn path(&self) -> &Path {
        self.0.path()
    }
}

pub(super) fn read(path: &Path) -> io::Result<Vec<u8>> {
    fs::read(path)
}

pub(super) fn read_prefix(path: &Path, len: usize) -> io::Result<Vec<u8>> {
    // A short read is not an error: the caller asked for at most `len` bytes and
    // a file smaller than that has answered in full.
    let mut bytes = Vec::with_capacity(len);
    let file = fs::File::open(path)?;
    file.take(len as u64).read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub(super) fn exists(path: &Path) -> io::Result<bool> {
    // `Path::exists` is `metadata(..).is_ok()`, which answers `false` for a path
    // it is not allowed to inspect. That has been the meaning here all along and
    // it stays: this backend can always answer, so it reports what it saw.
    Ok(path.exists())
}

pub(super) fn is_file(path: &Path) -> io::Result<bool> {
    // Likewise `Path::is_file`: a path that cannot be inspected is not a file.
    Ok(path.is_file())
}

pub(super) fn list_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        // One unreadable entry does not fail the listing, as the shard walk it
        // replaces never failed on one.
        let Ok(entry) = entry else { continue };
        if entry.file_type().is_ok_and(|kind| kind.is_file()) {
            files.push(entry.path());
        }
    }
    Ok(files)
}

pub(super) fn walk_files(root: &Path, strict: bool) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root) {
        match entry {
            Ok(entry) => {
                if entry.file_type().is_file() {
                    files.push(entry.into_path());
                }
            }
            // Depth 0 is the root itself: there is nothing to walk, so this is
            // the one failure the caller is told about.
            Err(error) if strict || error.depth() == 0 => return Err(error.into()),
            Err(_) => continue,
        }
    }
    Ok(files)
}

pub(super) fn create_dir_all(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)
}

pub(super) fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    fs::write(path, bytes)
}

pub(super) fn create_new(path: &Path) -> io::Result<()> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(|_file| ())
}

pub(super) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    // A unique name per write: two concurrent runs cannot rename each other's
    // file away, and a stale `.partial` from a killed run is never reused.
    let staged = staged_sibling(path);

    let write = (|| -> io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)?;
        file.write_all(bytes)?;
        // `sync_all` means a rename only ever exposes durable bytes.
        file.sync_all()
    })();
    if let Err(error) = write {
        let _ = fs::remove_file(&staged);
        return Err(error);
    }

    // `rename` replaces an existing file on Windows and POSIX; never remove the
    // old file first, or an interrupted run loses both copies.
    if let Err(error) = fs::rename(&staged, path) {
        let _ = fs::remove_file(&staged);
        return Err(error);
    }
    Ok(())
}

/// `.<stem>.<pid>-<counter>.partial` beside the destination.
fn staged_sibling(destination: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let stem = destination
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    let unique = format!(
        "{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    destination.with_file_name(format!(".{stem}.{unique}.partial"))
}

pub(super) fn remove_file(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

pub(super) fn copy(from: &Path, to: &Path) -> io::Result<()> {
    fs::copy(from, to).map(|_bytes| ())
}

pub(super) fn scratch_dir(prefix: &str) -> io::Result<ScratchDir> {
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .map(ScratchDir)
}

pub(super) fn sha256_file(path: &Path) -> io::Result<String> {
    // Chunked, so a catalog of a few hundred megabytes costs no more memory than
    // an empty one.
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub(super) fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    fs::canonicalize(path)
}

pub(super) fn current_dir() -> io::Result<PathBuf> {
    std::env::current_dir()
}

// `filetime` deprecated this call in 0.2.30 in favour of `std::fs::set_times`,
// which this crate cannot use: `FileTimes` is stable since Rust 1.99 and the
// workspace publishes `rust-version = "1.89"` (`scripts/check.sh msrv` refuses
// it). The attribute goes when the floor does; pinning the crate back to keep
// the floor would be the trade AGENTS.md rules out.
#[allow(deprecated)]
pub(super) fn set_mtime(path: &Path, unix_seconds: i64) -> io::Result<()> {
    filetime::set_file_mtime(path, filetime::FileTime::from_unix_time(unix_seconds, 0))
}

pub(super) fn available_space(path: &Path) -> io::Result<u64> {
    // A test's answer, before the platform's: `cargo test` builds with
    // `debug_assertions` and no portable unprivileged machine can be made
    // nearly full on demand, so this is how a real binary performs a real
    // refusal (`specs/modules/fs.md`).
    #[cfg(debug_assertions)]
    {
        if let Some(bytes) = test_free_space() {
            return Ok(bytes);
        }
    }

    #[cfg(any(unix, windows))]
    {
        // `f_bavail` and `lpFreeBytesAvailableToCaller`: what a non-privileged
        // writer may still use, rather than the blocks a superuser could.
        fs4::available_space(path)
    }

    // An operating system `fs4` has no answer for. `wasm32` never reaches this
    // file; it has its own backend and its own refusal.
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this target cannot report a filesystem's free space",
        ))
    }
}

/// The free space a debug build answers instead of asking the operating system.
///
/// `cargo test` compiles with `debug_assertions`, so a test can hand the real
/// binary a destination with too little room; a release build does not read the
/// name and always asks the platform. One name, one meaning, and it is recorded
/// where the seam's decisions live (`specs/modules/fs.md`).
#[cfg(debug_assertions)]
fn test_free_space() -> Option<u64> {
    std::env::var("LRSALVAGE_FREE_SPACE_BYTES")
        .ok()?
        .trim()
        .parse()
        .ok()
}
