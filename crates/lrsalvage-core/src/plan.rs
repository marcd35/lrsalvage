//! Plan: turn a catalog plus a preview index into explicit decisions.
//!
//! Nothing here writes to disk. A plan is cheap to build, printable, and
//! machine-readable, which is what makes `scan` a safe first step.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use crate::catalog::Catalog;
use crate::error::Result;
use crate::model::{CatalogImage, Orientation, Pyramid};
use crate::previews_db::{
    CacheSweep, Candidate, Layout, LevelOnDisk, PreviewIndex, PyramidKey, PyramidOnDisk,
};
use crate::{
    exif::{self, PayloadMetadata},
    fs, jpeg, lrprev, previews_db,
};

/// Where a payload lives, decided once so no reader has to sniff it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    /// A legacy `.lrprev` and the `level_N` chunk that was certified. `level`
    /// is `None` when nothing was certified — a file that is gone, one that
    /// failed to parse, or a pyramid no image claims — and such an item is
    /// never written.
    Container {
        path: PathBuf,
        level: Option<String>,
    },
    /// A modern cache's level file: the file *is* the level.
    LevelFile { path: PathBuf },
}

impl Payload {
    /// The payload file itself — a level file, or the container a level lives
    /// in. `report` and the item lines print this and nothing else.
    pub fn path(&self) -> &Path {
        match self {
            Payload::Container { path, .. } | Payload::LevelFile { path } => path,
        }
    }
}

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
    pub payload: Option<Payload>,
    pub output: Option<PathBuf>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub color_profile: Option<String>,
    pub quality: Option<String>,
    /// The catalog's claim about rotation, kept as recorded.
    pub orientation: Orientation,
    /// What the output states: the payload's own orientation when its EXIF
    /// declares one, else the catalog's. Computed once, here.
    pub effective_orientation: Orientation,
    /// The metadata segments the chosen payload already carries, read by the
    /// certification walk. Empty when it is bare; never a status.
    pub payload_metadata: PayloadMetadata,
    pub tier: Option<Tier>,
    /// The size `previews.db` declares for this level: the chosen level's own
    /// row for a level file, the pyramid's cropped size for a container.
    pub declared: Option<(u32, u32)>,
    /// The smallest level of a level-file pyramid, for the contact sheet. `None`
    /// for a container item, whose sheet walks its own chunks.
    pub smallest_level: Option<PathBuf>,
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
    /// Write the largest level at or below this long edge, instead of the
    /// largest present. `0` disables the cap. A cap below the floor is a legal
    /// window with nothing in it, not an error (`adr-0013-a-cap-demotes-the-level`).
    pub max_long_edge: u32,
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
            max_long_edge: 0,
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
    /// Level items above the configured cap: counted, reported and never
    /// written, because a cap chooses the level rather than emptying the image
    /// (`adr-0013-a-cap-demotes-the-level`).
    pub above_cap: u64,
    pub bytes: u64,
    /// Every level item in the plan, including the extra ones `--all_levels`
    /// emits for one image.
    pub levels: u64,
    /// Plan items the extractor evaluates: everything neither bound excluded.
    pub selected_items: u64,
    /// Distinct images among those items.
    pub selected_images: u64,
    /// Claimed pyramids holding **both** representations: at least one level
    /// file and at least one legacy `.lrprev`, whose container is never read.
    /// A cache stat, never a status, and it cannot move the exit code because
    /// the level files it counts always coexist with it
    /// (`adr-0006-degrade-without-guessing`).
    pub superseded: u64,
    /// `missing-payload` items whose pyramid carries a `DeletedPyramid` row
    /// Lightroom wrote. Its own record, never a claimed cause.
    pub deleted: u64,
    /// Payload-like files no shape matched, whatever the layout. Counted but
    /// never planned: an unrecognised file is not an orphan
    /// (`adr-0005-discover-payloads-by-index` ¶6).
    pub unrecognised: u64,
    pub tiers: HashMap<&'static str, u64>,
}

/// What the cache the plan read holds, as facts rather than decisions.
///
/// One struct, filled once, so every surface prints the same numbers: the
/// layout and the two version strings are observations
/// (`adr-0004-detect-layout-not-version`), and `shapes` describes files, never
/// an image's status.
#[derive(Debug, Clone, Default)]
pub struct CacheFacts {
    /// ADR 0004's five words, decided from the tree by `previews_db`.
    pub layout: Layout,
    /// `Adobe_DBVersion`, verbatim. Absent stays absent.
    pub db_version: Option<String>,
    /// `Adobe_CrsVersion`, verbatim. Absent stays absent.
    pub crs_version: Option<String>,
    pub shapes: Shapes,
    /// Unclaimed pyramids that are a render's second pyramid: a `thumbnail`
    /// row beside a pyramid an image claims, holding only small levels. Counted
    /// as a cache fact and never listed as an orphan, because every fresh
    /// render produces them (`adr-0008-a-render-companion-is-not-an-orphan`).
    pub thumbnail_pyramids: u64,
}

