//! The `wasm32-unknown-unknown` backend: the browser's storage, behind the seam.
//!
//! A page has no filesystem, so this backend is a **mount table** over the three
//! places a run's paths actually resolve to, plus the one thing SQLite needs and
//! a page cannot give it. The seam's signatures take `&Path` and nothing else —
//! that was #142's whole point — so the target has to be told, once, where a path
//! resolves:
//!
//! - **the store** — an origin-private pool that SQLite can open. The catalog copy
//!   and its `-wal` live here, because SQLite cannot open anything else on this
//!   target: the pool (`sqlite-wasm-vfs`'s `sahpool`) is the only persistent VFS
//!   in the measured route (`docs/browser-spike.md` §1–§2). It is also where
//!   [`scratch_dir`] puts the snapshot's private copy. It lives in this process,
//!   so it needs no bridge and is read, written and enumerated synchronously.
//! - **a bridged directory** — a `FileSystemDirectoryHandle`, from
//!   `showDirectoryPicker` or from the origin-private file system. The user's
//!   cache tree is read from one and the recovered JPEGs are written to another.
//!   Every operation on one is a promise and the seam is synchronous, so this half
//!   goes through the host's blocking bridge.
//! - **a bridged file** — a single picker's `File`, read-only.
//!
//! Where a path resolves is [`mounts`]'s question; how a bridged operation is
//! asked is [`bridge`]'s. Both are compiled for the host too, because both are
//! arithmetic rather than browser, and their tests are the ones that run in
//! `cargo test -p lrsalvage-core`.
//!
//! # The registration point
//!
//! The host installs this backend once, before the first operation, and this is
//! the contract the next slice reads:
//!
//! 1. [`bridge::install`] takes the host's one blocking call. Until it is called,
//!    every bridged operation is `Unsupported` — the honest answer for a page
//!    that wired nothing, and the difference between that and "absent".
//! 2. [`install_store`] is awaited once — or, from a host with no async runtime,
//!    [`start_install`] followed by [`poll_install`] from a macrotask until it
//!    settles. It installs (or reuses) the origin-private pool and makes it
//!    SQLite's default VFS. It is in this process, not behind the bridge, so it
//!    needs a **dedicated worker** — where the pool is specified to live, and
//!    therefore where this code must run.
//! 3. [`mount`] is called once per root, then [`seal`]. From there a path no mount
//!    answers for is `NotFound` rather than a guess.
//!
//! A mount is a *path prefix*, longest first, and the reason for a table rather
//! than a per-operation argument is that the callers build their paths from these
//! roots: adding a browser-only parameter to eighteen signatures would be the
//! caller change #142's choice exists to avoid.
//!
//! # What stays refused, and why
//!
//! [`set_mtime`] is `Unsupported`: the File System API cannot set a file's
//! modification time, and `--set-timestamps` is a CLI flag the app does not
//! offer. [`available_space`] is `Unsupported` too, and for a measured reason:
//! `navigator.storage.estimate()` describes the origin's quota rather than the
//! disk a picked folder is on, so a page cannot know the destination's free
//! space (`#117`). [`current_dir`] answers `/` instead: a page has no working
//! directory, `/` is this namespace's root, and a relative path therefore
//! resolves to no mount and is `NotFound` rather than being anchored somewhere a
//! user cannot see.

use std::cell::RefCell;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};

use rsqlite_vfs::VfsFilesManager;
use rsqlite_vfs::transfer::{DbTransfer, ExportSource};
use sha2::{Digest, Sha256};
use sqlite_wasm_vfs::sahpool::{self, OpfsSAHPoolCfgBuilder, OpfsSAHPoolUtil};

use super::bridge::{self, CHUNK};
use super::mounts::{self, Kind, Mount};

/// The least a store is worth installing with: a database, its journal, its
/// `-wal`, one scratch copy, and the temp file a second connection opens.
const MIN_CAPACITY: usize = 8;

