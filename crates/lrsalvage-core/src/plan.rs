//! Plan: turn a catalog plus a preview index into explicit decisions.
//!
//! Nothing here writes to disk. A plan is cheap to build, printable, and
//! machine-readable, which is what makes `scan` a safe first step.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use crate::catalog::Catalog;
use crate::error::Result;
use crate::model::{CatalogImage, Orientation};
use crate::previews_db::PreviewIndex;
use crate::{jpeg, lrprev};

/// One measured level: the chunk plus its decoded `(width, height)`.
type MeasuredLevel<'a> = (&'a lrprev::Chunk<'a>, (u32, u32));

/// Outcome of inspecting one catalog image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Payload exists and maps to a catalog image.
    Recoverable,
    /// `previews.db` has a row, but the payload file is gone.
    MissingPayload,
    /// Image is in the catalog but the cache has no entry for it.
    NoCacheEntry,
    /// Payload exists but cannot be mapped to a filename.
    Unmapped,
    /// Payload exists but its dimensions disagree with what was declared.
    Suspect,
    /// Payload is present but unreadable or truncated.
    Failed,
    /// The original file is still on disk, so there is nothing to salvage.
    OriginalPresent,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Recoverable => "recovered",
            Status::MissingPayload => "missing-payload",
            Status::NoCacheEntry => "no-cache-entry",
            Status::Unmapped => "unmapped",
            Status::Suspect => "suspect",
            Status::Failed => "failed",
            Status::OriginalPresent => "original-present",
        }
    }
}

/// How large the best available preview is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Long edge >= 4000 px: effectively the original resolution.
    Full,
    /// 1920–3999.
    Large,
    /// 1024–1919: Lightroom's typical "standard" preview.
    Standard,
    /// 640–1023.
    Small,
    /// Below 640: grid thumbnail.
    Thumbnail,
}

impl Tier {
    pub fn from_long_edge(px: u32) -> Self {
        match px {
            p if p >= 4000 => Tier::Full,
            p if p >= 1920 => Tier::Large,
            p if p >= 1024 => Tier::Standard,
            p if p >= 640 => Tier::Small,
            _ => Tier::Thumbnail,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Full => "full",
            Tier::Large => "large",
            Tier::Standard => "standard",
            Tier::Small => "small",
            Tier::Thumbnail => "thumbnail",
        }
    }
}

/// One decision about one image (or one level of one image).
#[derive(Debug, Clone)]
pub struct PlanItem {
    pub status: Status,
    pub image: Option<CatalogImage>,
    pub payload: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub color_profile: Option<String>,
    pub quality: Option<String>,
    pub orientation: Orientation,
    pub tier: Option<Tier>,
    /// Dimensions the cache *declared* for this pyramid, if it declared any.
    /// Used to decide whether the stored pixels are already rotated.
    pub declared: Option<(u32, u32)>,
    /// False when the level falls below the configured floor.
    pub selected: bool,
    pub note: Option<String>,
}

impl PlanItem {
    pub fn long_edge(&self) -> u32 {
        self.width.unwrap_or(0).max(self.height.unwrap_or(0))
    }
}

/// Knobs that change what ends up in a plan.
#[derive(Debug, Clone)]
pub struct Options {
    /// Ignore levels whose long edge is below this. `0` disables the floor.
    pub min_long_edge: u32,
    /// Emit every level instead of only the best one.
    pub all_levels: bool,
    /// Also consider images whose original is still on disk. Off by default:
    /// they are in no need of salvage, and checking every path costs a stat.
    pub include_present: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            min_long_edge: 640,
            all_levels: false,
            include_present: false,
        }
    }
}

/// Totals for one plan.
///
/// Status counters count **images**, not plan items: `--all-levels` emits one
/// item per level but must not inflate how many photos were recovered.
#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub images: u64,
    pub recoverable: u64,
    pub missing_payload: u64,
    pub no_cache_entry: u64,
    pub unmapped: u64,
    pub suspect: u64,
    pub failed: u64,
    /// Images whose original file is still on disk, and so are not salvage
    /// targets. Always zero when `include_present` is set.
    pub original_present: u64,
    /// Level items below the configured floor.
    pub below_floor: u64,
    pub bytes: u64,
    /// Every level item in the plan, including the extra ones `--all_levels`
    /// emits for one image.
    pub levels: u64,
    /// Plan items the extractor will write.
    pub selected_items: u64,
    /// Distinct images among those items.
    pub selected_images: u64,
    pub tiers: HashMap<&'static str, u64>,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub items: Vec<PlanItem>,
    pub stats: Stats,
}

