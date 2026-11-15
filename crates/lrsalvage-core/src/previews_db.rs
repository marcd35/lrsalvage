//! `previews.db` — the authoritative map from catalog image to payload file.
//!
//! Reads the cache's index into memory and derives the paths its rows name: one
//! container per pyramid, one level file per `PyramidLevel` row, widened by the
//! names in that pyramid's own shard folder. Whether a derived file exists is
//! the caller's question — this module reads no payload byte.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::types::ValueRef;

use crate::error::Result;
use crate::model::{CacheEntry, Pyramid, PyramidLevel};

/// The container name template: `<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>.lrprev`.
///
/// Placeholders only, never a name. `report` prints it verbatim so the shape of
/// a cache can be published without publishing one of its files
/// (`adr-0007-publish-a-verification-report`).
pub const CONTAINER_GRAMMAR: &str = "<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>.lrprev";

/// The level file name template: the container path with `_<longedge>` in place
/// of the extension.
pub const LEVEL_FILE_GRAMMAR: &str = "<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>_<longedge>";

/// In-memory view of a `previews.db`.
#[derive(Debug, Clone, Default)]
pub struct PreviewIndex {
    pub entries: HashMap<i64, CacheEntry>,
    /// Every row, including ones whose `imageId` repeats. Needed so payload
    /// sweeps do not mistake a duplicate row's file for an orphan.
    pub all_entries: Vec<CacheEntry>,
    pub pyramids: HashMap<String, Pyramid>,
    pub levels: HashMap<String, Vec<PyramidLevel>>,
}

/// One level file a pyramid's rows name.
///
/// The row travels with the path because it is the claim a level is judged
/// against; nothing else has to match a file back to a row.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate<'a> {
    /// `<cache>/<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>_<round(longDimension)>`
    pub path: PathBuf,
    /// The `PyramidLevel` row that derived the path, `None` for a file only
    /// the bounded glob found (`adr-0005-discover-payloads-by-index` ¶3).
    pub row: Option<&'a PyramidLevel>,
    /// The long edge the leaf names: the rounded `longDimension` for a
    /// row-derived name, the parsed suffix for a file only the glob found.
    pub long_edge: u32,
}

impl PreviewIndex {
    /// Open `<cache>/previews.db` read-only.
    pub fn open(db: &Path) -> Result<Self> {
        let conn = crate::snapshot::open_readonly(db)?;
        let mut index = Self::default();

        {
            let mut stmt = conn.prepare(
                "SELECT uuid, digest, colorProfile, quality, croppedWidth, croppedHeight \
                 FROM Pyramid",
            )?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let uuid = text(row.get_ref(0)?);
                index.pyramids.insert(
                    uuid.clone(),
                    Pyramid {
                        uuid,
                        digest: text(row.get_ref(1)?),
                        color_profile: text(row.get_ref(2)?),
                        quality: text(row.get_ref(3)?),
                        cropped_width: unsigned(row.get_ref(4)?),
                        cropped_height: unsigned(row.get_ref(5)?),
                    },
                );
            }
        }

