//! A stable, read-only snapshot of a catalog file and its WAL sidecars.

use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use crate::error::{Error, IoContext, Result};

/// A catalog copied to a temporary directory together with `-wal` and `-shm`.
///
/// Lightroom keeps catalogs in WAL mode. Opening the live file read-only can
/// observe a half-applied transaction, and `immutable=1` would ignore the WAL
/// entirely and silently return stale data. Copying the trio is the only
/// option that is both safe and correct.
///
/// The copy is then checkpointed once (it is ours, so writing to it is safe)
/// and the WAL removed. That matters because SQLite treats `immutable=1` as
/// "this file never changes" and does *not* read a neighbouring `-wal`: this
/// was measured, not assumed. After the checkpoint the copy really is a single
/// unchanging file, so [`Catalog::from_snapshot`](crate::catalog::Catalog::from_snapshot)
/// can open it immutably without going stale.
pub struct CatalogSnapshot {
    _dir: TempDir,
    catalog_path: PathBuf,
    digests: Option<SourceDigests>,
}

impl CatalogSnapshot {
    /// Copy `<catalog>`, `<catalog>-wal` and `<catalog>-shm` into a temp dir.
    pub fn capture(catalog: &Path) -> Result<Self> {
        Self::acquire(catalog, false)
    }

    /// [`Self::capture`], and prove the copy is the catalog.
    ///
    /// The catalog is hashed before the copy and the copy is hashed after it,
    /// so a pair that disagrees is a torn read — Lightroom wrote to the catalog
    /// while it was being read, or the disk returned different bytes — and a
    /// plan built from that copy would be a plan about a catalog that never
    /// existed. It is an error, never a warning: the whole claim is that the
    /// user's evidence was only ever read.
    ///
    /// The catalog file alone is hashed, never `-wal`/`-shm`: the sidecars are
    /// copied because SQLite needs them, and a Lightroom session appending to
    /// the WAL is what the snapshot exists to tolerate.
    pub fn capture_verified(catalog: &Path) -> Result<Self> {
        Self::acquire(catalog, true)
    }

    fn acquire(catalog: &Path, verify: bool) -> Result<Self> {
        let name = catalog.file_name().ok_or_else(|| Error::Io {
            path: catalog.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "catalog path has no file name",
            ),
        })?;

        let dir = tempfile::Builder::new()
            .prefix("lrsalvage-")
            .tempdir()
            .map_err(|source| Error::Io {
                path: std::env::temp_dir(),
                source,
            })?;

        // The catalog as it was when this run started, before anything we do to
        // it could be mistaken for what it always was.
        let source = if verify {
            Some(sha256_file(catalog)?)
        } else {
            None
        };

        // Keep the base name intact: SQLite only finds the sidecars when the
        // copy is called exactly the same thing as the original.
        let copy = dir.path().join(name);
        fs::copy(catalog, &copy).at(catalog)?;

        // Before the checkpoint, which rewrites the copy by folding our WAL into
        // it: after that the copy is ours, and no longer the catalog's bytes.
        let digests = match source {
            Some(source) => Some(verify_copy(catalog, &source, &copy)?),
            None => None,
        };

        for suffix in ["-wal", "-shm"] {
            let sidecar = with_suffix(catalog, suffix);
            if sidecar.is_file() {
                fs::copy(&sidecar, with_suffix(&copy, suffix)).at(&sidecar)?;
            }
        }

        checkpoint_copy(&copy);

        Ok(Self {
            _dir: dir,
            catalog_path: copy,
            digests,
        })
    }

    /// Path of the private copy of the catalog.
    pub fn path(&self) -> &Path {
        &self.catalog_path
    }

    /// The digest pair a verified capture proved, or `None` when this snapshot
    /// was taken without asking for the proof.
    pub fn digests(&self) -> Option<&SourceDigests> {
        self.digests.as_ref()
    }
}

/// The two SHA-256 digests a verified acquisition produced, as lowercase hex.
///
/// `source` is the catalog as this run read it and `snapshot` is the private
/// copy the rest of the run queried. A verified acquisition never hands out a
/// pair whose halves disagree — that is [`Error::SourceChanged`] instead — so a
/// report carrying both members is a proof rather than a claim.
#[derive(Debug)]
pub struct SourceDigests {
    source: String,
    snapshot: String,
}

impl SourceDigests {
    /// The catalog file's digest, taken before it was copied.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The private copy's digest, taken before it was checkpointed.
    pub fn snapshot(&self) -> &str {
        &self.snapshot
    }
}

/// Compare the catalog's digest with our copy's, and fail when they differ.
fn verify_copy(catalog: &Path, source: &str, copy: &Path) -> Result<SourceDigests> {
    let snapshot = sha256_file(copy)?;
    if source != snapshot {
        return Err(Error::SourceChanged {
            path: catalog.to_path_buf(),
            source_sha256: source.to_string(),
            snapshot_sha256: snapshot,
        });
    }
    Ok(SourceDigests {
        source: source.to_string(),
        snapshot,
    })
}