impl Plan {
    /// Build a plan for every image in `catalog`, then sweep `cache_root` for
    /// payloads that no image claims.
    pub fn build(
        catalog: &Catalog,
        index: &PreviewIndex,
        cache_root: &Path,
        output_root: &Path,
        options: &Options,
    ) -> Result<Self> {
        let images = catalog.images()?;
        let mut items = Vec::new();
        let mut stats = Stats {
            images: images.len() as u64,
            ..Stats::default()
        };
        let mut used: HashMap<String, u32> = HashMap::new();

        // Seed the claim set from rows that resolve to a real image only. A
        // stale row (`imageId` no longer in `Adobe_images`) must not suppress
        // the orphan sweep: its payload is exactly what salvage is for.
        let image_ids: HashSet<i64> = images.iter().map(|image| image.id_local).collect();
        let mut claimed: HashSet<String> = HashSet::new();
        for entry in &index.all_entries {
            if image_ids.contains(&entry.image_id) {
                claimed.insert(format!("{}-{}", entry.uuid, entry.digest));
            }
        }

        for image in images {
            let mut image = image;
            // Metadata is fetched here, once per image, so both the manifest
            // and the EXIF writer read the same facts.
            if image.xmp.is_none() {
                image.xmp = catalog.xmp(image.id_local).ok().flatten();
            }

            // An image whose original is still on disk needs no salvage. The
            // stat is skipped entirely when `include_present` is set, so that
            // opt-in run costs nothing per image.
            if !options.include_present && image.original_path.is_file() {
                stats.original_present += 1;
                let orientation = image.orientation;
                items.push(PlanItem {
                    status: Status::OriginalPresent,
                    image: Some(image),
                    payload: None,
                    output: None,
                    width: None,
                    height: None,
                    color_profile: None,
                    quality: None,
                    orientation,
                    tier: None,
                    declared: None,
                    selected: false,
                    note: Some("original still on disk".into()),
                });
                continue;
            }

            let Some(entry) = index.entries.get(&image.id_local) else {
                stats.no_cache_entry += 1;
                items.push(PlanItem {
                    status: Status::NoCacheEntry,
                    image: Some(image),
                    payload: None,
                    output: None,
                    width: None,
                    height: None,
                    color_profile: None,
                    quality: None,
                    orientation: Orientation::Unknown,
                    tier: None,
                    declared: None,
                    selected: false,
                    note: Some("no previews.db row for this image".into()),
                });
                continue;
            };

            claimed.insert(format!("{}-{}", entry.uuid, entry.digest));
            let payload = index.payload_path(cache_root, entry);
            let pyramid = index.pyramids.get(&entry.uuid);
            let orientation = image.orientation;

            if !payload.exists() {
                stats.missing_payload += 1;
                items.push(PlanItem {
                    status: Status::MissingPayload,
                    image: Some(image),
                    payload: Some(payload),
                    output: None,
                    width: None,
                    height: None,
                    color_profile: pyramid.map(|p| p.color_profile.clone()),
                    quality: pyramid.map(|p| p.quality.clone()),
                    orientation,
                    tier: None,
                    declared: None,
                    selected: false,
                    note: Some("previews.db row points at a file that is gone".into()),
                });
                continue;
            }

            let bytes = match std::fs::read(&payload) {
                Ok(b) => b,
                Err(e) => {
                    stats.failed += 1;
                    items.push(PlanItem {
                        status: Status::Failed,
                        image: Some(image),
                        payload: Some(payload),
                        output: None,
                        width: None,
                        height: None,
                        color_profile: pyramid.map(|p| p.color_profile.clone()),
                        quality: pyramid.map(|p| p.quality.clone()),
                        orientation,
                        tier: None,
                        declared: None,
                        selected: false,
                        note: Some(format!("unreadable payload: {e}")),
                    });
                    continue;
                }
            };

            let parsed = match lrprev::Lrprev::parse(&bytes) {
                Ok(p) => p,
                Err(e) => {
                    stats.failed += 1;
                    items.push(PlanItem {
                        status: Status::Failed,
                        image: Some(image),
                        payload: Some(payload),
                        output: None,
                        width: None,
                        height: None,
                        color_profile: pyramid.map(|p| p.color_profile.clone()),
                        quality: pyramid.map(|p| p.quality.clone()),
                        orientation,
                        tier: None,
                        declared: None,
                        selected: false,
                        note: Some(format!("not a usable .lrprev: {e}")),
                    });
                    continue;
                }
            };

            // Measure every level once; the declared header sizes are hints
            // only, the decoded SOF dimensions are the truth.
            let mut levels: Vec<MeasuredLevel<'_>> = parsed
                .levels()
                .into_iter()
                .filter_map(|c| jpeg::dimensions(c.data).ok().map(|d| (c, d)))
                .collect();
            levels.sort_by_key(|(_, (w, h))| (*w as u64) * (*h as u64));

            if levels.is_empty() {
                stats.failed += 1;
                let note = match parsed.header().and_then(|h| std::str::from_utf8(h).ok()) {
                    Some(_) => "no level contains a readable JPEG frame".to_string(),
                    None => "no readable levels and no header".to_string(),
                };
                items.push(PlanItem {
                    status: Status::Failed,
                    image: Some(image),
                    payload: Some(payload),
                    output: None,
                    width: None,
                    height: None,
                    color_profile: pyramid.map(|p| p.color_profile.clone()),
                    quality: pyramid.map(|p| p.quality.clone()),
                    orientation,
                    tier: None,
                    declared: None,
                    selected: false,
                    note: Some(note),
                });
                continue;
            }

            let best_index = levels.len() - 1;
            let chosen: Vec<(usize, MeasuredLevel<'_>)> = if options.all_levels {
                levels.into_iter().enumerate().collect()
            } else {
                let (chunk, dims) = levels.swap_remove(best_index);
                vec![(best_index, (chunk, dims))]
            };

            // One status per image: the best level decides, so `--all-levels`
            // cannot make one photo look like several recoveries.
            let mut image_status: Option<Status> = None;

            for (idx, (chunk, (w, h))) in chosen {
                let long_edge = w.max(h);
                let tier = Tier::from_long_edge(long_edge);
                let selected = long_edge >= options.min_long_edge;
                if !selected {
                    stats.below_floor += 1;
                }

                let mut note: Option<String> = None;
                let mut status = Status::Recoverable;

                // Cross-check the decoded frame against the catalogue's own
                // declaration. A mismatch means a stale or foreign payload.
                if let Some(level) = index.best_level(&entry.uuid) {
                    if idx == best_index
                        && (level.width != w || level.height != h)
                        && (level.width != h || level.height != w)
                    {
                        status = Status::Suspect;
                        note = Some(format!(
                            "decoded {w}x{h} but previews.db declares {}x{}; kept and flagged \
                             because the payload is not the size the catalog expected",
                            level.width, level.height
                        ));
                    }
                }

                if !jpeg::is_complete(chunk.data) {
                    status = Status::Failed;
                    note = Some("level does not end with an EOI marker".into());
                }

                // Only the extra levels `--all-levels` adds carry a size
                // suffix; the best level keeps the plain `<stem>.jpg` name.
                let relative = if options.all_levels && idx != best_index {
                    image.relative_output_path_with_suffix(&format!(" - {w}x{h}"))
                } else {
                    image.relative_output_path()
                };

                let mut output = None;
                match confined_output(output_root, &relative) {
                    Ok(path) => output = Some(unique_path(&mut used, path)),
                    Err(escape) => {
                        status = Status::Failed;
                        note = Some(escape);
                    }
                }

                if idx == best_index {
                    image_status = Some(status);
                }
                stats.levels += 1;

                if selected && matches!(status, Status::Recoverable | Status::Suspect) {
                    stats.bytes += chunk.data.len() as u64;
                }
                *stats.tiers.entry(tier.as_str()).or_insert(0) += 1;

                items.push(PlanItem {
                    status,
                    image: Some(image.clone()),
                    payload: Some(payload.clone()),
                    output,
                    width: Some(w),
                    height: Some(h),
                    color_profile: pyramid.map(|p| p.color_profile.clone()),
                    quality: pyramid.map(|p| p.quality.clone()),
                    orientation,
                    tier: Some(tier),
                    declared: pyramid.map(|p| (p.cropped_width, p.cropped_height)),
                    selected: matches!(status, Status::Recoverable | Status::Suspect) && selected,
                    note,
                });
            }

            match image_status {
                Some(Status::Recoverable) => stats.recoverable += 1,
                Some(Status::Suspect) => stats.suspect += 1,
                Some(Status::Failed) => stats.failed += 1,
                _ => {}
            }
        }

        // Payloads with no cache entry: recoverable, but unmappable.
        for name in payload_names(cache_root) {
            let stem = name.rsplit('/').next().unwrap_or(&name);
            if claimed.contains(stem) {
                continue;
            }
            let payload = cache_root.join(&name).with_extension("lrprev");
            items.push(PlanItem {
                status: Status::Unmapped,
                image: None,
                payload: Some(payload.clone()),
                // Nothing is extracted, so there is no output path to report;
                // the payload's own path is the useful fact.
                output: None,
                width: None,
                height: None,
                color_profile: None,
                quality: None,
                orientation: Orientation::Unknown,
                tier: None,
                declared: None,
                selected: false,
                note: Some(format!(
                    "payload exists but previews.db has no entry for it: {}",
                    payload.to_string_lossy().replace('\\', "/")
                )),
            });
            stats.unmapped += 1;
        }

        stats.selected_items = items.iter().filter(|item| item.selected).count() as u64;
        stats.selected_images = distinct_selected_images(&items) as u64;

        Ok(Plan { items, stats })
    }