        {
            let mut stmt =
                conn.prepare("SELECT uuid, level, longDimension, width, height FROM PyramidLevel")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let uuid = text(row.get_ref(0)?);
                index
                    .levels
                    .entry(uuid.clone())
                    .or_default()
                    .push(PyramidLevel {
                        uuid,
                        level: unsigned(row.get_ref(1)?),
                        long_dimension: unsigned(row.get_ref(2)?),
                        width: unsigned(row.get_ref(3)?),
                        height: unsigned(row.get_ref(4)?),
                    });
            }
        }

        {
            let mut stmt =
                conn.prepare("SELECT imageId, uuid, digest, orientation FROM ImageCacheEntry")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let image_id = signed(row.get_ref(0)?);
                // Real caches hold duplicates (3420 rows for 3416 images). The
                // later row is the one Lightroom would use, so keep it.
                let entry = CacheEntry {
                    image_id,
                    uuid: text(row.get_ref(1)?),
                    digest: text(row.get_ref(2)?),
                    orientation: match row.get_ref(3)? {
                        ValueRef::Null => None,
                        other => Some(text(other)),
                    },
                };
                index.all_entries.push(entry.clone());
                index.entries.insert(image_id, entry);
            }
        }

        Ok(index)
    }

    /// `<cache>/<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>.lrprev`
    pub fn payload_path(&self, cache_root: &Path, entry: &CacheEntry) -> PathBuf {
        shard(cache_root, &entry.uuid).join(format!("{}-{}.lrprev", entry.uuid, entry.digest))
    }

    /// Every level file of this pyramid, in path order: one leaf per
    /// `PyramidLevel` row, then any file in *this pyramid's own* shard folder
    /// named exactly `<UUID>-<digest>_<digits>`.
    ///
    /// The row-derived path is derived, never globbed: the suffix is the
    /// rounded `longDimension`, because the column comes back `REAL` (`101.0`)
    /// and formatting the float would match nothing (§5.7). Two rows that round
    /// to one leaf name one file, so the lower level number keeps it.
    ///
    /// The widening is bounded by this uuid **and** digest in this uuid's own
    /// folder, so it can find a level whose `PyramidLevel` row was pruned and
    /// cannot invent a mapping: a file whose digest differs is not a member
    /// (`adr-0005-discover-payloads-by-index` ¶3, ¶6). A row-derived path is
    /// kept over the same path the widening found, because the row is the claim
    /// `plan` compares against. Only names are read — never a payload byte — and
    /// a missing folder is a pyramid with nothing on disk, not an error.
    ///
    /// Whether a candidate is on disk is the caller's question: this module
    /// stats no file and reads no payload, so `plan` decides existence.
    pub fn candidates(&self, cache_root: &Path, uuid: &str, digest: &str) -> Vec<Candidate<'_>> {
        let mut named: Vec<Candidate<'_>> = self
            .levels
            .get(uuid)
            .map(|rows| {
                rows.iter()
                    .map(|row| Candidate {
                        path: shard(cache_root, uuid)
                            .join(format!("{uuid}-{digest}_{}", row.long_dimension)),
                        row: Some(row),
                        long_edge: row.long_dimension,
                    })
                    .collect()
            })
            .unwrap_or_default();

        // The widening, inside the row's own identity: this pyramid's shard
        // folder and nothing else. A level the index forgot is a file here.
        if let Ok(entries) = std::fs::read_dir(shard(cache_root, uuid)) {
            for entry in entries.flatten() {
                if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                    continue;
                }
                let Ok(name) = entry.file_name().into_string() else {
                    continue;
                };
                if let Some(long_edge) = level_leaf_long_edge(&name, uuid, digest) {
                    named.push(Candidate {
                        path: entry.path(),
                        row: None,
                        long_edge,
                    });
                }
            }
        }

        // Path first so the order never depends on the database's row order or
        // on `read_dir`; a row keeps a path the widening also found, and the
        // level number breaks a tie between two rows naming one leaf.
        named.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then_with(|| a.row.is_none().cmp(&b.row.is_none()))
                .then_with(|| a.row.map(|row| row.level).cmp(&b.row.map(|row| row.level)))
        });
        named.dedup_by(|a, b| a.path == b.path);
        named
    }

    /// Largest level for a pyramid by pixel area (`width * height`), keeping
    /// the highest `level` on a tie. Neither the highest level number nor the
    /// longest dimension reliably identifies the 1616px preview, because real
    /// caches store levels in an arbitrary order.
    pub fn best_level(&self, uuid: &str) -> Option<&PyramidLevel> {
        self.levels.get(uuid)?.iter().max_by_key(|level| {
            (
                u64::from(level.width) * u64::from(level.height),
                level.level,
            )
        })
    }
}

