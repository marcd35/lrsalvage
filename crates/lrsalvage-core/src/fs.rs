//! The crate's one filesystem seam.
//!
//! Every module asks this one for bytes, listings and writes. No other module's
//! shipped code in `lrsalvage-core` names `std::fs`, `tempfile` or `filetime`,
//! and `tests/fs_seam.rs` fails when that stops being true: it reads each source
//! file up to its own `#[cfg(test)]` module, so a test may still look at the disk
//! directly. Two exceptions are recorded rather than hidden. `masks` keeps its
//! own `walkdir` walk, because it is native-only and outside the seam
//! (`crate::masks`); and `lrsalvage-cli` is a separate crate that names
//! `std::fs` for its own files, which is not this seam's to own.
//!
//! # The operations are the call sites, not POSIX
//!
//! Each item below exists because a module needed it, and the list is
//! deliberately short of a general `open`/`read`/`write`/`seek` handle: a
//! handle is a shape a browser cannot offer, so it would be a seam whose second
//! implementation is impossible. `write_atomic` is the clearest case — staging,
//! `fsync` and `rename` are one operation here because they are one promise
//! (`adr-0003-preserve-preview-bytes`), and a caller that could half-perform it
//! would eventually do so.
//!
//! # One signature, two targets
//!
//! The backend is chosen by `cfg` at compile time: `native` for every target
//! that has an operating system, `wasm` for `wasm32-unknown-unknown`. There is
//! no trait object and no generic parameter, so a payload read costs a call
//! rather than a vtable lookup, and the callers are identical on both targets.
//! The cost is that the two
//! implementations are kept in step by the compiler rather than by a test: a
//! function added to one and forgotten in the other does not build.
//!
//! # Native first, and unchanged
//!
//! The native backend is the code those modules already had, moved: same
//! `std::fs` calls, same `tempfile` scratch directory, same 64 KiB hashing
//! buffer, same `.partial` staging name, same error kinds. Nothing about a run
//! of the CLI changed when this module appeared.
//!
//! # The second target answers, from a mount table
//!
//! `wasm32-unknown-unknown` has no filesystem, and #142's stub said so with one
//! [`io::ErrorKind::Unsupported`] for all seventeen operations it then had. That
//! was the
//! honest answer while nothing could answer better; it was never the browser's
//! implementation, and `previews_db::open` reporting a present database as
//! `Missing` was the measurement that produced the seam. The browser backend
//! replaces the stub: paths resolve against a registered table of roots (an
//! origin-private store SQLite can open, a picked directory, a picked file), the
//! store is answered in this process, and everything else goes through one
//! blocking call the host installs. `crate::fs::wasm` is the routing and
//! `crate::fs::bridge` the protocol; both carry the registration contract in
//! their module docs, and `specs/modules/fs.md` records it as the seam's browser
//! half.
//!
//! Two operations stay refused there. `set_mtime`, because the File System API
//! cannot set a modification time; and `available_space`, because a page cannot
//! know the destination's free space — `navigator.storage.estimate()` describes
//! the origin's quota, not the disk the folder is on (`#117`). `extract` already
//! treats that refusal as an unmeasured destination and proceeds. `current_dir`
//! answers `/` instead of refusing: a page has no working directory, so a
//! relative path resolves to no mount and is `NotFound` rather than being
//! anchored somewhere a user cannot see.
//!
//! A caller that only needs a `bool` and has no use for a reason still writes
//! `unwrap_or(false)`, and on the browser those are the sites that read a refusal
//! as absence. The seam keeps [`exists`] fallible for exactly that reason, and
//! `docs/browser-spike.md` §1 is the measurement.

use std::io;
use std::path::{Path, PathBuf};

#[cfg(not(target_arch = "wasm32"))]
mod native;
// The browser half is the crate's browser-facing surface: a page has to install
// it before a run, which is why this module is public on that target while the
// native backend, whose `std` does the installing, stays private.
#[cfg(target_arch = "wasm32")]
pub mod wasm;