/// The class each payload took, counted once over the cache's files.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Shapes {
    /// Level files the cache holds, a row-derived leaf and a widened one alike.
    pub level_file: u64,
    /// How many of those leaves name the long edge their own `SOF` measured —
    /// the one claim derived from the index that can rot.
    pub suffix_matches_sof: u64,
    /// `.lrprev` containers the cache holds.
    pub container: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub items: Vec<PlanItem>,
    pub stats: Stats,
    pub cache: CacheFacts,
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
        let versions = catalog.versions()?;
        // One walk of the cache tree answers every cache-level question: the
        // layout, the shape histogram, which pyramids are superseded and which
        // nobody claims. A second pass would be a second answer.
        let sweep = previews_db::sweep(cache_root);
        let measured = measure(&sweep);
        let mut cache = CacheFacts {
            layout: sweep.layout(),
            db_version: versions.db_version,
            crs_version: versions.crs_version,
            shapes: measured.shapes,
            thumbnail_pyramids: 0,
        };
        let mut items = Vec::new();
        let mut stats = Stats {
            images: images.len() as u64,
            // An unrecognised file is never planned, so its count has no item
            // to come from.
            unrecognised: sweep.unrecognised.len() as u64,
            ..Stats::default()
        };
        let mut used: HashMap<String, u32> = HashMap::new();

        // Seed the claim set from rows that resolve to a real image only. A
        // stale row (`imageId` no longer in `Adobe_images`) must not suppress
        // the orphan sweep: its payload is exactly what salvage is for.
        let image_ids: HashSet<i64> = images.iter().map(|image| image.id_local).collect();
        let mut claimed: HashSet<PyramidKey> = HashSet::new();
        let mut claimed_uuids: HashSet<String> = HashSet::new();
        for entry in &index.all_entries {
            if image_ids.contains(&entry.image_id) {
                claimed_uuids.insert(entry.uuid.clone());
                claimed.insert(PyramidKey::of(entry));
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
            if !options.include_present && is_file(&image.original_path) {
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
                    effective_orientation: orientation,
                    payload_metadata: PayloadMetadata::default(),
                    tier: None,
                    declared: None,
                    smallest_level: None,
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
                    effective_orientation: Orientation::Unknown,
                    payload_metadata: PayloadMetadata::default(),
                    tier: None,
                    declared: None,
                    smallest_level: None,
                    selected: false,
                    note: Some("no previews.db row for this image".into()),
                });
                continue;
            };

            let key = PyramidKey::of(entry);
            claimed.insert(key.clone());
            let pyramid = index.pyramid(&key);
            let orientation = image.orientation;
            let container = index.payload_path(cache_root, &key);

            // A render writes a second, thumbnail-sized pyramid beside the one
            // this entry claims (§5.10). When the claimed pyramid has no file at
            // all, that companion can be the last surviving payload of the
            // image, so it becomes what is read and what the level is judged
            // against (`adr-0009-rescue-an-image-from-its-companion`).
            let companion_key =
                rescue_companion(&sweep, &measured, index, &claimed, &claimed_uuids, &key)
                    .map(|companion| PyramidKey::new(&companion.uuid, &companion.digest));

            // What the cache holds for this pyramid decides how it is read: a
            // level file is the modern representation, and where one exists the
            // container is never opened (`adr-0005-discover-payloads-by-index`
            // ¶7). Existence is the decision, so the stat happens here.
            let present: Vec<Candidate<'_>> = index
                .candidates(cache_root, &key)
                .into_iter()
                .filter(|candidate| is_file(&candidate.path))
                .collect();

            let read = if !present.is_empty() {
                let (levels, smallest_level) = level_file_levels(present, &measured, options);
                Some((levels, smallest_level, pyramid, None))
            } else if is_file(&container) {
                Some((
                    container_levels(index, &key, &container, options),
                    None,
                    pyramid,
                    None,
                ))
            } else {
                companion_key.as_ref().and_then(|companion| {
                    let available: Vec<Candidate<'_>> = index
                        .candidates(cache_root, companion)
                        .into_iter()
                        .filter(|candidate| is_file(&candidate.path))
                        .collect();
                    // A classified companion holds at least one level on disk by
                    // construction; the guard keeps "no file, no rescue" true
                    // even if that ever changes.
                    (!available.is_empty()).then(|| {
                        let (levels, smallest_level) =
                            level_file_levels(available, &measured, options);
                        (
                            levels,
                            smallest_level,
                            index.pyramid(companion),
                            Some(RESCUED_NOTE),
                        )
                    })
                })
            };

            let Some((levels, smallest_level, claim, rescued_from)) = read else {
                // No file of this pyramid, and no companion to fall back to. A
                // row whose own level file is gone while another survives is a
                // pruned level, not this: 14.0+ prunes in the background (§5.5).
                stats.missing_payload += 1;
                // A tombstone is Lightroom's own record that it deleted the
                // *row*, quoted and counted, never a claimed cause for the file
                // being gone (`adr-0006-degrade-without-guessing`).
                let tombstone = index.deletion(&key);
                if tombstone.is_some() {
                    stats.deleted += 1;
                }
                items.push(PlanItem {
                    status: Status::MissingPayload,
                    image: Some(image),
                    payload: Some(Payload::Container {
                        path: container,
                        level: None,
                    }),
                    output: None,
                    width: None,
                    height: None,
                    color_profile: pyramid.map(|p| p.color_profile.clone()),
                    quality: pyramid.map(|p| p.quality.clone()),
                    orientation,
                    effective_orientation: orientation,
                    payload_metadata: PayloadMetadata::default(),
                    tier: None,
                    declared: None,
                    smallest_level: None,
                    selected: false,
                    note: Some(match tombstone {
                        Some(timestamp) => format!(
                            "no payload file for this pyramid is on disk; previews.db records its \
                             deletion at {timestamp}"
                        ),
                        None => "no payload file for this pyramid is on disk".to_string(),
                    }),
                });
                continue;
            };

            let (image_items, image_status) = decide_levels(
                &image,
                claim,
                levels,
                smallest_level,
                rescued_from,
                &mut Tally {
                    output_root,
                    options,
                    used: &mut used,
                    stats: &mut stats,
                },
            );
            items.extend(image_items);

            match image_status {
                Some(Status::Recoverable) => stats.recoverable += 1,
                Some(Status::Suspect) => stats.suspect += 1,
                Some(Status::Failed) => stats.failed += 1,
                _ => {}
            }
        }

        // Payloads with no cache entry: recoverable, but unmappable. One item
        // per pyramid, never one per level file: one orphan is one lost
        // photograph, whatever its level set looks like
        // (`adr-0005-discover-payloads-by-index` ¶8). The enumeration is
        // `previews_db`'s; which of these pyramids an image claims is decided
        // by the claim set above, and a claimed pyramid holding both
        // representations is the `superseded` cache stat rather than an item.
        for pyramid in &sweep.pyramids {
            let key = PyramidKey::new(&pyramid.uuid, &pyramid.digest);
            if !claimed.contains(&key) {
                let row = index.pyramid(&key);
                // A render writes a second, thumbnail-sized pyramid beside the
                // one an image claims, and no entry names it (§5.10). It is a
                // fact about the cache, not an orphan: counted, never listed
                // (`adr-0008-a-render-companion-is-not-an-orphan`).
                if is_thumbnail_pyramid(pyramid, row, &claimed_uuids, &measured) {
                    cache.thumbnail_pyramids += 1;
                    continue;
                }
                let Some(payload) = unclaimed_payload(pyramid, &measured) else {
                    continue;
                };
                let named = payload.path().to_string_lossy().replace('\\', "/");
                items.push(PlanItem {
                    status: Status::Unmapped,
                    image: None,
                    payload: Some(payload),
                    // Nothing is extracted, so there is no output path to
                    // report; the payload's own path is the useful fact.
                    output: None,
                    width: None,
                    height: None,
                    // The pair reaches the row now, so an orphan says what kind
                    // of pyramid it is instead of printing two dashes.
                    color_profile: row.map(|row| row.color_profile.clone()),
                    quality: row.map(|row| row.quality.clone()),
                    orientation: Orientation::Unknown,
                    effective_orientation: Orientation::Unknown,
                    payload_metadata: PayloadMetadata::default(),
                    tier: None,
                    declared: None,
                    smallest_level: None,
                    selected: false,
                    note: Some(format!(
                        "payload exists but previews.db has no entry for it: {named}"
                    )),
                });
                stats.unmapped += 1;
                continue;
            }

            // Claimed and holding both representations: the level files are the
            // pyramid and the legacy container is never read, so this is a
            // count of what was set aside, never a loss.
            if pyramid.container.is_some() && !pyramid.levels.is_empty() {
                stats.superseded += 1;
            }
        }

        stats.selected_items = items.iter().filter(|item| item.selected).count() as u64;
        stats.selected_images = distinct_selected_images(&items) as u64;

        Ok(Plan {
            items,
            stats,
            cache,
        })
    }

    /// Every item the extractor evaluates: an item neither bound excluded,
    /// whatever its status. A `failed` one is reported and writes nothing.
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

