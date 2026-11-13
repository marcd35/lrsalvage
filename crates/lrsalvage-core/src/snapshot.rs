//! A stable, read-only snapshot of a catalog file and its WAL sidecars.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
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
}

impl CatalogSnapshot {
    /// Copy `<catalog>`, `<catalog>-wal` and `<catalog>-shm` into a temp dir.
    pub fn capture(catalog: &Path) -> Result<Self> {
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

        // Keep the base name intact: SQLite only finds the sidecars when the
        // copy is called exactly the same thing as the original.
        let copy = dir.path().join(name);
        fs::copy(catalog, &copy).at(catalog)?;

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
        })
    }

    /// Path of the private copy of the catalog.
    pub fn path(&self) -> &Path {
        &self.catalog_path
    }
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
