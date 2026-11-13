//! Plan: turn a catalog plus a preview index into explicit decisions.
//!
//! Nothing here writes to disk. A plan is cheap to build, printable, and
//! machine-readable, which is what makes `scan` a safe first step.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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
}

impl Default for Options {
    fn default() -> Self {
        Options {
            min_long_edge: 640,
            all_levels: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub images: u64,
    pub recoverable: u64,
    pub missing_payload: u64,
    pub no_cache_entry: u64,
    pub unmapped: u64,
    pub suspect: u64,
    pub failed: u64,
    pub below_floor: u64,
    pub bytes: u64,
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
        let mut used: HashMap<PathBuf, u32> = HashMap::new();
        let mut claimed: HashSet<String> = HashSet::new();
        // Seed from every row, including duplicate `imageId`s that the map
        // collapsed, so their payloads are not reported as orphans.
        for entry in &index.all_entries {
            claimed.insert(format!("{}-{}", entry.uuid, entry.digest));
        }

        for image in images {
            let mut image = image;
            // Metadata is fetched here, once per image, so both the manifest
            // and the EXIF writer read the same facts.
            if image.xmp.is_none() {
                image.xmp = catalog.xmp(image.id_local).ok().flatten();
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
                            "decoded {w}x{h} but previews.db declares {}x{}",
                            level.width, level.height
                        ));
                    }
                }

                if !jpeg::is_complete(chunk.data) {
                    status = Status::Failed;
                    note = Some("level does not end with an EOI marker".into());
                }

                let relative = if options.all_levels {
                    let file = format!("{} - {}x{}.jpg", image.output_stem(), w, h);
                    image.relative_output_path().with_file_name(file)
                } else {
                    image.relative_output_path()
                };
                let output = unique_path(&mut used, output_root.join(relative));

                match status {
                    Status::Recoverable => {
                        stats.recoverable += 1;
                        if selected {
                            stats.bytes += chunk.data.len() as u64;
                        }
                    }
                    // Suspect payloads are still real photos, just smaller or
                    // differently sized than `previews.db` claims, so they are
                    // extracted too — and flagged in the report.
                    Status::Suspect => {
                        stats.suspect += 1;
                        if selected {
                            stats.bytes += chunk.data.len() as u64;
                        }
                    }
                    Status::Failed => stats.failed += 1,
                    _ => {}
                }
                *stats.tiers.entry(tier.as_str()).or_insert(0) += 1;

                items.push(PlanItem {
                    status,
                    image: Some(image.clone()),
                    payload: Some(payload.clone()),
                    output: Some(output),
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
        }

        // Payloads with no cache entry: recoverable, but unmappable.
        for name in payload_names(cache_root) {
            let stem = name.rsplit('/').next().unwrap_or(&name);
            if claimed.contains(stem) {
                continue;
            }
            let output_name = stem.replace('/', "_");
            items.push(PlanItem {
                status: Status::Unmapped,
                image: None,
                payload: Some(cache_root.join(&name).with_extension("lrprev")),
                output: Some(
                    output_root
                        .join("_unmapped")
                        .join(format!("{output_name}.jpg")),
                ),
                width: None,
                height: None,
                color_profile: None,
                quality: None,
                orientation: Orientation::Unknown,
                tier: None,
                declared: None,
                selected: false,
                note: Some("payload exists but previews.db has no entry for it".into()),
            });
            stats.unmapped += 1;
        }

        Ok(Plan { items, stats })
    }

    /// Every item the extractor should write.
    pub fn selected(&self) -> impl Iterator<Item = &PlanItem> {
        self.items.iter().filter(|i| i.selected)
    }
}

/// Append ` (2)`, ` (3)` … when two images resolve to the same output path.
fn unique_path(used: &mut HashMap<PathBuf, u32>, path: PathBuf) -> PathBuf {
    let count = used.entry(path.clone()).or_insert(0);
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