/// The `Pyramid.quality` value a render writes on its second pyramid (§5.10).
const THUMBNAIL_QUALITY: &str = "thumbnail";

/// The note an item carries when its payload came from the image's own render
/// companion rather than from the pyramid the cache entry names.
///
/// No path and no digest: the useful fact is which pyramid it came from, and a
/// note that quotes a machine's path is one a report cannot carry
/// (`adr-0007-publish-a-verification-report`).
const RESCUED_NOTE: &str = "the pyramid previews.db names has no file; recovered from this \
                             image's render companion instead";

/// The image's own render companion, when the pyramid its entry names has no
/// file at all.
///
/// Only a **classified** companion may rescue — the same three clauses ADR 0008
/// uses, so the payload is the pyramid §5.10 measured a render writing beside the
/// preview one and no mapping is invented from a shared uuid
/// (`adr-0002-map-via-previews-db`, `adr-0009-rescue-an-image-from-its-companion`).
/// A sibling of another quality, an oversized one, or a uuid nothing maps stays
/// out: each is a state no measurement covers.
///
/// A cache that somehow holds more than one candidate is ranked by the largest
/// measured level, then by the digest, so the choice is never arbitrary.
fn rescue_companion<'a>(
    sweep: &'a CacheSweep,
    measured: &Measurements,
    index: &PreviewIndex,
    claimed: &HashSet<PyramidKey>,
    claimed_uuids: &HashSet<String>,
    key: &PyramidKey,
) -> Option<&'a PyramidOnDisk> {
    sweep
        .pyramids
        .iter()
        .filter(|pyramid| pyramid.uuid == key.uuid())
        .filter(|pyramid| {
            let candidate = PyramidKey::new(&pyramid.uuid, &pyramid.digest);
            candidate != *key && !claimed.contains(&candidate)
        })
        .filter(|pyramid| {
            let candidate = PyramidKey::new(&pyramid.uuid, &pyramid.digest);
            is_thumbnail_pyramid(pyramid, index.pyramid(&candidate), claimed_uuids, measured)
        })
        .max_by_key(|pyramid| (measured.largest_area(pyramid), &pyramid.digest))
}