const NO_STORE: &str = "no origin-private store is installed on this target";
const NO_SCRATCH: &str = "no store mount is registered, so a scratch directory has nowhere to live";
const NO_MTIME: &str = "the File System API cannot set a file's modification time";
const NO_ROOM: &str = "a page cannot know the destination's free space";

/// The install, while a host is driving it.
type Install = Pin<Box<dyn Future<Output = Result<(), String>>>>;

thread_local! {
    static MOUNTS: RefCell<Vec<Mount>> = const { RefCell::new(Vec::new()) };
    static STORE: RefCell<Option<OpfsSAHPoolUtil>> = const { RefCell::new(None) };
    static INSTALL: RefCell<Option<Install>> = const { RefCell::new(None) };
}

// ---------------------------------------------------------------------------
// The registration point
// ---------------------------------------------------------------------------

/// The host's one blocking call, as the bridge's protocol defines it.
pub type BridgeCall = super::bridge::BridgeCall;

/// Take the host's blocking call. Called once, before the first operation.
///
/// Until it is called, every bridged operation is `Unsupported` — the honest
/// answer for a page that wired nothing, and not the same thing as "absent".
pub fn install_bridge(call: BridgeCall) {
    super::bridge::install(call)
}

/// Install (or reuse) the origin-private store and make it SQLite's default VFS.
///
/// `clear` discards whatever a previous session left; a run that resumes asks for
/// `false`. The capacity is a count of files the pool may hold at once, not a
/// size: a slot grows with what is written to it.
///
/// A host that cannot `.await` — one with no async runtime, which is what
/// `deny.toml` keeps this tree to — uses [`start_install`] and [`poll_install`]
/// instead. They drive this same future; nothing else about it changes.
pub async fn install_store(capacity: usize, clear: bool) -> Result<(), String> {
    let capacity = capacity.max(MIN_CAPACITY);
    let config = OpfsSAHPoolCfgBuilder::new()
        .initial_capacity(capacity)
        .clear_on_init(clear)
        .build();
    let pool = sahpool::install::<sqlite_wasm_rs::WasmOsCallback>(&config, true)
        .await
        .map_err(|error| error.to_string())?;
    // A reused pool keeps the capacity it was first given, so ask for this run's.
    pool.ensure_capacity(capacity)
        .await
        .map_err(|error| error.to_string())?;
    STORE.with(|slot| *slot.borrow_mut() = Some(pool));
    Ok(())
}

/// Begin installing the store, for a host that drives it from a task loop.
///
/// The install is a promise (acquiring the pool's slots is), and a synchronous
/// host cannot await it. Rather than take an async runtime for one promise, the
/// host starts the install here and calls [`poll_install`] from a macrotask until
/// it reports the store is there — which is exactly where a settled browser
/// promise's callback runs anyway.
pub fn start_install(capacity: usize, clear: bool) {
    let future = install_store(capacity, clear);
    INSTALL.with(|slot| *slot.borrow_mut() = Some(Box::pin(future)));
}

/// Advance the install: `Ok(false)` while it is still waiting, `Ok(true)` once
/// the store is installed, and the error if it failed.
pub fn poll_install() -> Result<bool, String> {
    let mut future = INSTALL
        .with(|slot| slot.borrow_mut().take())
        .ok_or_else(|| "no store install has been started".to_string())?;
    let mut context = Context::from_waker(std::task::Waker::noop());
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result.map(|()| true),
        Poll::Pending => {
            INSTALL.with(|slot| *slot.borrow_mut() = Some(future));
            Ok(false)
        }
    }
}

/// Release the store's sync access handles after closing its SQLite connections.
pub fn pause_store() -> io::Result<()> {
    store()?.pause().map_err(store_error)
}

/// Register one path prefix, and which storage answers under it.
///
/// `store` marks the pool; everything else is bridged. A nested mount wins over
/// the mount that contains it, so a picked folder and one read-only file inside
/// it can both be mounted.
pub fn mount(root: &Path, store: bool) {
    MOUNTS.with(|mounts| {
        let mut mounts = mounts.borrow_mut();
        mounts.retain(|mount| mount.root != root);
        mounts.push(Mount::new(root, store));
        mounts::sort_deepest_first(&mut mounts);
    });
}