/// The SHA-256 of a file's bytes as lowercase hex, read in chunks so a catalog
/// of a few hundred megabytes costs no more memory than an empty one.
///
/// Public because it is the number `--verify-source` publishes: the CLI matrix
/// asserts the report's `source_sha256` against it, so the field and the file's
/// bytes cannot drift into two definitions of the same hex.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).at(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).at(path)?;
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

/// Fold the copied WAL into the copied database, then drop the sidecars.
///
/// Best effort: a copy that cannot be checkpointed (busy, corrupt tail) is
/// still readable through the WAL, so [`open_readonly`] falls back to a plain
/// read-only open in that case. Failing `capture` here would be worse than
/// returning a usable snapshot.
fn checkpoint_copy(copy: &Path) {
    let wal = with_suffix(copy, "-wal");
    if !wal.is_file() {
        return;
    }

    let checkpointed = Connection::open(copy)
        .and_then(|conn| {
            conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                row.get::<_, i64>(0)
            })
        })
        .map(|busy| busy == 0)
        .unwrap_or(false);

    // SQLite deletes both files when the last connection closes, but be
    // explicit: a leftover empty `-wal` would force the slower fallback path.
    if checkpointed {
        let _ = fs::remove_file(&wal);
        let _ = fs::remove_file(with_suffix(copy, "-shm"));
    }
}

/// `path` with a suffix appended to its final component, e.g. `x.lrcat-wal`.
pub(crate) fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(suffix);
    PathBuf::from(name)
}

/// Open a database read-only, never touching the original.
///
/// Uses `immutable=1` so SQLite skips locking and change detection, which is
/// safe precisely because the caller only ever hands us a private, finished
/// copy (see [`CatalogSnapshot`]). If a `-wal` file is still present — the
/// checkpoint did not happen — `immutable=1` would silently hide committed
/// rows, so fall back to a plain read-only open that replays the WAL.
pub(crate) fn open_readonly(path: &Path) -> Result<Connection> {
    let query = if with_suffix(path, "-wal").is_file() {
        "mode=ro"
    } else {
        "mode=ro&immutable=1"
    };
    let uri = format!("file:{}?{}", uri_path(path), query);
    let conn = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    Ok(conn)
}