/// `true` when this unclaimed pyramid is the second pyramid a render writes
/// beside a preview pyramid, rather than an orphan.
///
/// All three clauses are needed, and each is a way the state can differ from a
/// rendered cache's ordinary one
/// (`adr-0008-a-render-companion-is-not-an-orphan`):
///
/// - **a claimed pyramid on the same uuid**, from the live claim set — so a uuid
///   nothing maps keeps every pyramid it holds `unmapped`, tombstoned or not;
/// - **the row's own word**, `quality` exactly `thumbnail`, with no case folding
///   and no guess from size;
/// - **only small levels**, every one of them measured inside the `thumbnail`
///   tier, so a row that lies about its size can never hide a payload the tool
///   would otherwise be willing to write. A level whose frame cannot be read
///   disqualifies the pyramid too: the answer stays visible.
fn is_thumbnail_pyramid(
    pyramid: &PyramidOnDisk,
    row: Option<&Pyramid>,
    claimed_uuids: &HashSet<String>,
    measured: &Measurements,
) -> bool {
    if !claimed_uuids.contains(&pyramid.uuid) {
        return false;
    }
    if row.map(|row| row.quality.as_str()) != Some(THUMBNAIL_QUALITY) {
        return false;
    }
    !pyramid.levels.is_empty()
        && pyramid.levels.iter().all(|level| {
            measured
                .frames
                .get(&level.path)
                .is_some_and(|(width, height)| {
                    Tier::from_long_edge((*width).max(*height)) == Tier::Thumbnail
                })
        })
}

/// What an unclaimed pyramid is reported as: the largest level file it holds by
/// **measured** `SOF` area, or the `.lrprev` when it holds no level file.
///
/// Nothing is selected or written for either arm: an unclaimed payload has no
/// catalog image, so there is no output path to plan for it.
///
/// The rank is measured, in every mode, because the row is the claim and the
/// bytes are the verdict (`adr-0005-discover-payloads-by-index` ¶5, ¶9). A leaf's
/// `_<n>` is only its author's word; a file whose frame cannot be read at all
/// ranks below one that can, then by that name, then by path, so the choice is
/// never arbitrary.
fn unclaimed_payload(pyramid: &PyramidOnDisk, measured: &Measurements) -> Option<Payload> {
    if let Some(level) = measured.largest_level(pyramid) {
        return Some(Payload::LevelFile {
            path: level.to_path_buf(),
        });
    }
    pyramid.container.as_ref().map(|path| Payload::Container {
        path: path.clone(),
        // A container no image claims: listed and never written, so no
        // level was ever certified in it.
        level: None,
    })
}

/// One read of every level file the cache holds, and the two facts it yields.
///
/// The histogram and the rank come from the same bytes, so a `scan` pays for one
/// pass over the cache and not two: `suffix_matches_sof` is the count that catches
/// an index whose names rotted, and the measured area is what orders a pyramid
/// nobody claims (`adr-0005-discover-payloads-by-index` ¶9,
/// `adr-0007-publish-a-verification-report`).
#[derive(Debug, Default)]
struct Measurements {
    shapes: Shapes,
    /// `<cache>/<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>_<n>` → the frame its own
    /// `SOF` declared. Absent when the bytes declared no readable frame.
    frames: HashMap<PathBuf, (u32, u32)>,
}

