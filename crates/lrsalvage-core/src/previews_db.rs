//! `previews.db` — the authoritative map from catalog image to payload file.
//!
//! Reads the cache's index into memory and derives the paths its rows name: one
//! container per pyramid, one level file per `PyramidLevel` row, widened by the
//! names in that pyramid's own shard folder. It also enumerates the cache tree
//! once, into one entry per `(uuid, digest)` present on disk, which is what the
//! caller's orphan sweep counts. Whether a derived file exists is the caller's
//! question — this module reads no payload byte.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use rusqlite::Connection;
use rusqlite::types::ValueRef;
use walkdir::WalkDir;

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

/// What the cache holds, in ADR 0004's five words.
///
/// Decided from the tree alone — never from a version, which is why nothing in
/// this module compares one to a layout (`adr-0004-detect-layout-not-version`).
/// `mixed` describes the cache, not an image: per-image supersession is
/// `plan`'s `superseded` count.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Layout {
    /// `.lrprev` containers only.
    Legacy,
    /// Loose payload files only, one file per level.
    Modern,
    /// Both kinds, anywhere in the cache.
    Mixed,
    /// An index and no payload files at all.
    #[default]
    None,
    /// Payload-like files exist, but none is a shape this reader knows.
    Unknown,
}

impl Layout {
    /// The word a report prints.
    pub fn as_str(self) -> &'static str {
        match self {
            Layout::Legacy => "legacy",
            Layout::Modern => "modern",
            Layout::Mixed => "mixed",
            Layout::None => "none",
            Layout::Unknown => "unknown",
        }
    }
}

/// In-memory view of a `previews.db`.
#[derive(Debug, Clone, Default)]
pub struct PreviewIndex {
    pub entries: HashMap<i64, CacheEntry>,
    /// Every row, including ones whose `imageId` repeats. Needed so payload
    /// sweeps do not mistake a duplicate row's file for an orphan.
    pub all_entries: Vec<CacheEntry>,
    pub pyramids: HashMap<String, Pyramid>,
    pub levels: HashMap<String, Vec<PyramidLevel>>,
    /// `DeletedPyramid` rows: `(uuid, digest)` → the `deletedTimeStamp`
    /// Lightroom recorded. A tombstone narrates; it never subtracts from a
    /// count, suppresses an orphan or changes a status
    /// (`adr-0006-degrade-without-guessing`).
    pub deleted: HashMap<(String, String), f64>,
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

        {
            // A store that predates the table reports no tombstones rather than
            // refusing a cache whose payloads are all readable.
            if table_exists(&conn, "DeletedPyramid")? {
                let mut stmt =
                    conn.prepare("SELECT uuid, digest, deletedTimeStamp FROM DeletedPyramid")?;
                let mut rows = stmt.query([])?;
                while let Some(row) = rows.next()? {
                    let uuid = text(row.get_ref(0)?);
                    let digest = text(row.get_ref(1)?);
                    index
                        .deleted
                        .insert((uuid, digest), number(row.get_ref(2)?).unwrap_or(0.0));
                }
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

/// One pyramid the cache tree holds, identified by the `(uuid, digest)` its own
/// leaves name — the key legacy and modern layouts share.
///
/// It merges both representations of one pyramid: every level file and, when the
/// cache holds one, the coexisting `.lrprev`. No byte was read to build it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyramidOnDisk {
    pub uuid: String,
    pub digest: String,
    /// Every level file of this pyramid, in path order.
    pub levels: Vec<LevelOnDisk>,
    /// The coexisting `<UUID>-<digest>.lrprev`, when there is one.
    pub container: Option<PathBuf>,
}

impl PyramidOnDisk {
    /// The largest level file this pyramid holds, by the `_<n>` its leaf names.
    ///
    /// No byte is read: the name is the only ranking a sweep has, which is why
    /// it can never certify a payload — `plan` does that for a claimed one.
    pub fn largest_level(&self) -> Option<&Path> {
        self.levels
            .iter()
            .max_by_key(|level| level.long_edge)
            .map(|level| level.path.as_path())
    }
}

/// One level file a tree walk found, with the long edge its leaf names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelOnDisk {
    pub path: PathBuf,
    pub long_edge: u32,
}

/// Everything one walk of the cache tree found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheSweep {
    /// One entry per `(uuid, digest)` present on disk, in uuid then digest
    /// order. Which of them an image claims is `plan`'s decision.
    pub pyramids: Vec<PyramidOnDisk>,
    /// Payload-like files that are no shape this reader knows: a leaf with no
    /// numeric suffix, one in a shard folder its uuid does not derive, one whose
    /// uuid does not parse. In path order.
    ///
    /// "Payload-like" is ADR 0004's narrow rule — a file whose first two bytes
    /// are `FF D8` — so these are reported rather than silently ignored, and
    /// they feed the `unknown` layout instead of the orphan count
    /// (`adr-0005-discover-payloads-by-index` ¶6).
    pub unrecognised: Vec<PathBuf>,
}