// The browser half's two pure pieces: where a path resolves, and what the host
// is asked. Both are compiled for the host as well, because both are arithmetic
// rather than browser, and their unit tests are the only ones that can run
// without a page. On the host nothing calls them, which is what the `dead_code`
// allowance says rather than leaving a reviewer to wonder.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) mod bridge;
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) mod mounts;

#[cfg(not(target_arch = "wasm32"))]
use native as backend;
#[cfg(target_arch = "wasm32")]
use wasm as backend;

/// A private directory that is removed when this value is dropped.
///
/// The snapshot's temp copy lives as long as this handle, which is what makes
/// "the copy goes away with the snapshot" true. The removal is best effort and
/// its error is ignored, exactly as `tempfile`'s own guard did: a scratch
/// directory left behind is a nuisance, never a lost photograph.
pub struct ScratchDir(backend::ScratchDir);

impl ScratchDir {
    /// The directory's own path.
    pub fn path(&self) -> &Path {
        self.0.path()
    }
}

/// Read a whole file.
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    backend::read(path)
}

/// The first `len` bytes of a file, or every byte when it is shorter.
pub fn read_prefix(path: &Path, len: usize) -> io::Result<Vec<u8>> {
    backend::read_prefix(path, len)
}

/// Whether anything at all is at `path`.
///
/// Natively this answers `false` where the path cannot be inspected, which is
/// what `Path::exists` has always meant here. It is fallible anyway, because
/// the one question this seam exists to stop answering by accident is this one:
/// a target that cannot look must say so, not report absence.
pub fn exists(path: &Path) -> io::Result<bool> {
    backend::exists(path)
}

/// Whether a regular file is at `path`; `false` where it cannot be inspected.
pub fn is_file(path: &Path) -> io::Result<bool> {
    backend::is_file(path)
}

/// The regular files directly inside `dir`, in the order the platform lists them.
///
/// Directories and anything else that is not a regular file are left out. An
/// entry that cannot be inspected is skipped; a `dir` that cannot be read is the
/// `Err`.
pub fn list_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    backend::list_files(dir)
}

/// Every regular file under `root`, recursively.
///
/// The cache-tree walk. An entry the walk cannot read is skipped rather than
/// failing the whole sweep, which is what `previews_db` has always done: one
/// unreadable folder must not hide the payloads next to it. A `root` that cannot
/// be walked at all is the `Err`.
pub fn walk_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    backend::walk_files(root, false)
}

/// Return a traversal failure at any depth, so an incomplete inventory cannot
/// be mistaken for an absence of payload files.
pub(crate) fn walk_files_strict(root: &Path) -> io::Result<Vec<PathBuf>> {
    backend::walk_files(root, true)
}

/// Create `dir` and every missing parent; an existing directory is not an error.
pub fn create_dir_all(dir: &Path) -> io::Result<()> {
    backend::create_dir_all(dir)
}

/// Write `bytes` to `path`, replacing whatever was there.
///
/// Whole-file and not atomic: `rename`-then-replace is [`write_atomic`], and a
/// caller that wants a reader never to see a mixture asks for that instead.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    backend::write(path, bytes)
}

/// Create an empty file at `path`, failing when anything is already there.
///
/// The destination-probe's operation: it proves a directory is writable without
/// touching anything a previous run left, and it never truncates.
pub fn create_new(path: &Path) -> io::Result<()> {
    backend::create_new(path)
}

/// Write `bytes` so that `path` holds either what it held before or all of them.
///
/// The bytes go to a unique sibling, are flushed to the device, and only then
/// replace `path` by `rename`. A failure leaves `path` untouched and the staged
/// sibling removed.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    backend::write_atomic(path, bytes)
}

/// Remove the file at `path`.
pub fn remove_file(path: &Path) -> io::Result<()> {
    backend::remove_file(path)
}

/// Copy `from` to `to`, replacing `to` and carrying `from`'s permissions.
pub fn copy(from: &Path, to: &Path) -> io::Result<()> {
    backend::copy(from, to)
}