impl Measurements {
    /// The pixel area this level file's own frame declared.
    fn area(&self, path: &Path) -> Option<u64> {
        self.frames
            .get(path)
            .map(|(width, height)| u64::from(*width) * u64::from(*height))
    }

    /// The area of this pyramid's largest measured level, `0` when none could be
    /// read — what ranks one rescue candidate against another.
    fn largest_area(&self, pyramid: &PyramidOnDisk) -> u64 {
        pyramid
            .levels
            .iter()
            .filter_map(|level| self.area(&level.path))
            .max()
            .unwrap_or(0)
    }

    /// The largest level file of this pyramid, by measured area, then by the
    /// `_<n>` the leaf names, then by path.
    fn largest_level<'a>(&self, pyramid: &'a PyramidOnDisk) -> Option<&'a Path> {
        pyramid
            .levels
            .iter()
            .max_by(|a, b| {
                (self.area(&a.path), a.long_edge, &a.path).cmp(&(
                    self.area(&b.path),
                    b.long_edge,
                    &b.path,
                ))
            })
            .map(|level| level.path.as_path())
    }
}

/// The class each payload took, counted over the one tree walk, and the area each
/// level file's own frame declares.
///
/// `level_file` and `container` need nothing but names. `suffix_matches_sof` and
/// the areas are the two counts that read a payload: they compare the `_<n>` a
/// leaf names with the long edge its own `SOF` measured, which is what catches an
/// index that rotted and what ranks an unclaimed pyramid. Incompleteness does not
/// disqualify a readable frame, and a file with no readable `SOF` simply does not
/// match (`adr-0007-publish-a-verification-report`).
///
/// Each level file is read whole, and that is the cost this pass adds to a scan:
/// a bounded prefix could cut an `APPn` segment in half and lose a frame that is
/// really there, and a frame that cannot be read counts as no match.
fn measure(sweep: &CacheSweep) -> Measurements {
    let mut measured = Measurements::default();
    for pyramid in &sweep.pyramids {
        measured.shapes.level_file += pyramid.levels.len() as u64;
        measured.shapes.container += u64::from(pyramid.container.is_some());
        for level in &pyramid.levels {
            let Some((width, height)) = measured_frame(level) else {
                continue;
            };
            if width.max(height) == level.long_edge {
                measured.shapes.suffix_matches_sof += 1;
            }
            measured.frames.insert(level.path.clone(), (width, height));
        }
    }
    measured
}

/// `is_file` through the seam, for a decision that only needs a yes or a no.
///
/// A path the seam cannot answer for is not a file, which is exactly what
/// `Path::is_file` has always meant here: a payload that is not there is a fact
/// about the cache, and this module decides about caches rather than disks.
fn is_file(path: &Path) -> bool {
    fs::is_file(path).unwrap_or(false)
}

/// The frame this level file's own bytes declare, `None` when they declare none.
fn measured_frame(level: &LevelOnDisk) -> Option<(u32, u32)> {
    let bytes = fs::read(&level.path).ok()?;
    jpeg::certify(&bytes)
        .ok()
        .map(|certified| certified.dimensions)
}

/// One level of one pyramid, as the plan found it.
struct FoundLevel {
    /// `true` for the level that owns the image's plain output name; the extra
    /// levels `--all-levels` adds carry a size suffix instead.
    winner: bool,
    payload: Payload,
    /// The frame the bytes declare, when they could be read at all.
    dimensions: Option<(u32, u32)>,
    /// Why this level cannot be written. A failure is never papered over by a
    /// smaller level: only a file that is *absent* falls back.
    failure: Option<String>,
    /// The size `previews.db` declares for this item.
    declared: Option<(u32, u32)>,
    /// The size this level is judged against. `None` when nothing is compared:
    /// a container's smaller levels, and any level with no row behind it.
    claim: Option<(u32, u32)>,
    /// The metadata segments the payload's own bytes carry.
    metadata: PayloadMetadata,
    /// The `Orientation` the payload's own EXIF declares, if any. It describes
    /// these pixels and so outranks the catalog's.
    declared_orientation: Option<Orientation>,
    /// Payload bytes, for the run's total.
    bytes: usize,
}

/// What every level decision needs but none of them owns.
struct Tally<'a> {
    output_root: &'a Path,
    options: &'a Options,
    used: &'a mut HashMap<String, u32>,
    stats: &'a mut Stats,
}