/// End registration. A path no mount answers for is `NotFound` from here on.
///
/// There is nothing to do — that is already the answer — but the call is the
/// contract, and a session that keeps the habit cannot be the one that silently
/// keeps a stale mount from the run before.
pub fn seal() {}

/// Forget every mount and the store, for a new run in the same worker.
pub fn reset() {
    MOUNTS.with(|mounts| mounts.borrow_mut().clear());
    STORE.with(|slot| *slot.borrow_mut() = None);
    INSTALL.with(|slot| *slot.borrow_mut() = None);
}

/// Whether a store is installed, for a host that wants to report its state.
pub fn store_installed() -> bool {
    STORE.with(|slot| slot.borrow().is_some())
}

// ---------------------------------------------------------------------------
// Errors and plumbing
// ---------------------------------------------------------------------------

fn unsupported(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message)
}

/// The pool's errors, in the vocabulary the seam promised: `std::io::Error`.
fn store_error(error: sahpool::OpfsSAHError) -> io::Error {
    use sahpool::OpfsSAHError;
    let kind = match &error {
        OpfsSAHError::FileNotFound(_) => io::ErrorKind::NotFound,
        OpfsSAHError::FileExists(_) => io::ErrorKind::AlreadyExists,
        OpfsSAHError::FileInUse(_)
        | OpfsSAHError::FilesInUse
        | OpfsSAHError::Busy
        | OpfsSAHError::Paused => io::ErrorKind::WouldBlock,
        OpfsSAHError::NotSupported => io::ErrorKind::Unsupported,
        OpfsSAHError::InvalidFilename(_)
        | OpfsSAHError::InvalidDirectory
        | OpfsSAHError::InvalidDirectoryEntry => io::ErrorKind::InvalidInput,
        OpfsSAHError::NoCapacity | OpfsSAHError::OutOfMemory => io::ErrorKind::StorageFull,
        // "A pending WAL describes this image, or this namespace needs
        // reconciling": no `ErrorKind` says that, so it is `Other` with the
        // pool's own message and `specs/modules/fs.md` records it.
        OpfsSAHError::RecoveryRequired(_) | OpfsSAHError::NeedsRecovery => io::ErrorKind::Other,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, error.to_string())
}

fn mount_of(path: &Path) -> io::Result<Mount> {
    MOUNTS.with(|mounts| {
        mounts::find(&mounts.borrow(), path)
            .cloned()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("no mount answers for this path: {}", path.display()),
                )
            })
    })
}

fn store() -> io::Result<OpfsSAHPoolUtil> {
    STORE.with(|slot| slot.borrow().clone().ok_or_else(|| unsupported(NO_STORE)))
}

fn store_root() -> io::Result<PathBuf> {
    MOUNTS
        .with(|mounts| {
            mounts
                .borrow()
                .iter()
                .find(|mount| mount.kind == Kind::Store)
                .map(|mount| mount.root.clone())
        })
        .ok_or_else(|| unsupported(NO_SCRATCH))
}