impl CacheSweep {
    /// ADR 0004's five words, decided from the tree alone.
    ///
    /// Only a payload file makes a cache `legacy`, `modern` or `mixed`; an
    /// index that holds nothing is `none`, and payload-like files that matched
    /// no shape are `unknown` — the two cases a user most needs told apart.
    pub fn layout(&self) -> Layout {
        let mut level_files = false;
        let mut containers = false;
        for pyramid in &self.pyramids {
            level_files |= !pyramid.levels.is_empty();
            containers |= pyramid.container.is_some();
        }
        match (level_files, containers) {
            (true, true) => Layout::Mixed,
            (true, false) => Layout::Modern,
            (false, true) => Layout::Legacy,
            (false, false) if self.unrecognised.is_empty() => Layout::None,
            (false, false) => Layout::Unknown,
        }
    }
}

/// One walk of the cache tree: every pyramid it holds, and every payload-like
/// file that is no pyramid at all.
///
/// The enumeration the orphan sweep counts, so it is one entry per pyramid and
/// never one per file — one orphan is one lost photograph, whatever its level
/// set looks like (`adr-0005-discover-payloads-by-index` ¶8). A file is a member
/// only when its leaf parses to a uuid and a 32-character lower-case hex digest
/// (plus a numeric suffix for a level file) **and** both shard folders agree
/// with that uuid, because the folder is an address and never an identity
/// (§5.2). Everything else is offered to the two-byte sniff and nothing more:
/// `previews.db` and its `-wal`/`-shm` neighbours do not start with `FF D8`, so
/// they stay neither payload nor pyramid.
///
/// Which of these pyramids an image claims, and what an unclaimed one becomes,
/// is `plan`'s decision, not this module's.
pub fn sweep(cache_root: &Path) -> CacheSweep {
    let mut found: HashMap<(String, String), PyramidOnDisk> = HashMap::new();
    let mut unrecognised = Vec::new();

    for entry in WalkDir::new(cache_root)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let Some((uuid, digest, level)) = leaf_identity(cache_root, entry.path()) else {
            if crate::jpeg::is_payload_start(entry.path()) {
                unrecognised.push(entry.into_path());
            }
            continue;
        };
        let pyramid = found
            .entry((uuid.clone(), digest.clone()))
            .or_insert_with(|| PyramidOnDisk {
                uuid,
                digest,
                levels: Vec::new(),
                container: None,
            });
        match level {
            Some(long_edge) => pyramid.levels.push(LevelOnDisk {
                path: entry.into_path(),
                long_edge,
            }),
            None => pyramid.container = Some(entry.into_path()),
        }
    }

    let mut pyramids: Vec<PyramidOnDisk> = found.into_values().collect();
    for pyramid in &mut pyramids {
        pyramid.levels.sort_by(|a, b| a.path.cmp(&b.path));
    }
    pyramids.sort_by(|a, b| (&a.uuid, &a.digest).cmp(&(&b.uuid, &b.digest)));
    unrecognised.sort();

    CacheSweep {
        pyramids,
        unrecognised,
    }
}