/// The long edge a candidate claims, in the order the plan trusts it: the frame
/// the file's own bytes declared, then the row that named it, then the `_<n>` in
/// its name.
///
/// The cap judges a level by its bytes the way the floor already does, so a row
/// that lies about a size cannot smuggle a payload past it. The row and the name
/// are only for a file the cache sweep found no readable frame for — the same
/// "unknown" answer the floor gives such a level
/// (`adr-0013-a-cap-demotes-the-level`).
fn candidate_long_edge(candidate: &Candidate<'_>, measured: &Measurements) -> u32 {
    measured
        .frames
        .get(&candidate.path)
        .map(|(width, height)| (*width).max(*height))
        .or_else(|| candidate.row.map(|row| row.width.max(row.height)))
        .unwrap_or(candidate.long_edge)
}

/// The index of the level a run writes: the last one that clears the cap, or the
/// last present when the cap is `0` or nothing clears it.
///
/// The cap chooses the winner rather than filtering after the ranking, so a bound
/// nothing clears still leaves the largest level as the item that carries the
/// image's status — the shape an image below the floor already has — and a bound
/// that binds leaves the demoted level as an item too, `selected: false`
/// (`adr-0013-a-cap-demotes-the-level`). Both ranks that write a level call this,
/// so the two cannot drift apart.
fn capped_winner(len: usize, cap: u32, long_edge_at: impl Fn(usize) -> u32) -> usize {
    let last = len - 1;
    match cap {
        0 => last,
        cap => (0..len)
            .rposition(|index| long_edge_at(index) <= cap)
            .unwrap_or(last),
    }
}

/// A pyramid's level files, ranked by the claim and then certified.
///
/// Only the winner is read unless `--all-levels` asked for every one: the rank
/// comes from each candidate's own row, so a five-level pyramid costs one file
/// read, not five. Returns the smallest candidate the same enumeration found,
/// which the contact sheet inlines.
fn level_file_levels(
    candidates: Vec<Candidate<'_>>,
    measured: &Measurements,
    options: &Options,
) -> (Vec<FoundLevel>, Option<PathBuf>) {
    let mut ranked = candidates;
    // Pixel area as the row declares it, with the level number as tie-break. A
    // file with no row — the widened glob's business — ranks below every claim,
    // so a claim is never beaten by a guess, and ranks among its own kind by
    // the long edge its leaf names: nothing is invented to compare it with
    // (`adr-0005-discover-payloads-by-index` ¶5).
    ranked.sort_by_key(|candidate| match candidate.row {
        Some(row) => (1u8, u64::from(row.width) * u64::from(row.height), row.level),
        None => (0, 0, candidate.long_edge),
    });

    let smallest_level = ranked.first().map(|candidate| candidate.path.clone());
    let winner = capped_winner(ranked.len(), options.max_long_edge, |index| {
        candidate_long_edge(&ranked[index], measured)
    });
    let chosen: Vec<usize> = if options.all_levels {
        (0..ranked.len()).collect()
    } else {
        vec![winner]
    };

    let levels = chosen
        .into_iter()
        .map(|index| {
            let candidate = &ranked[index];
            let read = fs::read(&candidate.path);
            let (dimensions, failure, metadata, declared_orientation) = match &read {
                Ok(bytes) => match jpeg::certify(bytes) {
                    Ok(certified) => (
                        Some(certified.dimensions),
                        (!certified.complete)
                            .then(|| "level does not end with an EOI marker".to_string()),
                        exif::inventory(bytes, &certified.segments),
                        exif::orientation(bytes, &certified.segments),
                    ),
                    Err(e) => (
                        None,
                        Some(format!("not a JPEG preview: {e}")),
                        PayloadMetadata::default(),
                        None,
                    ),
                },
                Err(e) => (
                    None,
                    Some(format!("unreadable payload: {e}")),
                    PayloadMetadata::default(),
                    None,
                ),
            };
            FoundLevel {
                winner: index == winner,
                payload: Payload::LevelFile {
                    path: candidate.path.clone(),
                },
                dimensions,
                failure,
                // The row that derived this leaf is the claim, and its own row
                // is all it is compared against, so a stale neighbour cannot
                // make a surviving level look wrong. A file the widening found
                // has no row: there is nothing to disagree with, so it cannot
                // be `suspect` however small it is.
                declared: candidate.row.map(|row| (row.width, row.height)),
                claim: candidate.row.map(|row| (row.width, row.height)),
                metadata,
                declared_orientation,
                bytes: read.map(|bytes| bytes.len()).unwrap_or(0),
            }
        })
        .collect();

    (levels, smallest_level)
}