/// A virtual path as the pool's name.
///
/// The pool's namespace is flat and its names are UTF-8 keys under 499 bytes with
/// no NUL, so a path is used verbatim: that is what makes SQLite open the same
/// file the seam's caller built the path for.
fn store_name(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

// ---------------------------------------------------------------------------
// One reader shape for both kinds
// ---------------------------------------------------------------------------

fn size_of(path: &Path) -> io::Result<u64> {
    match mount_of(path)?.kind {
        Kind::Store => {
            let pool = store()?;
            let export = pool.open_export(&store_name(path)).map_err(store_error)?;
            Ok(export.size())
        }
        Kind::Bridge => {
            let stat = bridge::stat(path)?;
            if !stat.exists || !stat.is_file {
                return Err(io::Error::new(io::ErrorKind::NotFound, store_name(path)));
            }
            Ok(stat.len)
        }
    }
}

fn read_chunk(path: &Path, offset: u64, len: usize) -> io::Result<Vec<u8>> {
    match mount_of(path)?.kind {
        Kind::Store => {
            let pool = store()?;
            let mut export = pool.open_export(&store_name(path)).map_err(store_error)?;
            let mut bytes = vec![0u8; len];
            let read = export.read_at(offset, &mut bytes).map_err(store_error)?;
            bytes.truncate(read);
            Ok(bytes)
        }
        Kind::Bridge => bridge::read(path, offset, len),
    }
}

/// Walk the first `total` bytes of `path` in chunks, handing each to `each`.
///
/// The one read loop: `read`, `read_prefix`, `sha256_file` and `copy` want the
/// same walk with a different sink. A short answer that is not the end is a host
/// that lost bytes, and returning what arrived would report a truncated file as
/// whole — so it is one error, built here rather than four times.
fn for_each_chunk(
    path: &Path,
    total: u64,
    chunk: usize,
    mut each: impl FnMut(&[u8]) -> io::Result<()>,
) -> io::Result<()> {
    let mut offset = 0u64;
    while offset < total {
        let want = ((total - offset) as usize).min(chunk);
        let piece = read_chunk(path, offset, want)?;
        if piece.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("{} ended before {total} bytes", path.display()),
            ));
        }
        offset += piece.len() as u64;
        each(&piece)?;
    }
    Ok(())
}

/// Every byte of a path, by chunks: the one call that needs the whole file.
fn read_all(path: &Path, chunk: usize) -> io::Result<Vec<u8>> {
    let total = size_of(path)?;
    let mut bytes = Vec::with_capacity(total as usize);
    for_each_chunk(path, total, chunk, |piece| {
        bytes.extend_from_slice(piece);
        Ok(())
    })?;
    Ok(bytes)
}

/// The store's names under `path`, one level deep or recursive.
///
/// Both listing operations are this scan with a different filter, and both keep
/// the same tail: a path nothing is under is a directory that is not there.
fn store_children(path: &Path, recursive: bool) -> io::Result<Vec<PathBuf>> {
    let pool = store()?;
    let name = store_name(path);
    let prefix = mounts::dir_prefix(&name);
    let mut found = Vec::new();
    for candidate in pool.names().map_err(store_error)? {
        let Some(rest) = candidate.strip_prefix(&prefix) else {
            continue;
        };
        let wanted = if recursive {
            !rest.is_empty()
        } else {
            !rest.is_empty() && !rest.contains('/')
        };
        if wanted {
            found.push(path.join(rest));
        }
    }
    if found.is_empty() && !exists(path)? {
        return Err(io::Error::new(io::ErrorKind::NotFound, name));
    }
    Ok(found)
}

// ---------------------------------------------------------------------------
// The seam's operations
// ---------------------------------------------------------------------------

/// A private directory: a prefix in the store, removed with this value.
///
/// The store is in this process, so the removal is synchronous and the handle
/// means what it means natively — dropping it takes the copy with it. A scratch
/// directory is not an *entry* in the store until something is written into it
/// (the pool is flat, and a marker file would show up in the listings the seam's
/// own tests pin), so `exists` on one answers `false` until its first file.
pub(super) struct ScratchDir(PathBuf);

impl ScratchDir {
    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        // Best effort, as the native guard was: a scratch directory left behind
        // is a nuisance, never a lost photograph. Nothing here may panic during
        // an unwind, so every step is a `let _`.
        let Ok(pool) = store() else { return };
        let Ok(names) = pool.names() else { return };
        let exact = store_name(&self.0);
        let prefix = mounts::dir_prefix(&exact);
        for name in names {
            if name == exact || name.starts_with(&prefix) {
                let _ = pool.remove(&name);
            }
        }
    }
}