/// A new private directory under the platform's temporary directory, whose name
/// begins with `prefix` and which is removed when the handle is dropped.
pub fn scratch_dir(prefix: &str) -> io::Result<ScratchDir> {
    backend::scratch_dir(prefix).map(ScratchDir)
}

/// The SHA-256 of a file's bytes as lowercase hex, read in chunks.
///
/// This is the number `--verify-source` publishes and `scan --json` reports as
/// `source_sha256`/`snapshot_sha256`, so it has one definition and it lives
/// here: a browser has a stream and not a file, and the digest of the stream is
/// still this hex.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    backend::sha256_file(path)
}

/// The absolute, symlink-free spelling of `path`.
///
/// Used to compare a destination with the evidence trees it must not land in,
/// so an answer that is merely lexical is not good enough.
pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    backend::canonicalize(path)
}

/// The process's working directory, which a relative destination is anchored at.
pub fn current_dir() -> io::Result<PathBuf> {
    backend::current_dir()
}

/// Set a file's modification time to `unix_seconds` since the Unix epoch.
///
/// `--set-timestamps` writes the capture time this way; the timestamp library
/// stays behind this seam so a target without file times has one place to say so.
pub fn set_mtime(path: &Path, unix_seconds: i64) -> io::Result<()> {
    backend::set_mtime(path, unix_seconds)
}

