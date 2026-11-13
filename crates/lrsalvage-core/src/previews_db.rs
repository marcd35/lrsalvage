//! `previews.db` — the authoritative map from catalog image to payload file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::types::ValueRef;

use crate::error::Result;
use crate::model::{CacheEntry, Pyramid, PyramidLevel};

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
        let first: String = entry.uuid.chars().take(1).collect();
        let first_four: String = entry.uuid.chars().take(4).collect();
        cache_root
            .join(first)
            .join(first_four)
            .join(format!("{}-{}.lrprev", entry.uuid, entry.digest))
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

    use rusqlite::Connection;

    use super::*;

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