pub(super) fn read(path: &Path) -> io::Result<Vec<u8>> {
    read_all(path, CHUNK)
}

pub(super) fn read_prefix(path: &Path, len: usize) -> io::Result<Vec<u8>> {
    if len == 0 {
        return Ok(Vec::new());
    }
    // A short read is not an error: the caller asked for at most `len` bytes and
    // a file smaller than that has answered in full.
    let want = (size_of(path)? as usize).min(len);
    let mut bytes = Vec::with_capacity(want);
    for_each_chunk(path, want as u64, CHUNK, |piece| {
        bytes.extend_from_slice(piece);
        Ok(())
    })?;
    Ok(bytes)
}

pub(super) fn exists(path: &Path) -> io::Result<bool> {
    match mount_of(path)?.kind {
        Kind::Store => {
            let pool = store()?;
            let name = store_name(path);
            if pool.contains(&name).map_err(store_error)? {
                return Ok(true);
            }
            // A directory exists in the flat store exactly when something is
            // under it: there is nowhere else for one to be.
            let prefix = mounts::dir_prefix(&name);
            Ok(pool
                .names()
                .map_err(store_error)?
                .iter()
                .any(|candidate| candidate == &name || candidate.starts_with(&prefix)))
        }
        Kind::Bridge => Ok(bridge::stat(path)?.exists),
    }
}

pub(super) fn is_file(path: &Path) -> io::Result<bool> {
    match mount_of(path)?.kind {
        Kind::Store => {
            let pool = store()?;
            pool.contains(&store_name(path)).map_err(store_error)
        }
        Kind::Bridge => {
            let stat = bridge::stat(path)?;
            Ok(stat.exists && stat.is_file)
        }
    }
}

pub(super) fn list_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    match mount_of(dir)?.kind {
        Kind::Store => store_children(dir, false),
        Kind::Bridge => {
            let mut files = Vec::new();
            let mut from = 0usize;
            loop {
                let (page, done) = bridge::list(dir, from)?;
                from += page.len();
                for name in page {
                    files.push(dir.join(name));
                }
                if done {
                    return Ok(files);
                }
            }
        }
    }
}

pub(super) fn walk_files(root: &Path, _strict: bool) -> io::Result<Vec<PathBuf>> {
    match mount_of(root)?.kind {
        Kind::Store => store_children(root, true),
        Kind::Bridge => {
            let mut files = Vec::new();
            let mut from = 0usize;
            loop {
                let (page, done) = bridge::walk(root, from)?;
                from += page.len();
                for name in page {
                    files.push(root.join(name));
                }
                if done {
                    return Ok(files);
                }
            }
        }
    }
}

pub(super) fn create_dir_all(dir: &Path) -> io::Result<()> {
    match mount_of(dir)?.kind {
        // The store is flat: a directory is a prefix, and nothing has to be made
        // for one to exist.
        Kind::Store => Ok(()),
        Kind::Bridge => bridge::mkdir(dir),
    }
}

pub(super) fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    match mount_of(path)?.kind {
        Kind::Store => store_write(path, bytes),
        Kind::Bridge => bridge::write(path, bytes),
    }
}

pub(super) fn create_new(path: &Path) -> io::Result<()> {
    match mount_of(path)?.kind {
        Kind::Store => {
            let pool = store()?;
            if pool.contains(&store_name(path)).map_err(store_error)? {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    store_name(path),
                ));
            }
            store_write(path, b"")
        }
        Kind::Bridge => bridge::create_new(path),
    }
}

pub(super) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    match mount_of(path)?.kind {
        // One `createWritable` … `close` is the platform's atomic commit, which
        // is why the bridge gets the whole body in one call rather than a staged
        // sibling: an aborted stream leaves the previous contents byte-identical
        // (`docs/browser-spike.md` §3).
        Kind::Bridge => bridge::write_atomic(path, bytes),
        // A store destination has one writer in one process, and a publish is one
        // commit, so the same call answers both promises.
        Kind::Store => store_write(path, bytes),
    }
}