/// How many bytes the filesystem holding `path` can still take, for a
/// non-privileged writer.
///
/// It answers for the filesystem `path` is on, not for the directory itself, and
/// the two targets differ on a path that is not there: Windows answers for the
/// volume the path would be on, unix refuses one (`statvfs` needs a path that
/// exists). A caller that must have an answer on every target asks about an
/// existing ancestor, which is what `extract` does — it refuses before its first
/// write, so it may not create the destination to ask.
///
/// On `wasm32` this refuses, which is the honest answer: a page cannot know the
/// destination's free space (`#117`), and `navigator.storage.estimate()`
/// describes the origin's quota rather than the disk a picked folder is on.
///
/// A debug build answers `LRSALVAGE_FREE_SPACE_BYTES` instead of asking the
/// platform — on the native backend only, because the `wasm32` one refuses this
/// operation and never reads the name. That is the seam's one test aid.
pub fn available_space(path: &Path) -> io::Result<u64> {
    backend::available_space(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory for one test, removed by the guard.
    fn scratch() -> ScratchDir {
        scratch_dir("lrsalvage-fs-test-").expect("a scratch directory")
    }

    #[test]
    fn read_returns_every_byte() {
        let dir = scratch();
        let path = dir.path().join("whole.bin");
        write(&path, b"the whole file").expect("write");
        assert_eq!(read(&path).expect("read"), b"the whole file");
        assert!(
            read(&dir.path().join("absent")).is_err(),
            "an absent file is an error, not an empty one"
        );
    }

    #[test]
    fn read_prefix_stops_at_the_length_and_tolerates_short_files() {
        let dir = scratch();
        let path = dir.path().join("head.bin");
        write(&path, b"\xFF\xD8rest").expect("write");
        assert_eq!(read_prefix(&path, 2).expect("prefix"), b"\xFF\xD8");
        assert_eq!(
            read_prefix(&path, 4096).expect("short prefix"),
            b"\xFF\xD8rest",
            "a file shorter than the request is all of itself"
        );
        write(&path, b"\xFF").expect("write");
        assert_eq!(read_prefix(&path, 2).expect("one byte"), b"\xFF");
    }

    #[test]
    fn exists_and_is_file_separate_a_file_from_a_directory_and_from_nothing() {
        let dir = scratch();
        let file = dir.path().join("present.bin");
        write(&file, b"x").expect("write");

        assert!(exists(&file).expect("exists"));
        assert!(is_file(&file).expect("is_file"));
        assert!(exists(dir.path()).expect("a directory exists"));
        assert!(
            !is_file(dir.path()).expect("a directory is not a file"),
            "a directory is not a regular file"
        );
        assert!(!exists(&dir.path().join("absent")).expect("exists"));
        assert!(!is_file(&dir.path().join("absent")).expect("is_file"));
    }

    #[test]
    fn list_files_answers_one_directory_and_no_descendents() {
        let dir = scratch();
        let root = dir.path().join("listing");
        create_dir_all(&root.join("nested")).expect("nested directory");
        write(&root.join("a.bin"), b"a").expect("write");
        write(&root.join("b.bin"), b"b").expect("write");
        write(&root.join("nested").join("c.bin"), b"c").expect("write");

        let mut found: Vec<String> = list_files(&root)
            .expect("list")
            .iter()
            .filter_map(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .collect();
        found.sort();
        assert_eq!(found, vec!["a.bin", "b.bin"], "one level, files only");
        assert!(
            list_files(&dir.path().join("absent")).is_err(),
            "a directory that cannot be read is the error"
        );
    }

    #[test]
    fn walk_files_answers_every_descendent_and_skips_nothing_it_can_read() {
        let dir = scratch();
        let root = dir.path().join("tree");
        create_dir_all(&root.join("one").join("two")).expect("nested");
        write(&root.join("top.bin"), b"t").expect("write");
        write(&root.join("one").join("mid.bin"), b"m").expect("write");
        write(&root.join("one").join("two").join("deep.bin"), b"d").expect("write");

        let mut found: Vec<String> = walk_files(&root)
            .expect("walk")
            .iter()
            .filter_map(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .collect();
        found.sort();
        assert_eq!(
            found,
            vec!["deep.bin", "mid.bin", "top.bin"],
            "every level, and no directory among the answers"
        );
        assert!(
            walk_files(&dir.path().join("absent")).is_err(),
            "a root that cannot be walked is the error"
        );
    }

    #[test]
    fn create_dir_all_is_idempotent_and_makes_parents() {
        let dir = scratch();
        let nested = dir.path().join("a").join("b").join("c");
        create_dir_all(&nested).expect("create");
        assert!(exists(&nested).expect("exists"));
        create_dir_all(&nested).expect("an existing directory is not an error");
    }

    #[test]
    fn write_replaces_what_was_there() {
        let dir = scratch();
        let path = dir.path().join("replaced.bin");
        write(&path, b"the first contents").expect("first write");
        write(&path, b"the second").expect("second write");
        assert_eq!(read(&path).expect("read"), b"the second");
    }

    #[test]
    fn create_new_refuses_a_path_that_exists_and_never_truncates_it() {
        let dir = scratch();
        let path = dir.path().join("claimed.bin");
        create_new(&path).expect("a fresh path");
        assert_eq!(
            read(&path).expect("read"),
            b"",
            "an empty file, not a missing one"
        );
        write(&path, b"somebody's bytes").expect("write");
        let error = create_new(&path).expect_err("a taken path is the error");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            read(&path).expect("read"),
            b"somebody's bytes",
            "the refusal must leave the existing file alone"
        );
    }

    #[test]
    fn write_atomic_leaves_the_new_bytes_and_no_staged_sibling() {
        let dir = scratch();
        let path = dir.path().join("destination.bin");
        write(&path, b"the old contents").expect("a pre-existing file");

        write_atomic(&path, b"the new contents").expect("atomic write");
        assert_eq!(read(&path).expect("read"), b"the new contents");
        assert_eq!(
            list_files(dir.path()).expect("list"),
            vec![path.clone()],
            "the staged sibling is renamed away, never left behind"
        );
    }

    #[test]
    fn write_atomic_creates_a_destination_that_is_not_there() {
        let dir = scratch();
        let path = dir.path().join("fresh.bin");
        write_atomic(&path, b"bytes").expect("atomic write");
        assert_eq!(read(&path).expect("read"), b"bytes");
    }

    #[test]
    fn remove_file_takes_the_file_and_errors_on_a_second_call() {
        let dir = scratch();
        let path = dir.path().join("removed.bin");
        write(&path, b"x").expect("write");
        remove_file(&path).expect("remove");
        assert!(!exists(&path).expect("exists"));
        assert!(
            remove_file(&path).is_err(),
            "removing what is not there is an error, as it always was"
        );
    }

    #[test]
    fn copy_carries_the_bytes() {
        let dir = scratch();
        let from = dir.path().join("from.bin");
        let to = dir.path().join("to.bin");
        write(&from, b"the catalog as it was").expect("write");
        copy(&from, &to).expect("copy");
        assert_eq!(read(&to).expect("read"), b"the catalog as it was");
        assert_eq!(
            read(&from).expect("read"),
            b"the catalog as it was",
            "a copy never consumes its source"
        );
    }

    #[test]
    fn a_scratch_dir_is_private_and_goes_away_with_its_handle() {
        let path = {
            let inner = scratch_dir("lrsalvage-fs-test-inner-").expect("scratch");
            let path = inner.path().to_path_buf();
            assert!(path.is_absolute(), "the directory is a real, absolute path");
            assert!(exists(&path).expect("exists"));
            path
        };
        assert!(
            !exists(&path).expect("exists"),
            "dropping the handle removes the directory"
        );
    }

    #[test]
    fn sha256_file_is_the_digest_of_the_bytes() {
        let dir = scratch();
        let path = dir.path().join("hashed.bin");
        write(&path, b"abc").expect("write");
        assert_eq!(
            sha256_file(&path).expect("digest"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "the SHA-256 of `abc`, lowercase hex"
        );
        write(&path, b"").expect("write");
        assert_eq!(
            sha256_file(&path).expect("digest"),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "the digest of an empty file, which is not an error"
        );
    }

    #[test]
    fn canonicalize_resolves_a_relative_spelling() {
        let dir = scratch();
        let nested = dir.path().join("a").join("b");
        create_dir_all(&nested).expect("create");
        write(&nested.join("file.bin"), b"x").expect("write");

        let direct = canonicalize(&nested).expect("canonical");
        let roundabout = canonicalize(&nested.join(".").join("..").join("b")).expect("canonical");
        assert_eq!(direct, roundabout, "one directory, one spelling");
        assert!(direct.is_absolute(), "the answer is absolute");
        assert!(
            canonicalize(&dir.path().join("absent")).is_err(),
            "what is not there cannot be resolved"
        );
    }

    #[test]
    fn current_dir_is_absolute_and_a_real_directory() {
        let cwd = current_dir().expect("current_dir");
        assert!(cwd.is_absolute(), "a working directory is absolute");
        assert!(exists(&cwd).expect("exists"));
    }

    #[test]
    fn set_mtime_sets_the_time_it_was_given() {
        let dir = scratch();
        let path = dir.path().join("stamped.bin");
        write(&path, b"x").expect("write");

        // 2009-08-06T00:00:00Z, the capture time `format-notes` uses in examples.
        set_mtime(&path, 1_248_825_600).expect("set the time");
        let modified = std::fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .expect("the file's time")
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a time after the epoch");
        assert_eq!(modified.as_secs(), 1_248_825_600);
        assert!(
            set_mtime(&dir.path().join("absent"), 0).is_err(),
            "a file that is not there cannot be stamped"
        );
    }

    #[test]
    fn available_space_answers_a_number_for_a_real_directory() {
        let dir = scratch();
        // The number is this machine's, so the claim here is about the answer's
        // shape rather than a size (`CODING_STANDARDS.md`, *Claims*). What a
        // path that is not there answers differs by platform — unix refuses one,
        // Windows answers for the volume — which is why a caller asks about an
        // ancestor that exists.
        let free = available_space(dir.path()).expect("a real directory answers");
        assert!(
            free > 0,
            "a filesystem this test writes to has room, got {free}"
        );
    }
}