/// The `(uuid, digest)` a file's leaf names and, for a level file, its long
/// edge — when the leaf is at a payload address and both shard folders agree
/// with the uuid it names.
fn leaf_identity(cache_root: &Path, path: &Path) -> Option<(String, String, Option<u32>)> {
    let relative = path.strip_prefix(cache_root).ok()?;
    let parts: Vec<&str> = relative
        .components()
        .map(|component| match component {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect::<Option<_>>()?;
    let [first, first_four, leaf] = parts.as_slice() else {
        return None;
    };
    let (uuid, digest, level) = parse_leaf(leaf)?;
    // `<uuid[0]>/<uuid[0..4]>`: the folders are an address, never an identity.
    if *first != uuid.get(..1)? || *first_four != uuid.get(..4)? {
        return None;
    }
    Some((uuid.to_string(), digest.to_string(), level))
}

/// A leaf name split into `(uuid, digest)` and, for a level file, its long edge.
///
/// A leaf is `<UUID>-<digest>.lrprev` or `<UUID>-<digest>_<digits>`. The uuid is
/// five hex groups of 8-4-4-4-12 characters and the digest 32 lower-case hex
/// characters (§5.2); anything else is not a payload this reader knows.
fn parse_leaf(name: &str) -> Option<(&str, &str, Option<u32>)> {
    let (stem, level) = match name.strip_suffix(".lrprev") {
        Some(stem) => (stem, None),
        None => {
            let (stem, suffix) = name.rsplit_once('_')?;
            if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            (stem, Some(suffix.parse().ok()?))
        }
    };
    // The digest has no hyphens, so the last group of the stem is the digest.
    let (uuid, digest) = stem.rsplit_once('-')?;
    if !is_uuid(uuid) || !is_digest(digest) {
        return None;
    }
    Some((uuid, digest, level))
}

/// Five hex groups of 8-4-4-4-12 characters, either case (§5.2).
fn is_uuid(value: &str) -> bool {
    let groups: Vec<&str> = value.split('-').collect();
    groups.len() == 5
        && groups.iter().zip([8, 4, 4, 4, 12]).all(|(group, length)| {
            group.len() == length && group.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

/// 32 lower-case hex characters: the digest is written verbatim, and its case
/// matters on a case-sensitive filesystem.
fn is_digest(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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

/// `true` when `name` is one of this database's tables.
fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [name],
        |row| row.get(0),
    )?;
    Ok(exists)
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
    fn the_tree_walk_counts_one_entry_per_pyramid() {
        let uuid = "1A2B3C4D-5E6F-4A5B-8C9D-000000000001";
        let digest = "deadbeefdeadbeefdeadbeefdeadbeef";
        let other = "9F8E7D6C-0000-4000-8000-000000000002";
        let other_digest = "00000000000000000000000000000000";
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path();
        let write = |path: PathBuf| {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"not read").unwrap();
        };

        // Two level files and a coexisting container are one pyramid.
        let shard = shard_dir(cache, uuid);
        write(shard.join(format!("{uuid}-{digest}_640")));
        write(shard.join(format!("{uuid}-{digest}_1616")));
        write(shard.join(format!("{uuid}-{digest}.lrprev")));
        // A second pyramid, container only.
        let other_shard = shard_dir(cache, other);
        write(other_shard.join(format!("{other}-{other_digest}.lrprev")));

        // None of these is a payload: the index and its sidecars sit in the
        // cache root, one leaf is in the wrong shard, and the rest do not parse.
        write(cache.join("previews.db"));
        write(cache.join("previews.db-wal"));
        write(cache.join("previews.db-shm"));
        write(
            cache
                .join("0")
                .join("0000")
                .join(format!("{uuid}-{digest}_640")),
        );
        write(shard.join(format!("{uuid}-{digest}")));
        write(shard.join("not-a-uuid-0123456789abcdef_1616"));
        write(shard.join(format!("{uuid}-{digest}_wide")));

        let sweep = sweep(cache);
        let pyramids = &sweep.pyramids;
        assert_eq!(pyramids.len(), 2, "one entry per pyramid: {pyramids:#?}");

        let level_files = &pyramids[0];
        assert_eq!(level_files.uuid, uuid);
        assert_eq!(level_files.digest, digest);
        assert_eq!(
            level_files
                .levels
                .iter()
                .map(|level| level.long_edge)
                .collect::<Vec<_>>(),
            vec![1616, 640],
            "path order, each carrying the long edge its leaf names"
        );
        assert!(level_files.container.is_some(), "the .lrprev is merged in");
        assert_eq!(
            level_files.largest_level(),
            Some(shard.join(format!("{uuid}-{digest}_1616")).as_path()),
            "the payload is the largest level, chosen from the name alone"
        );

        let container_only = &pyramids[1];
        assert_eq!(container_only.uuid, other);
        assert!(container_only.levels.is_empty());
        assert_eq!(container_only.largest_level(), None);
        assert!(container_only.container.is_some());
    }

    /// Write `bytes` at `relative`, creating the folders it needs.
    fn write(root: &Path, relative: &str, bytes: &[u8]) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
    }

    const UUID_A: &str = "1A2B3C4D-5E6F-4A5B-8C9D-000000000001";
    const DIGEST_A: &str = "deadbeefdeadbeefdeadbeefdeadbeef";

    #[test]
    fn a_missing_cache_root_holds_no_pyramids_and_no_layout() {
        let dir = tempfile::tempdir().unwrap();
        let sweep = sweep(&dir.path().join("absent"));
        assert!(sweep.pyramids.is_empty());
        assert!(sweep.unrecognised.is_empty());
        assert_eq!(sweep.layout(), Layout::None);
    }

    #[test]
    fn the_layout_word_is_decided_from_the_tree_alone() {
        // An index and nothing payload-shaped: `none`, not an error.
        let empty = tempfile::tempdir().unwrap();
        write(empty.path(), "previews.db", b"SQLite format 3\0");
        assert_eq!(sweep(empty.path()).layout(), Layout::None);

        // Payload-like files that match no shape: `unknown`, never `none`.
        let unknown = tempfile::tempdir().unwrap();
        write(
            unknown.path(),
            "Z/ZZZZ/not-a-uuid-0123456789abcdef_1616",
            &[0xFF, 0xD8, 0xFF, 0xD9],
        );
        assert_eq!(sweep(unknown.path()).layout(), Layout::Unknown);

        // A `.lrprev` at a payload address: `legacy`.
        let legacy = tempfile::tempdir().unwrap();
        write(
            legacy.path(),
            &format!("1/1A2B/{UUID_A}-{DIGEST_A}.lrprev"),
            b"not read",
        );
        assert_eq!(sweep(legacy.path()).layout(), Layout::Legacy);

        // One level file: `modern`.
        let modern = tempfile::tempdir().unwrap();
        write(
            modern.path(),
            &format!("1/1A2B/{UUID_A}-{DIGEST_A}_1616"),
            b"not read",
        );
        assert_eq!(sweep(modern.path()).layout(), Layout::Modern);

        // Both kinds, in one pyramid or two: `mixed` describes the cache.
        write(
            modern.path(),
            &format!("1/1A2B/{UUID_A}-{DIGEST_A}.lrprev"),
            b"not read",
        );
        assert_eq!(sweep(modern.path()).layout(), Layout::Mixed);
        assert_eq!(Layout::Mixed.as_str(), "mixed");
    }

    #[test]
    fn a_payload_like_file_of_no_shape_is_unrecognised() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path();
        // Three shapes the reader knows nothing about, each a real JPEG start.
        write(cache, &format!("1/1A2B/{UUID_A}-{DIGEST_A}"), &[0xFF, 0xD8]);
        write(
            cache,
            &format!("0/0000/{UUID_A}-{DIGEST_A}_1616"),
            &[0xFF, 0xD8],
        );
        write(cache, "Z/ZZZZ/not-a-uuid_1616", &[0xFF, 0xD8]);
        // Not payload-like: the index and its sidecars, and a name with no digits.
        write(cache, "previews.db", b"SQLite format 3\0");
        write(cache, "previews.db-wal", b"\0");
        write(
            cache,
            &format!("1/1A2B/{UUID_A}-{DIGEST_A}_wide"),
            b"not read",
        );
        // ... and one real level file, which is a pyramid and not unrecognised.
        write(
            cache,
            &format!("1/1A2B/{UUID_A}-{DIGEST_A}_1616"),
            b"not read",
        );

        let sweep = sweep(cache);
        assert_eq!(
            sweep.unrecognised,
            vec![
                cache
                    .join("0/0000")
                    .join(format!("{UUID_A}-{DIGEST_A}_1616")),
                cache.join("1/1A2B").join(format!("{UUID_A}-{DIGEST_A}")),
                cache.join("Z/ZZZZ/not-a-uuid_1616"),
            ],
            "in path order, and only the payload-like ones"
        );
        assert_eq!(sweep.pyramids.len(), 1, "the level file is a pyramid");
        assert_eq!(sweep.layout(), Layout::Modern);
    }

    #[test]
    fn tombstones_are_read_into_the_index_and_count_nothing() {
        let index = index_with(|conn| {
            conn.execute_batch(
                "INSERT INTO DeletedPyramid (uuid, digest, deletedTimeStamp)
                 VALUES ('AAAA', 'digest', 700000000.0);",
            )
            .unwrap();
        });

        assert_eq!(
            index
                .deleted
                .get(&("AAAA".to_string(), "digest".to_string()))
                .copied(),
            Some(700_000_000.0),
            "the timestamp is kept verbatim for a note to quote"
        );
        assert!(index.entries.is_empty(), "a tombstone is not a mapping");
    }

    #[test]
    fn a_store_without_the_tombstone_table_reports_no_deletions() {
        // Not every store version carries `DeletedPyramid`; losing the count is
        // not a reason to refuse a cache whose payloads are all readable.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("previews.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE Pyramid (uuid NOT NULL, digest NOT NULL, colorProfile NOT NULL,
               quality NOT NULL, croppedWidth NOT NULL, croppedHeight NOT NULL);
             CREATE TABLE PyramidLevel (uuid NOT NULL, digest NOT NULL, level NOT NULL,
               longDimension NOT NULL, lastAccess NOT NULL, width NOT NULL, height NOT NULL);
             CREATE TABLE ImageCacheEntry (imageId NOT NULL, uuid NOT NULL, digest NOT NULL,
               orientation);",
        )
        .unwrap();
        drop(conn);

        let index = PreviewIndex::open(&path).expect("a store without tombstones still opens");
        assert!(index.deleted.is_empty());
        assert!(index.entries.is_empty());
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