    /// Every item the extractor should write.
    pub fn selected(&self) -> impl Iterator<Item = &PlanItem> {
        self.items.iter().filter(|i| i.selected)
    }

    /// How many distinct catalog images the selected items belong to.
    ///
    /// `--all-levels` can select several items for one image; this is the
    /// count a human means by "images recovered".
    pub fn selected_images(&self) -> usize {
        distinct_selected_images(&self.items)
    }
}

fn distinct_selected_images(items: &[PlanItem]) -> usize {
    items
        .iter()
        .filter(|item| item.selected)
        .filter_map(|item| item.image.as_ref().map(|image| image.id_local))
        .collect::<HashSet<i64>>()
        .len()
}

/// Append ` (2)`, ` (3)` … when two images resolve to the same output path.
///
/// The key is case-folded on Windows, where `IMG.jpg` and `img.jpg` are one
/// file and the second copy would otherwise overwrite the first.
fn unique_path(used: &mut HashMap<String, u32>, path: PathBuf) -> PathBuf {
    let key = if cfg!(windows) {
        path.to_string_lossy().to_lowercase()
    } else {
        path.to_string_lossy().into_owned()
    };
    let count = used.entry(key).or_insert(0);
    *count += 1;
    if *count == 1 {
        return path;
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!("{stem} ({}).jpg", *count))
}