/// `<cache>/<uuid[0]>/<uuid[0..4]>`: the shard folders, which are an address
/// and never an identity (§5.2).
fn shard(cache_root: &Path, uuid: &str) -> PathBuf {
    let first: String = uuid.chars().take(1).collect();
    let first_four: String = uuid.chars().take(4).collect();
    cache_root.join(first).join(first_four)
}

/// The long edge a level file's leaf names, when the name is exactly
/// `<UUID>-<digest>_<digits>` for this uuid and digest.
///
/// Case is significant: both are compared verbatim, because the exact case of a
/// uuid and a digest matters on a case-sensitive filesystem.
fn level_leaf_long_edge(name: &str, uuid: &str, digest: &str) -> Option<u32> {
    let suffix = name
        .strip_prefix(uuid)?
        .strip_prefix('-')?
        .strip_prefix(digest)?
        .strip_prefix('_')?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    suffix.parse().ok()
}

/// Text column, tolerating odd storage classes.
fn text(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Null => String::new(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => {
            String::from_utf8_lossy(bytes).into_owned()
        }
    }
}

/// Numeric column stored as INTEGER or REAL in practice (`1616.0`).
fn number(value: ValueRef<'_>) -> Option<f64> {
    match value {
        ValueRef::Integer(i) => Some(i as f64),
        ValueRef::Real(f) => Some(f),
        ValueRef::Text(bytes) => std::str::from_utf8(bytes).ok()?.trim().parse().ok(),
        ValueRef::Null | ValueRef::Blob(_) => None,
    }
}

fn unsigned(value: ValueRef<'_>) -> u32 {
    number(value)
        .map(|n| n.round().clamp(0.0, f64::from(u32::MAX)) as u32)
        .unwrap_or(0)
}