pub(super) fn remove_file(path: &Path) -> io::Result<()> {
    match mount_of(path)?.kind {
        Kind::Store => {
            let pool = store()?;
            match pool.remove(&store_name(path)).map_err(store_error)? {
                true => Ok(()),
                // Removing what is not there is an error, as it always was.
                false => Err(io::Error::new(io::ErrorKind::NotFound, store_name(path))),
            }
        }
        Kind::Bridge => bridge::remove(path),
    }
}

pub(super) fn copy(from: &Path, to: &Path) -> io::Result<()> {
    let total = size_of(from)?;
    match mount_of(to)?.kind {
        Kind::Store => {
            let pool = store()?;
            let name = store_name(to);
            // `create_import` publishes a new name only, and a target that is
            // already there is what a copy replaces. The pool's own refusal when
            // a `-wal` companion exists is kept: publishing an image that a
            // pending WAL describes is the one thing a copy must not do.
            if pool.contains(&name).map_err(store_error)? {
                pool.remove(&name).map_err(store_error)?;
            }
            let mut import = pool
                .begin_import_unchecked(&name, total)
                .map_err(store_error)?;
            for_each_chunk(from, total, CHUNK, |piece| {
                import.write(piece).map_err(store_error)
            })?;
            import.finish().map_err(store_error)
        }
        // A destination the host owns is the picked folder the JPEGs go to, and
        // the seam's copy is whole-file. The one large copy in a run is the
        // catalog, and a catalog copy always lands in the store above.
        Kind::Bridge => {
            let bytes = read(from)?;
            bridge::write(to, &bytes)
        }
    }
}

pub(super) fn scratch_dir(prefix: &str) -> io::Result<ScratchDir> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let root = store_root()?;
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    Ok(ScratchDir(
        root.join("scratch").join(format!("{prefix}{unique}")),
    ))
}

pub(super) fn sha256_file(path: &Path) -> io::Result<String> {
    // Chunked, as natively: a catalog of a few hundred megabytes costs the chunk
    // and not the file.
    let total = size_of(path)?;
    let mut hasher = Sha256::new();
    for_each_chunk(path, total, 64 * 1024, |piece| {
        hasher.update(piece);
        Ok(())
    })?;
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub(super) fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    match mount_of(path)?.kind {
        Kind::Store => Ok(mounts::normalise(path)),
        Kind::Bridge => bridge::canonicalize(path),
    }
}

pub(super) fn current_dir() -> io::Result<PathBuf> {
    Ok(PathBuf::from("/"))
}

pub(super) fn set_mtime(_path: &Path, _unix_seconds: i64) -> io::Result<()> {
    Err(unsupported(NO_MTIME))
}

// ---------------------------------------------------------------------------
// The store's writes
// ---------------------------------------------------------------------------

/// Publish `bytes` under `path` in the pool, replacing what was there.
///
/// A byte-preserving import (`import_db_unchecked`) rather than a checked one:
/// the checked route rewrites the SQLite header to rollback mode, which is the
/// measured 78-of-128 reading `docs/browser-spike.md` §2 records. A `copy` is a
/// copy; interpretation is the caller's.
fn store_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let pool = store()?;
    let name = store_name(path);
    if pool.contains(&name).map_err(store_error)? {
        pool.remove(&name).map_err(store_error)?;
    }
    pool.import_db_unchecked(&name, bytes).map_err(store_error)
}

/// Refused, not guessed: a page cannot know the destination's free space.
///
/// `navigator.storage.estimate()` describes the *origin's* quota, not the disk a
/// picked folder is on, so any number here would be about something else — the
/// one thing this seam exists to stop. `extract::ensure_room` already treats a
/// refusal as `Unmeasured` and proceeds, so a browser run behaves the way a
/// desktop run on an unmeasurable filesystem does (`#117`, `#64`).
pub(super) fn available_space(_path: &Path) -> io::Result<u64> {
    Err(unsupported(NO_ROOM))
}