/// A `.lrprev`, read exactly as it always was: parsed, measured level by level,
/// ranked by decoded area.
///
/// Only read when the container is the pyramid's only representation. The claim
/// a level is judged against is the pyramid's own best row, and only on the
/// level this container writes: its whole level set is in the file, or the file
/// is truncated.
fn container_levels(
    index: &PreviewIndex,
    key: &PyramidKey,
    container: &Path,
    options: &Options,
) -> Vec<FoundLevel> {
    let pyramid = index.pyramid(key);
    let best = index.best_level(key);
    let declared = pyramid.map(|p| (p.cropped_width, p.cropped_height));
    let failed = |note: String| FoundLevel {
        winner: true,
        payload: Payload::Container {
            path: container.to_path_buf(),
            level: None,
        },
        dimensions: None,
        failure: Some(note),
        declared,
        claim: None,
        metadata: PayloadMetadata::default(),
        declared_orientation: None,
        bytes: 0,
    };

    let bytes = match fs::read(container) {
        Ok(bytes) => bytes,
        Err(e) => return vec![failed(format!("unreadable payload: {e}"))],
    };
    let parsed = match lrprev::Lrprev::parse(&bytes) {
        Ok(parsed) => parsed,
        Err(e) => return vec![failed(format!("not a usable .lrprev: {e}"))],
    };

    // Measure every level once; the declared header sizes are hints only, the
    // decoded SOF dimensions are the truth.
    let mut measured: Vec<(&lrprev::Chunk<'_>, jpeg::Certified)> = parsed
        .levels()
        .into_iter()
        .filter_map(|chunk| {
            jpeg::certify(chunk.data)
                .ok()
                .map(|certified| (chunk, certified))
        })
        .collect();
    measured.sort_by_key(|(_, certified)| {
        let (width, height) = certified.dimensions;
        u64::from(width) * u64::from(height)
    });

    if measured.is_empty() {
        let note = match parsed.header().and_then(|h| std::str::from_utf8(h).ok()) {
            Some(_) => "no level contains a readable JPEG frame".to_string(),
            None => "no readable levels and no header".to_string(),
        };
        return vec![failed(note)];
    }

    let last = measured.len() - 1;
    let winner = capped_winner(measured.len(), options.max_long_edge, |index| {
        let (width, height) = measured[index].1.dimensions;
        width.max(height)
    });
    let chosen: Vec<usize> = if options.all_levels {
        (0..measured.len()).collect()
    } else {
        vec![winner]
    };
    chosen
        .into_iter()
        .map(|position| {
            let (chunk, certified) = &measured[position];
            let judged = position == winner;
            // The level a container writes is compared against the row that
            // claims *that* size. The pyramid's own best row is that claim only
            // when the chunk is the container's largest, which is every
            // uncapped run; a cap can choose a smaller chunk, and judging it
            // against the best row would flag an ordinary level `suspect`
            // (`adr-0005-discover-payloads-by-index` ¶5,
            // `adr-0013-a-cap-demotes-the-level`).
            let claim = if !judged {
                None
            } else if position == last {
                best.map(|row| (row.width, row.height))
            } else {
                let (width, height) = certified.dimensions;
                index
                    .row_for_size(key, width, height)
                    .map(|row| (row.width, row.height))
            };
            FoundLevel {
                winner: judged,
                payload: Payload::Container {
                    path: container.to_path_buf(),
                    level: Some(chunk.name.clone()),
                },
                dimensions: Some(certified.dimensions),
                failure: (!certified.complete)
                    .then(|| "level does not end with an EOI marker".to_string()),
                declared,
                claim,
                metadata: exif::inventory(chunk.data, &certified.segments),
                declared_orientation: exif::orientation(chunk.data, &certified.segments),
                bytes: chunk.data.len(),
            }
        })
        .collect()
}

/// Turn one pyramid's levels into plan items, and report the image's status.
///
/// One status per image: the level the image is written from decides, so
/// `--all-levels` cannot make one photo look like several recoveries.
fn decide_levels(
    image: &CatalogImage,
    pyramid: Option<&Pyramid>,
    levels: Vec<FoundLevel>,
    smallest_level: Option<PathBuf>,
    // Where the payload came from, when it is not the pyramid the entry names.
    rescued_from: Option<&str>,
    tally: &mut Tally<'_>,
) -> (Vec<PlanItem>, Option<Status>) {
    let mut levels = levels;
    // The level that owns the plain output name is marked by the rank, not by its
    // position: a cap can choose one that is not the last candidate.
    let winner = levels
        .iter()
        .position(|level| level.winner)
        .unwrap_or(levels.len() - 1);
    let chosen: Vec<FoundLevel> = if tally.options.all_levels {
        levels
    } else {
        vec![levels.swap_remove(winner)]
    };

    let mut image_status = None;
    let mut items = Vec::with_capacity(chosen.len());

    for level in chosen {
        let tier = level
            .dimensions
            .map(|(width, height)| Tier::from_long_edge(width.max(height)));
        // The floor and the cap bound which levels are written; neither empties
        // an image and neither fails one. A broken level is still evaluated, so
        // the run reports it and exits partial instead of claiming a clean
        // success over a cache it could not read. A level whose size cannot be
        // measured is not below the floor and not above the cap — it is unknown.
        let below_floor = level
            .dimensions
            .is_some_and(|(width, height)| width.max(height) < tally.options.min_long_edge);
        let above_cap = tally.options.max_long_edge != 0
            && level
                .dimensions
                .is_some_and(|(width, height)| width.max(height) > tally.options.max_long_edge);

        let mut status = Status::Recoverable;
        let mut note = None;
        if let Some(reason) = level.failure {
            status = Status::Failed;
            note = Some(reason);
        } else if let Some((declared_width, declared_height)) = level.claim {
            let (width, height) = level.dimensions.unwrap_or_default();
            // Width and height may be swapped before calling it a disagreement:
            // the stored render is unrotated, so the cache may record the
            // upright shape. A real photograph at the wrong size is still worth
            // having, and every report flags it.
            if (declared_width != width || declared_height != height)
                && (declared_width != height || declared_height != width)
            {
                status = Status::Suspect;
                note = Some(format!(
                    "decoded {width}x{height} but previews.db declares \
                     {declared_width}x{declared_height}; kept and flagged because the payload is \
                     not the size the catalog expected"
                ));
            }
        }

        // The payload's own orientation describes *these* pixels, so it is what
        // the output states; the catalog's applies only when the payload says
        // nothing. Computed once, and a disagreement is a note, never a status
        // (`adr-0003-preserve-preview-bytes` §4).
        let effective_orientation = level.declared_orientation.unwrap_or(image.orientation);
        let mut notes: Vec<String> = rescued_from
            .map(str::to_string)
            .into_iter()
            .chain(note)
            .collect();

        let writable = matches!(status, Status::Recoverable | Status::Suspect);
        if writable && below_floor {
            tally.stats.below_floor += 1;
        }
        if writable && above_cap {
            tally.stats.above_cap += 1;
        }

        // Only the extra levels `--all-levels` adds carry a size suffix; the
        // pyramid's largest level keeps the plain `<stem>.jpg` name.
        let relative = if tally.options.all_levels && !level.winner {
            let (width, height) = level.dimensions.unwrap_or_default();
            image.relative_output_path_with_suffix(&format!(" - {width}x{height}"))
        } else {
            image.relative_output_path()
        };

        let mut output = None;
        match confined_output(tally.output_root, &relative) {
            Ok(path) => output = Some(unique_path(tally.used, path)),
            Err(escape) => {
                status = Status::Failed;
                notes.clear();
                notes.push(escape);
            }
        }

        // What a payload carried, and what writing metadata does with it, is
        // reported rather than buried (`adr-0003-preserve-preview-bytes` §2).
        // The plan holds no metadata flag — `extract` owns that choice — so the
        // note states the trade and `extract`'s own note states the act.
        if matches!(status, Status::Recoverable | Status::Suspect) {
            if level.metadata.exif {
                notes.push(
                    "the payload carries its own EXIF; with metadata written, ours replaces \
                     its segment and render-time tags are not carried over"
                        .to_string(),
                );
            }
            if level.declared_orientation.is_some_and(|declared| {
                image.orientation != Orientation::Unknown && declared != image.orientation
            }) {
                notes.push(
                    "the payload's own EXIF orientation disagrees with the catalog's; the \
                     payload's value describes these pixels and is written"
                        .to_string(),
                );
            }
        }
        let note = (!notes.is_empty()).then(|| notes.join("; "));

        // Only a level that is actually written contributes bytes; a failed one
        // is evaluated and reported, and writes nothing.
        let writable = writable && !matches!(status, Status::Failed);
        if level.winner {
            image_status = Some(status);
        }
        tally.stats.levels += 1;
        if writable && !below_floor && !above_cap {
            tally.stats.bytes += level.bytes as u64;
        }
        if let Some(tier) = tier {
            *tally.stats.tiers.entry(tier.as_str()).or_insert(0) += 1;
        }

        items.push(PlanItem {
            status,
            image: Some(image.clone()),
            payload: Some(level.payload),
            output,
            width: level.dimensions.map(|(width, _)| width),
            height: level.dimensions.map(|(_, height)| height),
            color_profile: pyramid.map(|p| p.color_profile.clone()),
            quality: pyramid.map(|p| p.quality.clone()),
            orientation: image.orientation,
            effective_orientation,
            payload_metadata: level.metadata,
            tier,
            declared: level.declared,
            smallest_level: smallest_level.clone(),
            selected: !below_floor && !above_cap,
            note,
        });
    }

    (items, image_status)
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
    use super::*;

    // The plan's fixture-dependent tests live in `tests/plan.rs` and compose a
    // `Vault` (`specs/fixtures.md`). What stays here reaches a private helper and
    // needs no bytes, so it travels in the package and runs where the crate lands.

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
}