/// Windows paths use `\`, and a URI needs those escaped along with the few
/// characters SQLite treats as query syntax.
fn uri_path(path: &Path) -> String {
    let mut out = String::with_capacity(path.as_os_str().len());
    for ch in path.to_string_lossy().chars() {
        match ch {
            '\\' => out.push('/'),
            '%' => out.push_str("%25"),
            '?' => out.push_str("%3F"),
            '#' => out.push_str("%23"),
            ' ' => out.push_str("%20"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A WAL-mode catalog with a row written but not checkpointed, returned
    /// with its connection still open — exactly what Lightroom looks like.
    fn live_wal_catalog(dir: &Path) -> (PathBuf, Connection) {
        let path = dir.join("2023.lrcat");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;\
             PRAGMA wal_autocheckpoint=0;\
             CREATE TABLE Adobe_images (id_local INTEGER PRIMARY KEY, name TEXT);",
        )
        .unwrap();
        // Move the schema into the main file so the interesting rows below are
        // the only thing the WAL holds.
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .unwrap();
        conn.execute(
            "INSERT INTO Adobe_images (id_local, name) VALUES (1, 'one'), (2, 'two')",
            [],
        )
        .unwrap();
        (path, conn)
    }

    fn count_images(path: &Path) -> i64 {
        let conn = open_readonly(path).unwrap();
        conn.query_row("SELECT count(*) FROM Adobe_images", [], |row| row.get(0))
            .unwrap()
    }

    fn names(path: &Path) -> Vec<String> {
        let conn = open_readonly(path).unwrap();
        let mut stmt = conn
            .prepare("SELECT name FROM Adobe_images ORDER BY id_local")
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap()
    }

    #[test]
    fn snapshot_contains_rows_that_exist_only_in_the_uncheckpointed_wal() {
        let live = tempfile::tempdir().unwrap();
        let (path, conn) = live_wal_catalog(live.path());

        // Premise: the main file alone does not know about the rows yet. If
        // this fails, SQLite checkpointed behind our back and the test proves
        // nothing.
        let main_only = live.path().join("main-only.lrcat");
        fs::copy(&path, &main_only).unwrap();
        assert_eq!(
            count_images(&main_only),
            0,
            "rows leaked into the main file"
        );

        let snapshot = CatalogSnapshot::capture(&path).unwrap();
        assert_eq!(snapshot.path().file_name().unwrap(), "2023.lrcat");
        assert_eq!(names(snapshot.path()), ["one", "two"]);
        drop(conn);
    }

    #[test]
    fn capture_leaves_the_live_catalog_and_its_wal_byte_identical() {
        let live = tempfile::tempdir().unwrap();
        let (path, conn) = live_wal_catalog(live.path());
        let wal = with_suffix(&path, "-wal");
        assert!(wal.is_file(), "test needs a live WAL file");

        let before_catalog = fs::read(&path).unwrap();
        let before_wal = fs::read(&wal).unwrap();

        let snapshot = CatalogSnapshot::capture(&path).unwrap();
        assert!(snapshot.path().is_file());

        assert_eq!(fs::read(&path).unwrap(), before_catalog);
        assert_eq!(fs::read(&wal).unwrap(), before_wal);
        drop(conn);
    }

    #[test]
    fn the_digest_is_the_sha256_of_the_file_bytes_in_lowercase_hex() {
        let live = tempfile::tempdir().unwrap();
        let empty = live.path().join("empty.bin");
        fs::write(&empty, b"").unwrap();
        assert_eq!(
            sha256_file(&empty).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        let abc = live.path().join("abc.bin");
        fs::write(&abc, b"abc").unwrap();
        assert_eq!(
            sha256_file(&abc).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_verified_capture_proves_the_copy_is_the_catalog() {
        let live = tempfile::tempdir().unwrap();
        let path = live.path().join("clean.lrcat");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE Adobe_images (id_local INTEGER PRIMARY KEY, name TEXT);
                 INSERT INTO Adobe_images (id_local, name) VALUES (1, 'one');",
            )
            .unwrap();
        }

        let snapshot = CatalogSnapshot::capture_verified(&path).unwrap();
        let digests = snapshot
            .digests()
            .expect("a verified capture reports its pair");
        assert_eq!(digests.source(), digests.snapshot());
        assert_eq!(digests.source().len(), 64);
        assert_eq!(
            digests.source(),
            sha256_file(&path).unwrap(),
            "the reported digest is the catalog's own"
        );

        // A snapshot nobody asked to verify reports no pair: an unmeasured fact
        // is absent, never a stand-in.
        let plain = CatalogSnapshot::capture(&path).unwrap();
        assert!(plain.digests().is_none());
    }

    #[test]
    fn a_live_wal_does_not_trip_the_verified_capture() {
        // The sidecars are copied, never hashed: a Lightroom session appending
        // to the WAL is the case the snapshot exists to tolerate.
        let live = tempfile::tempdir().unwrap();
        let (path, conn) = live_wal_catalog(live.path());
        assert!(
            with_suffix(&path, "-wal").is_file(),
            "test needs a live WAL"
        );

        let snapshot = CatalogSnapshot::capture_verified(&path).unwrap();
        let digests = snapshot.digests().expect("a verified capture");
        assert_eq!(digests.source(), digests.snapshot());
        drop(conn);
    }

    #[test]
    fn a_copy_that_is_not_the_catalog_fails_naming_both_digests() {
        let live = tempfile::tempdir().unwrap();
        let catalog = live.path().join("2023.lrcat");
        fs::write(&catalog, b"the catalog as it was").unwrap();
        let copy = live.path().join("copy.lrcat");
        fs::copy(&catalog, &copy).unwrap();

        let source = sha256_file(&catalog).unwrap();
        // What a torn read leaves behind: the pair would agree only if the copy
        // were still the catalog's bytes.
        fs::write(&copy, b"not the catalog").unwrap();

        let error = verify_copy(&catalog, &source, &copy).expect_err("a mismatch is an error");
        let Error::SourceChanged {
            path,
            source_sha256,
            snapshot_sha256,
        } = &error
        else {
            panic!("expected SourceChanged, got {error:?}");
        };
        assert_eq!(path, &catalog);
        assert_eq!(source_sha256, &source);
        assert_eq!(snapshot_sha256, &sha256_file(&copy).unwrap());
        assert_ne!(source_sha256, snapshot_sha256);

        let message = error.to_string();
        for stated in [
            path.to_string_lossy().into_owned().as_str(),
            source_sha256,
            snapshot_sha256,
        ] {
            assert!(
                message.contains(stated),
                "the message states {stated:?}:\n{message}"
            );
        }
        assert!(message.contains("changed"), "{message}");
    }

    #[test]
    fn captures_a_database_that_has_no_sidecars() {
        let live = tempfile::tempdir().unwrap();
        let path = live.path().join("clean.lrcat");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE Adobe_images (id_local INTEGER PRIMARY KEY, name TEXT);
                 INSERT INTO Adobe_images (id_local, name) VALUES (7, 'seven');",
            )
            .unwrap();
        }
        assert!(!with_suffix(&path, "-wal").exists());

        let snapshot = CatalogSnapshot::capture(&path).unwrap();
        assert_eq!(names(snapshot.path()), ["seven"]);
    }

    #[test]
    fn missing_catalog_is_an_io_error_naming_the_path() {
        let live = tempfile::tempdir().unwrap();
        let missing = live.path().join("nope.lrcat");

        match CatalogSnapshot::capture(&missing) {
            Err(Error::Io { path, .. }) => assert_eq!(path, missing),
            other => panic!("expected Error::Io, got {:?}", other.err()),
        }
    }
}