fn signed(value: ValueRef<'_>) -> i64 {
    number(value).map(|n| n.round() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use rusqlite::Connection;

    use super::*;

    fn slashed(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    const SCHEMA: &str = r#"
        CREATE TABLE Pyramid (uuid NOT NULL, digest NOT NULL, colorProfile NOT NULL,
          fileTimeStamp, quality NOT NULL, croppedWidth NOT NULL, croppedHeight NOT NULL,
          pyramidFileTimeStamp, fingerprint, fromProxy);
        CREATE TABLE PyramidLevel (uuid NOT NULL, digest NOT NULL, level NOT NULL,
          longDimension NOT NULL, lastAccess NOT NULL, width NOT NULL, height NOT NULL);
        CREATE TABLE ImageCacheEntry (imageId NOT NULL, uuid NOT NULL, digest NOT NULL,
          orientation);
        CREATE TABLE DeletedPyramid (uuid NOT NULL, digest NOT NULL, deletedTimeStamp NOT NULL);
    "#;

    /// Build a cache, seed it, and read it back the way the tool does.
    fn index_with(seed: impl FnOnce(&Connection)) -> PreviewIndex {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("previews.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        seed(&conn);
        drop(conn);
        PreviewIndex::open(&path).unwrap()
    }

    fn base_pyramid(conn: &Connection, uuid: &str) {
        conn.execute(
            "INSERT INTO Pyramid (uuid, digest, colorProfile, quality, croppedWidth, croppedHeight) \
             VALUES (?1, 'digest', 'ACR', 'HIGH', 1616.0, 1077.0)",
            [uuid],
        )
        .unwrap();
    }

    #[test]
    fn reads_entries_keyed_by_image_id() {
        let index = index_with(|conn| {
            conn.execute(
                "INSERT INTO ImageCacheEntry (imageId, uuid, digest, orientation) \
                 VALUES (4062.0, 'ABCD1234-0000', 'deadbeef', 'AB')",
                [],
            )
            .unwrap();
        });

        let entry = index.entries.get(&4062).expect("float imageId coerced");
        assert_eq!(entry.uuid, "ABCD1234-0000");
        assert_eq!(entry.digest, "deadbeef");
        assert_eq!(entry.orientation.as_deref(), Some("AB"));
    }

    #[test]
    fn duplicate_image_id_keeps_the_last_row() {
        let index = index_with(|conn| {
            conn.execute(
                "INSERT INTO ImageCacheEntry (imageId, uuid, digest, orientation) \
                 VALUES (4062.0, 'OLD', 'aaa', 'AB'), (4062.0, 'NEW', 'bbb', 'BC')",
                [],
            )
            .unwrap();
        });

        assert_eq!(index.entries.len(), 1);
        assert_eq!(index.entries[&4062].digest, "bbb");
        assert_eq!(index.entries[&4062].uuid, "NEW");
    }

    #[test]
    fn pyramid_dimensions_are_coerced_from_reals() {
        let index = index_with(|conn| base_pyramid(conn, "ABCD"));

        let pyramid = &index.pyramids["ABCD"];
        assert_eq!(pyramid.cropped_width, 1616);
        assert_eq!(pyramid.cropped_height, 1077);
        assert_eq!(pyramid.quality, "HIGH");
        assert_eq!(pyramid.color_profile, "ACR");
    }

    #[test]
    fn payload_path_follows_the_preview_cache_layout() {
        let index = index_with(|conn| {
            conn.execute(
                "INSERT INTO ImageCacheEntry (imageId, uuid, digest, orientation) \
                 VALUES (4062.0, '1A2B3C4D-5E6F', 'deadbeef', 'AB')",
                [],
            )
            .unwrap();
        });

        let path = index.payload_path(
            Path::new("C:/cache/2023 Previews.lrdata"),
            &index.entries[&4062],
        );
        assert_eq!(
            path,
            Path::new("C:/cache/2023 Previews.lrdata/1/1A2B/1A2B3C4D-5E6F-deadbeef.lrprev")
        );
    }

    #[test]
    fn best_level_is_the_largest_area_not_the_highest_number() {
        let index = index_with(|conn| {
            base_pyramid(conn, "ABCD");
            conn.execute_batch(
                "INSERT INTO PyramidLevel (uuid, digest, level, longDimension, lastAccess, width, height)
                 VALUES ('ABCD', 'd', 0, 1616, 0, 1616, 1077),
                        ('ABCD', 'd', 1, 1024, 0, 1024, 683),
                        ('ABCD', 'd', 2, 640,  0, 640,  427);",
            )
            .unwrap();
        });

        let best = index.best_level("ABCD").unwrap();
        assert_eq!((best.level, best.width, best.height), (0, 1616, 1077));
    }

    #[test]
    fn best_level_breaks_area_ties_on_level() {
        let index = index_with(|conn| {
            base_pyramid(conn, "ABCD");
            conn.execute_batch(
                "INSERT INTO PyramidLevel (uuid, digest, level, longDimension, lastAccess, width, height)
                 VALUES ('ABCD', 'd', 1, 800, 0, 800, 600),
                        ('ABCD', 'd', 3, 800, 0, 800, 600);",
            )
            .unwrap();
        });

        assert_eq!(index.best_level("ABCD").unwrap().level, 3);
    }

    #[test]
    fn best_level_unknown_pyramid_is_none() {
        let index = index_with(|_| {});
        assert!(index.best_level("missing").is_none());
    }

    #[test]
    fn candidates_name_each_rows_level_file() {
        let index = index_with(|conn| {
            base_pyramid(conn, "1A2B3C4D-5E6F");
            conn.execute_batch(
                "INSERT INTO PyramidLevel (uuid, digest, level, longDimension, lastAccess, width, height)
                 VALUES ('1A2B3C4D-5E6F', 'd', 0, 1616.0, 0, 1616, 1077),
                        ('1A2B3C4D-5E6F', 'd', 2, 101.0,  0, 101,  68);",
            )
            .unwrap();
        });

        let candidates = index.candidates(Path::new("C:/cache"), "1A2B3C4D-5E6F", "deadbeef");
        let paths: Vec<String> = candidates.iter().map(|c| slashed(&c.path)).collect();
        assert_eq!(
            paths,
            vec![
                "C:/cache/1/1A2B/1A2B3C4D-5E6F-deadbeef_101",
                "C:/cache/1/1A2B/1A2B3C4D-5E6F-deadbeef_1616",
            ],
            "one leaf per row, in path order, under the address the uuid derives"
        );
        assert_eq!(
            candidates[0]
                .row
                .map(|row| (row.level, row.width, row.height)),
            Some((2, 101, 68)),
            "each candidate carries the row that derived its name"
        );
        assert_eq!(candidates[1].row.map(|row| row.long_dimension), Some(1616));
    }

    #[test]
    fn a_real_long_dimension_rounds_and_two_rows_may_name_one_leaf() {
        // §5.7: `longDimension` comes back REAL, so 100.6 and 101.0 both name
        // `_101` — the suffix is the rounded integer, never the float.
        let index = index_with(|conn| {
            base_pyramid(conn, "ABCD");
            conn.execute_batch(
                "INSERT INTO PyramidLevel (uuid, digest, level, longDimension, lastAccess, width, height)
                 VALUES ('ABCD', 'd', 1, 100.6, 0, 101, 68),
                        ('ABCD', 'd', 2, 101.0, 0, 101, 68);",
            )
            .unwrap();
        });

        let candidates = index.candidates(Path::new("C:/cache"), "ABCD", "d");
        assert_eq!(candidates.len(), 1, "one leaf is one file");
        assert!(
            candidates[0].path.ends_with("ABCD-d_101"),
            "{:?}",
            candidates[0].path
        );
        assert_eq!(
            candidates[0].row.map(|row| row.level),
            Some(1),
            "the lower level number keeps a shared leaf, deterministically"
        );
    }

    #[test]
    fn a_pyramid_without_level_rows_has_no_candidates() {
        let index = index_with(|conn| base_pyramid(conn, "ABCD"));
        assert!(
            index
                .candidates(Path::new("C:/cache"), "ABCD", "d")
                .is_empty()
        );
        assert!(
            index
                .candidates(Path::new("C:/cache"), "nope", "d")
                .is_empty()
        );
    }

    /// The shard folder a uuid derives, created under `root`.
    fn shard_dir(root: &Path, uuid: &str) -> PathBuf {
        let first: String = uuid.chars().take(1).collect();
        let first_four: String = uuid.chars().take(4).collect();
        let dir = root.join(first).join(first_four);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn names(candidates: &[Candidate<'_>]) -> Vec<String> {
        candidates
            .iter()
            .map(|candidate| {
                candidate
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    #[test]
    fn the_widening_accepts_only_this_pyramids_own_level_files() {
        let uuid = "1A2B3C4D-5E6F";
        let digest = "deadbeef";
        let dir = tempfile::tempdir().unwrap();
        let shard = shard_dir(dir.path(), uuid);
        let write = |name: &str| fs::write(shard.join(name), b"not read").unwrap();

        write(&format!("{uuid}-{digest}_1616"));
        write(&format!("{uuid}-{digest}_640"));
        // A container is not a level file.
        write(&format!("{uuid}-{digest}.lrprev"));
        // A differing digest is another pyramid, not this one's level (¶6).
        write(&format!("{uuid}-0000beef_1616"));
        // No numeric suffix, no digits, and a trailing underscore.
        write(&format!("{uuid}-{digest}"));
        write(&format!("{uuid}-{digest}_"));
        write(&format!("{uuid}-{digest}_640.old"));
        write(&format!("{uuid}-{digest}_wide"));
        // Another pyramid's leaf, sitting in this folder.
        write("OTHER0000-0000-0000-0000-000000000000-deadbeef_640");

        // No `PyramidLevel` row at all: the widening alone finds these.
        let index = index_with(|conn| base_pyramid(conn, uuid));
        let candidates = index.candidates(dir.path(), uuid, digest);

        assert_eq!(
            names(&candidates),
            vec![
                format!("{uuid}-{digest}_1616"),
                format!("{uuid}-{digest}_640")
            ],
            "only this uuid and digest, in path order"
        );
        assert!(
            candidates.iter().all(|candidate| candidate.row.is_none()),
            "a file only the widening found carries no claim"
        );
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.long_edge)
                .collect::<Vec<_>>(),
            vec![1616, 640],
            "the suffix is the long edge the leaf names"
        );
    }

    #[test]
    fn a_file_a_row_already_named_is_one_candidate_and_keeps_its_row() {
        let uuid = "1A2B3C4D-5E6F";
        let digest = "deadbeef";
        let dir = tempfile::tempdir().unwrap();
        let shard = shard_dir(dir.path(), uuid);
        fs::write(shard.join(format!("{uuid}-{digest}_640")), b"not read").unwrap();

        let index = index_with(|conn| {
            base_pyramid(conn, uuid);
            conn.execute_batch(&format!(
                "INSERT INTO PyramidLevel (uuid, digest, level, longDimension, lastAccess, width, height)
                 VALUES ('{uuid}', 'd', 1, 640.0, 0, 640, 427);"
            ))
            .unwrap();
        });

        let candidates = index.candidates(dir.path(), uuid, digest);
        assert_eq!(names(&candidates), vec![format!("{uuid}-{digest}_640")]);
        assert_eq!(
            candidates[0]
                .row
                .map(|row| (row.level, row.width, row.height)),
            Some((1, 640, 427)),
            "the row that derived the path is the claim, not the widened copy"
        );
    }

    #[test]
    fn the_name_templates_agree_with_the_paths_they_stand_for() {
        let uuid = "1A2B3C4D-5E6F";
        let digest = "deadbeef";
        let dir = tempfile::tempdir().unwrap();
        let shard = shard_dir(dir.path(), uuid);
        fs::write(shard.join(format!("{uuid}-{digest}_1616")), b"not read").unwrap();

        let index = index_with(|conn| {
            conn.execute(
                "INSERT INTO ImageCacheEntry (imageId, uuid, digest, orientation) \
                 VALUES (4062.0, '1A2B3C4D-5E6F', 'deadbeef', 'AB')",
                [],
            )
            .unwrap();
            base_pyramid(conn, uuid);
            conn.execute_batch(&format!(
                "INSERT INTO PyramidLevel (uuid, digest, level, longDimension, lastAccess, width, height)
                 VALUES ('{uuid}', 'deadbeef', 0, 1616.0, 0, 1616, 1077);"
            ))
            .unwrap();
        });

        let entry = &index.entries[&4062];
        // Substituting a known pair into a template is what the verification
        // report will do; the template must then describe the derived path.
        let fill = |template: &str, long_edge: &str| {
            template
                .replace("<uuid[0]>", &uuid[..1])
                .replace("<uuid[0..4]>", &uuid[..4])
                .replace("<UUID>", uuid)
                .replace("<digest>", digest)
                .replace("<longedge>", long_edge)
        };

        assert_eq!(
            slashed(&index.payload_path(dir.path(), entry)),
            slashed(&dir.path().join(fill(CONTAINER_GRAMMAR, ""))),
            "CONTAINER_GRAMMAR must describe the path payload_path derives"
        );
        let candidates = index.candidates(dir.path(), uuid, digest);
        assert_eq!(candidates.len(), 1, "{candidates:?}");
        assert_eq!(
            slashed(&candidates[0].path),
            slashed(&dir.path().join(fill(LEVEL_FILE_GRAMMAR, "1616"))),
            "LEVEL_FILE_GRAMMAR must describe the path candidates derives"
        );
    }

    #[test]
    fn missing_tables_surface_as_a_sqlite_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("previews.db");
        fs::write(&path, b"").unwrap();

        match PreviewIndex::open(&path) {
            Err(crate::error::Error::Sqlite(_)) => {}
            other => panic!("expected Error::Sqlite, got {:?}", other.err()),
        }
    }
}