/// `<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>` for every `.lrprev` on disk.
fn payload_names(cache_root: &Path) -> Vec<String> {
    use walkdir::WalkDir;

    WalkDir::new(cache_root)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "lrprev"))
        .filter_map(|e| {
            e.path()
                .strip_prefix(cache_root)
                .ok()
                .map(|rel| rel.with_extension("").to_string_lossy().replace('\\', "/"))
        })
        .collect()
}

/// Join `relative` onto `output_root`, refusing anything that lands outside it.
///
/// [`CatalogImage::relative_output_path`] already sanitises every component, so
/// this is the second line of defence: if that ever regresses, the plan fails
/// the item instead of writing outside the destination. `starts_with` alone is
/// not enough — `out/../escaped` still starts with `out` — so the relative path
/// must also be made of ordinary components only.
fn confined_output(output_root: &Path, relative: &Path) -> std::result::Result<PathBuf, String> {
    let escape = || {
        format!(
            "refusing to write outside the output root: {} escapes {}",
            relative.display(),
            output_root.display()
        )
    };
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(escape());
    }
    let joined = output_root.join(relative);
    if !joined.starts_with(output_root) {
        return Err(escape());
    }
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Component, Path, PathBuf};

    use rusqlite::{Connection, params};
    use tempfile::TempDir;

    use crate::catalog::Catalog;
    use crate::model::{CacheEntry, Pyramid, PyramidLevel};
    use crate::previews_db::PreviewIndex;
    use crate::snapshot::CatalogSnapshot;

    use super::*;

    const CATALOG_SCHEMA: &str = r#"
        CREATE TABLE Adobe_images (id_local INTEGER PRIMARY KEY, copyName TEXT,
          orientation TEXT, fileFormat TEXT, rootFile INTEGER);
        CREATE TABLE AgLibraryFile (id_local INTEGER PRIMARY KEY, folder INTEGER,
          baseName TEXT, extension TEXT);
        CREATE TABLE AgLibraryFolder (id_local INTEGER PRIMARY KEY, pathFromRoot TEXT,
          rootFolder INTEGER);
        CREATE TABLE AgLibraryRootFolder (id_local INTEGER PRIMARY KEY, absolutePath TEXT);
    "#;

    /// One catalog image plus the cache rows that describe its pyramid.
    struct ImageRow {
        id: i64,
        base_name: String,
        copy_name: Option<String>,
        path_from_root: String,
        /// Last segment of this image's root folder, and therefore the first
        /// directory of its output tree.
        root_label: String,
        /// Write a file at the reconstructed original path.
        original_on_disk: bool,
        /// `false`: no `AgLibraryFile` row at all (an image whose file row was
        /// deleted), which leaves the base name empty.
        has_file: bool,
        cache: Option<CacheRow>,
    }

    struct CacheRow {
        uuid: String,
        digest: String,
        /// Levels actually written to the payload; `None` means the row points
        /// at a file that is not on disk.
        levels: Option<Vec<(u16, u16)>>,
        /// Dimensions `PyramidLevel` claims about this pyramid.
        declared: Vec<(u16, u16)>,
    }

    impl ImageRow {
        fn new(id: i64, base_name: &str) -> Self {
            ImageRow {
                id,
                base_name: base_name.to_string(),
                copy_name: None,
                path_from_root: String::new(),
                root_label: "2015".to_string(),
                original_on_disk: false,
                has_file: true,
                cache: None,
            }
        }

        fn without_file(mut self) -> Self {
            self.has_file = false;
            self
        }

        fn traversal(mut self, path_from_root: &str) -> Self {
            self.path_from_root = path_from_root.to_string();
            self
        }

        fn original_on_disk(mut self) -> Self {
            self.original_on_disk = true;
            self
        }

        fn copy(mut self, copy_name: &str) -> Self {
            self.copy_name = Some(copy_name.to_string());
            self
        }

        fn missing_payload(mut self, uuid: &str, declared: &[(u16, u16)]) -> Self {
            self.cache = Some(CacheRow {
                uuid: uuid.to_string(),
                digest: digest_of(uuid),
                levels: None,
                declared: declared.to_vec(),
            });
            self
        }

        fn cache(mut self, uuid: &str, levels: &[(u16, u16)], declared: &[(u16, u16)]) -> Self {
            self.cache = Some(CacheRow {
                uuid: uuid.to_string(),
                digest: digest_of(uuid),
                levels: Some(levels.to_vec()),
                declared: declared.to_vec(),
            });
            self
        }
    }

    fn digest_of(uuid: &str) -> String {
        uuid.chars()
            .filter(char::is_ascii_hexdigit)
            .take(8)
            .collect::<String>()
            .to_ascii_lowercase()
    }

    /// A catalog, a preview cache and a destination, all inside one temp dir.
    struct Fixture {
        /// Keeps the temp tree alive for the life of the fixture.
        _dir: TempDir,
        _snapshot: CatalogSnapshot,
        catalog: Catalog,
        rows: Vec<ImageRow>,
        cache: PathBuf,
        output: PathBuf,
    }
    impl Fixture {
        fn new(rows: Vec<ImageRow>) -> Self {
            let dir = tempfile::tempdir().expect("temp dir");
            let cache = dir.path().join("cache");
            let output = dir.path().join("out");
            fs::create_dir_all(&cache).expect("create cache");
            fs::create_dir_all(&output).expect("create output root");

            let catalog_dir = dir.path().join("catalog");
            fs::create_dir_all(&catalog_dir).expect("create catalog dir");
            let catalog_path = catalog_dir.join("test.lrcat");
            let conn = Connection::open(&catalog_path).expect("create catalog");
            conn.execute_batch(CATALOG_SCHEMA).expect("catalog schema");
            for (index, row) in rows.iter().enumerate() {
                let id = index as i64 + 1;
                conn.execute(
                    "INSERT INTO AgLibraryRootFolder (id_local, absolutePath) VALUES (?1, ?2)",
                    params![
                        id,
                        format!(
                            "{}/",
                            dir.path()
                                .join("originals")
                                .join(&row.root_label)
                                .to_string_lossy()
                                .replace('\\', "/")
                        )
                    ],
                )
                .expect("root folder");
                if row.has_file {
                    conn.execute(
                        "INSERT INTO AgLibraryFolder (id_local, pathFromRoot, rootFolder) \
                         VALUES (?1, ?2, ?1)",
                        params![id, row.path_from_root],
                    )
                    .expect("folder");
                    conn.execute(
                        "INSERT INTO AgLibraryFile (id_local, folder, baseName, extension) \
                         VALUES (?1, ?1, ?2, 'CR2')",
                        params![id, row.base_name],
                    )
                    .expect("file row");
                }
                let root_file = if row.has_file {
                    Some(id)
                } else {
                    None
                };
                conn.execute(
                    "INSERT INTO Adobe_images \
                     (id_local, copyName, orientation, fileFormat, rootFile) \
                     VALUES (?1, ?2, 'AB', 'CR2', ?3)",
                    params![row.id, row.copy_name, root_file],
                )
                .expect("image row");
            }
            drop(conn);

            let (snapshot, catalog) = Catalog::open(&catalog_path).expect("open catalog");
            for image in catalog.images().expect("images") {
                if rows
                    .iter()
                    .any(|row| row.id == image.id_local && row.original_on_disk)
                {
                    if let Some(parent) = image.original_path.parent() {
                        fs::create_dir_all(parent).expect("create original dir");
                    }
                    fs::write(&image.original_path, b"not a preview").expect("write original");
                }
            }

            let fixture = Fixture {
                _dir: dir,
                _snapshot: snapshot,
                catalog,
                rows,
                cache,
                output,
            };
            for row in &fixture.rows {
                if let Some(cache) = &row.cache {
                    if let Some(levels) = &cache.levels {
                        fixture.write_payload(&cache.uuid, &cache.digest, levels);
                    }
                }
            }
            fixture
        }

        fn path(&self) -> &Path {
            self._dir.path()
        }

        /// The index `previews.db` would produce for this fixture.
        fn index(&self) -> PreviewIndex {
            let mut index = PreviewIndex::default();
            for row in &self.rows {
                let Some(cache) = &row.cache else { continue };
                let entry = CacheEntry {
                    image_id: row.id,
                    uuid: cache.uuid.clone(),
                    digest: cache.digest.clone(),
                    orientation: Some("AB".to_string()),
                };
                index.entries.insert(row.id, entry.clone());
                index.all_entries.push(entry);
                index.pyramids.insert(
                    cache.uuid.clone(),
                    Pyramid {
                        uuid: cache.uuid.clone(),
                        digest: cache.digest.clone(),
                        color_profile: "AdobeRGB".to_string(),
                        quality: "standard".to_string(),
                        cropped_width: u32::from(cache.declared[0].0),
                        cropped_height: u32::from(cache.declared[0].1),
                    },
                );
                index.levels.insert(
                    cache.uuid.clone(),
                    cache
                        .declared
                        .iter()
                        .enumerate()
                        .map(|(level, (width, height))| PyramidLevel {
                            uuid: cache.uuid.clone(),
                            level: level as u32,
                            long_dimension: u32::from(*width).max(u32::from(*height)),
                            width: u32::from(*width),
                            height: u32::from(*height),
                        })
                        .collect(),
                );
            }
            index
        }

        fn plan(&self, options: &Options) -> Plan {
            let index = self.index();
            self.build(&index, options)
        }

        fn build(&self, index: &PreviewIndex, options: &Options) -> Plan {
            Plan::build(&self.catalog, index, &self.cache, &self.output, options)
                .expect("build plan")
        }

        /// `<cache>/<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>.lrprev`
        fn write_payload(&self, uuid: &str, digest: &str, levels: &[(u16, u16)]) -> PathBuf {
            let first: String = uuid.chars().take(1).collect();
            let first_four: String = uuid.chars().take(4).collect();
            let dir = self.cache.join(first).join(first_four);
            fs::create_dir_all(&dir).expect("create payload dir");
            let path = dir.join(format!("{uuid}-{digest}.lrprev"));

            let mut bytes = chunk("header", b"pyramid = { }", 0);
            for (index, (width, height)) in levels.iter().enumerate() {
                bytes.extend(chunk(
                    &format!("level_{}", index + 1),
                    &jpeg(*width, *height),
                    0,
                ));
            }
            fs::write(&path, bytes).expect("write payload");
            path
        }
    }

    fn item(plan: &Plan, image_id: i64) -> &PlanItem {
        plan.items
            .iter()
            .find(|item| {
                item.image
                    .as_ref()
                    .is_some_and(|image| image.id_local == image_id)
            })
            .unwrap_or_else(|| panic!("no plan item for image {image_id}"))
    }

    fn items_for(plan: &Plan, image_id: i64) -> Vec<&PlanItem> {
        plan.items
            .iter()
            .filter(|item| {
                item.image
                    .as_ref()
                    .is_some_and(|image| image.id_local == image_id)
            })
            .collect()
    }

    fn slashed(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    /// One `AgHg` chunk: magic, header length, version, kind, lengths, name,
    /// payload, padding.
    fn chunk(name: &str, payload: &[u8], padding: usize) -> Vec<u8> {
        let header_len = 24 + name.len();
        let mut bytes = Vec::with_capacity(header_len + payload.len() + padding);
        bytes.extend_from_slice(b"AgHg");
        bytes.extend_from_slice(&u16::try_from(header_len).unwrap().to_be_bytes());
        bytes.push(0); // version
        bytes.push(0); // kind
        bytes.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        bytes.extend_from_slice(&(padding as u64).to_be_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(payload);
        bytes.resize(bytes.len() + padding, 0xEE);
        bytes
    }

    /// A structurally valid JPEG at `width` x `height`.
    fn jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8];
        bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&[0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        bytes.extend_from_slice(&[
            0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00,
        ]);
        bytes.extend_from_slice(&[0x00, 0xFF, 0xD9]);
        bytes
    }

    #[test]
    fn a_relative_output_path_that_escapes_the_output_root_is_refused() {
        let root = Path::new("out");
        assert_eq!(
            confined_output(root, Path::new("2015/IMG_0001.jpg")),
            Ok(PathBuf::from("out/2015/IMG_0001.jpg"))
        );
        assert!(confined_output(root, Path::new("../escaped/IMG_0001.jpg")).is_err());
        if cfg!(windows) {
            // Joining an absolute path replaces the root outright.
            assert!(confined_output(root, Path::new("C:/escaped/IMG_0001.jpg")).is_err());
        }
    }

    #[test]
    fn a_hostile_path_from_root_cannot_write_outside_the_output_root() {
        let fixture = Fixture::new(vec![
            ImageRow::new(1, "IMG_0001")
                .traversal("../../../escaped/")
                .cache("AAAA0001-1111-4111-8111-000000000001", &[(1616, 1077)], &[(1616, 1077)]),
        ]);
        let plan = fixture.plan(&Options::default());

        let output = item(&plan, 1).output.as_ref().expect("output path");
        assert!(
            output.starts_with(&fixture.output),
            "{} must stay under {}",
            output.display(),
            fixture.output.display()
        );
        assert_eq!(slashed(output), slashed(&fixture.output.join("2015/escaped/IMG_0001.jpg")));
        assert!(
            !output
                .components()
                .any(|component| matches!(component, Component::ParentDir)),
            "{output:?}"
        );
    }

    #[test]
    fn an_image_whose_original_is_still_on_disk_is_not_a_salvage_target() {
        let fixture = Fixture::new(vec![
            ImageRow::new(1, "IMG_0001")
                .original_on_disk()
                .cache("AAAA0001-1111-4111-8111-000000000001", &[(1616, 1077)], &[(1616, 1077)]),
            ImageRow::new(2, "IMG_0002")
                .cache("AAAA0002-1111-4111-8111-000000000002", &[(1616, 1077)], &[(1616, 1077)]),
        ]);
        // Premise: image 1's original really is on disk.
        let image = item(&fixture.plan(&Options::default()), 1)
            .image
            .clone()
            .expect("image data");
        assert!(image.original_path.is_file(), "{}", image.original_path.display());

        let plan = fixture.plan(&Options::default());
        let present = item(&plan, 1);
        assert_eq!(present.status, Status::OriginalPresent);
        assert_eq!(present.status.as_str(), "original-present");
        assert!(!present.selected, "an image with its original is not salvaged");
        assert!(present.output.is_none());
        assert!(
            present
                .note
                .as_deref()
                .is_some_and(|note| note.contains("original still on disk")),
            "{:?}",
            present.note
        );

        assert_eq!(plan.stats.original_present, 1);
        assert_eq!(plan.stats.recoverable, 1, "image 2 is the salvage target");
        assert_eq!(plan.selected().count(), 1);
        assert_eq!(item(&plan, 2).status, Status::Recoverable);
    }

    #[test]
    fn include_present_salvages_an_image_whose_original_is_on_disk() {
        let fixture = Fixture::new(vec![
            ImageRow::new(1, "IMG_0001")
                .original_on_disk()
                .cache("AAAA0001-1111-4111-8111-000000000001", &[(1616, 1077)], &[(1616, 1077)]),
        ]);
        let plan = fixture.plan(&Options {
            include_present: true,
            ..Options::default()
        });

        let item = item(&plan, 1);
        assert_eq!(item.status, Status::Recoverable);
        assert!(item.selected);
        assert_eq!(plan.stats.original_present, 0);
        assert_eq!(plan.stats.recoverable, 1);
    }

    #[test]
    fn a_cache_row_for_a_deleted_image_does_not_hide_its_payload() {
        let fixture = Fixture::new(vec![
            ImageRow::new(1, "IMG_0001")
                .cache("AAAA0001-1111-4111-8111-000000000001", &[(1616, 1077)], &[(1616, 1077)]),
        ]);
        let mut index = fixture.index();
        // A stale row: `imageId` 999 has no row in Adobe_images any more.
        let stale = CacheEntry {
            image_id: 999,
            uuid: "ZZZZ0999-1111-4111-8111-000000000999".to_string(),
            digest: "0bad".to_string(),
            orientation: None,
        };
        fixture.write_payload(&stale.uuid, &stale.digest, &[(1024, 683)]);
        index.all_entries.push(stale.clone());
        index.entries.insert(999, stale);

        let plan = fixture.build(&index, &Options::default());
        let unmapped: Vec<&PlanItem> = plan
            .items
            .iter()
            .filter(|item| item.status == Status::Unmapped)
            .collect();
        assert_eq!(
            unmapped.len(),
            1,
            "the dead row's payload is an orphan, not a claim: {:#?}",
            plan.items
        );
        assert!(
            unmapped[0]
                .payload
                .as_ref()
                .is_some_and(|payload| payload.is_file()),
            "{:?}",
            unmapped[0].payload
        );
        assert_eq!(plan.stats.unmapped, 1);
        assert_eq!(plan.stats.recoverable, 1);
    }

    #[test]
    fn all_levels_counts_images_once_and_names_only_the_extra_levels() {
        let fixture = Fixture::new(vec![
            ImageRow::new(1, "IMG_0001").cache(
                "AAAA0001-1111-4111-8111-000000000001",
                &[(1024, 683), (1616, 1077)],
                &[(1616, 1077)],
            ),
        ]);
        let plan = fixture.plan(&Options {
            all_levels: true,
            ..Options::default()
        });

        let items = items_for(&plan, 1);
        assert_eq!(items.len(), 2, "both levels are planned");
        let mut names: Vec<String> = items
            .iter()
            .map(|item| {
                item.output
                    .as_ref()
                    .expect("output path")
                    .file_name()
                    .expect("file name")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec!["IMG_0001 - 1024x683.jpg", "IMG_0001.jpg"],
            "only the extra level carries a size suffix"
        );

        assert_eq!(plan.stats.recoverable, 1, "one image, not two levels");
        assert_eq!(plan.stats.levels, 2);
        assert_eq!(plan.stats.selected_items, 2);
        assert_eq!(plan.stats.selected_images, 1);
        assert_eq!(plan.selected_images(), 1);
        assert_eq!(plan.stats.below_floor, 0);
    }

    #[test]
    fn stats_count_images_rather_than_levels() {
        let fixture = Fixture::new(vec![
            ImageRow::new(1, "IMG_0001").cache(
                "AAAA0001-1111-4111-8111-000000000001",
                &[(1024, 683), (1616, 1077)],
                &[(1616, 1077)],
            ),
            // The largest level disagrees with previews.db: the image is suspect.
            ImageRow::new(2, "IMG_0002").cache(
                "AAAA0002-1111-4111-8111-000000000002",
                &[(800, 600), (1024, 683)],
                &[(1616, 1077)],
            ),
        ]);
        let plan = fixture.plan(&Options {
            all_levels: true,
            ..Options::default()
        });

        let all: Vec<&PlanItem> = plan.items.iter().collect();
        assert_eq!(all.len(), 4, "two readable levels per image");
        assert_eq!(plan.stats.levels, 4);
        assert_eq!(plan.stats.recoverable, 1, "image 1");
        assert_eq!(plan.stats.suspect, 1, "image 2, counted once");
        assert_eq!(plan.stats.failed, 0);
        assert_eq!(plan.stats.selected_items, 4);
        assert_eq!(plan.stats.selected_images, 2);
    }

    #[test]
    fn an_unmapped_payload_has_no_output_path_but_names_its_payload() {
        let fixture = Fixture::new(vec![
            ImageRow::new(1, "IMG_0001")
                .cache("AAAA0001-1111-4111-8111-000000000001", &[(1616, 1077)], &[(1616, 1077)]),
        ]);
        fixture.write_payload("ZZZZ0999-1111-4111-8111-000000000999", "0bad", &[(1024, 683)]);

        let plan = fixture.plan(&Options::default());
        let unmapped: Vec<&PlanItem> = plan
            .items
            .iter()
            .filter(|item| item.status == Status::Unmapped)
            .collect();
        assert_eq!(unmapped.len(), 1);
        assert!(
            unmapped[0].output.is_none(),
            "an unmapped payload is listed, never written: {:?}",
            unmapped[0].output
        );
        let note = unmapped[0].note.as_deref().expect("note");
        assert!(note.contains("ZZZZ0999"), "note names the payload: {note}");
        assert!(note.contains("0bad"), "note names the payload: {note}");
    }

    #[test]
    fn a_suspect_payload_is_still_selected_and_the_note_explains_why() {
        let fixture = Fixture::new(vec![
            ImageRow::new(1, "IMG_0001")
                .cache("AAAA0001-1111-4111-8111-000000000001", &[(800, 600)], &[(1616, 1077)]),
        ]);
        let plan = fixture.plan(&Options::default());

        let suspect = item(&plan, 1);
        assert_eq!(suspect.status, Status::Suspect);
        assert!(suspect.selected, "a suspect payload is still a real photo");
        let note = suspect.note.as_deref().expect("note");
        assert!(
            note.contains("800x600") && note.contains("1616x1077"),
            "note names both sizes: {note}"
        );
        assert_eq!(plan.stats.suspect, 1);
        assert_eq!(plan.stats.recoverable, 0);
    }

    #[test]
    fn unique_path_treats_case_variants_as_one_file_on_windows() {
        let mut used: HashMap<String, u32> = HashMap::new();
        assert_eq!(
            unique_path(&mut used, PathBuf::from("out/IMG_0001.jpg")),
            PathBuf::from("out/IMG_0001.jpg")
        );
        let second = unique_path(&mut used, PathBuf::from("out/img_0001.jpg"));
        if cfg!(windows) {
            assert_eq!(
                second,
                PathBuf::from("out/img_0001 (2).jpg"),
                "case-only differences are one file on Windows"
            );
        } else {
            assert_eq!(second, PathBuf::from("out/img_0001.jpg"));
        }
    }

    #[test]
    fn an_image_without_a_file_row_gets_an_id_based_output_name() {
        let fixture = Fixture::new(vec![
            ImageRow::new(9, "")
                .without_file()
                .cache("AAAA0009-1111-4111-8111-000000000009", &[(1616, 1077)], &[(1616, 1077)]),
        ]);
        let plan = fixture.plan(&Options::default());

        let planned = item(&plan, 9);
        assert_eq!(planned.status, Status::Recoverable);
        let output = planned.output.as_ref().expect("output path");
        assert_eq!(
            slashed(output),
            slashed(&fixture.output.join("image-9.jpg")),
            "no file row means no base name and no folder, and `.jpg` alone is not a file name"
        );
        assert!(planned.selected);
    }

    #[test]
    fn a_virtual_copy_keeps_its_own_name_and_its_own_output_path() {
        let fixture = Fixture::new(vec![
            ImageRow::new(1, "IMG_0001")
                .cache("AAAA0001-1111-4111-8111-000000000001", &[(1616, 1077)], &[(1616, 1077)]),
            ImageRow::new(2, "IMG_0001")
                .copy("Black and white")
                .cache("AAAA0002-1111-4111-8111-000000000002", &[(1616, 1077)], &[(1616, 1077)]),
        ]);
        let plan = fixture.plan(&Options::default());

        let master = item(&plan, 1);
        let copy = item(&plan, 2);
        assert_eq!(master.image.as_ref().unwrap().output_stem(), "IMG_0001");
        assert_eq!(
            copy.image.as_ref().unwrap().output_stem(),
            "IMG_0001 - Black and white"
        );
        assert_ne!(master.output, copy.output);
        assert_eq!(
            copy.output.as_ref().unwrap().file_name().unwrap(),
            "IMG_0001 - Black and white.jpg"
        );
    }

    #[test]
    fn a_cache_row_whose_payload_is_gone_is_missing_payload() {
        let fixture = Fixture::new(vec![
            ImageRow::new(1, "IMG_0001")
                .missing_payload("AAAA0001-1111-4111-8111-000000000001", &[(1616, 1077)]),
        ]);
        let plan = fixture.plan(&Options::default());

        let missing = item(&plan, 1);
        assert_eq!(missing.status, Status::MissingPayload);
        assert!(!missing.selected);
        assert!(missing.output.is_none());
        let payload = missing.payload.as_ref().expect("payload path");
        assert!(
            payload.starts_with(fixture.path().join("cache")),
            "{}",
            payload.display()
        );
        assert!(!payload.exists());
        assert_eq!(plan.stats.missing_payload, 1);
        assert_eq!(plan.stats.recoverable, 0);
    }

    #[test]
    fn two_images_whose_names_differ_only_in_case_do_not_collide() {
        let fixture = Fixture::new(vec![
            ImageRow::new(1, "IMG_0001")
                .cache("AAAA0001-1111-4111-8111-000000000001", &[(1616, 1077)], &[(1616, 1077)]),
            ImageRow::new(2, "img_0001")
                .cache("AAAA0002-1111-4111-8111-000000000002", &[(1616, 1077)], &[(1616, 1077)]),
        ]);
        let plan = fixture.plan(&Options::default());
        let first = item(&plan, 1).output.as_ref().expect("output");
        let second = item(&plan, 2).output.as_ref().expect("output");

        assert_eq!(first.file_name().unwrap(), "IMG_0001.jpg");
        assert_ne!(first, second);
        if cfg!(windows) {
            assert_eq!(
                second.file_name().unwrap(),
                "img_0001 (2).jpg",
                "one overwrites the other unless the dedupe key is case-insensitive"
            );
        }
    }
}
