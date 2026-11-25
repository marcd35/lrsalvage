//! Plan recovery of indexed JPEGs when the catalog is unavailable.

use std::collections::HashMap;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::IoContext;
use crate::plan::{Control, Progress};
use crate::previews_db::{self, Layout, PyramidKey};
use crate::{Error, Result, fs, jpeg, lrprev};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Recoverable,
    Missing,
    Invalid,
    Unsupported,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recoverable => "recoverable",
            Self::Missing => "missing",
            Self::Invalid => "invalid",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContainerStatus {
    Usable,
    Invalid,
    Unsupported,
}

impl ContainerStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Usable => "usable",
            Self::Invalid => "invalid",
            Self::Unsupported => "unsupported",
        }
    }
}

/// What an indexless run counts: one outcome per `.lrprev` container.
///
/// Not the indexed counts under other words: `containers` is a number of files
/// and `usable` is how many of them yielded a certified JPEG, where the indexed
/// mode counts pyramids the index named.
#[derive(Debug, Default, Serialize)]
pub struct ContainerCounts {
    pub containers: usize,
    pub usable: usize,
    pub invalid: usize,
    pub unsupported: usize,
}

/// One container, and the one preview this mode offers from it.
#[derive(Debug)]
pub struct ContainerItem {
    /// The container's exact cache-relative path: this mode's identity for a
    /// candidate, and the only input its generated name is derived from.
    pub relative: PathBuf,
    pub filename: String,
    pub status: ContainerStatus,
    /// The chosen JPEG, `Some` exactly when the status is `usable`.
    pub preview: Option<Preview>,
}

/// A cache recovered with no index at all (#198).
#[derive(Debug)]
pub struct LegacyRecovery {
    /// One entry per `.lrprev` container found, in cache-relative path order.
    pub items: Vec<ContainerItem>,
    pub counts: ContainerCounts,
}

impl LegacyRecovery {
    /// Enumerate every `.lrprev` under `cache` and choose one JPEG from each.
    ///
    /// Nothing here opens `previews.db`: with no index there is nothing to map,
    /// so a container is a candidate on its own and the name it produces comes
    /// from where it sits. A stopped scan has no plan, as the indexed one has
    /// none.
    pub fn scan(
        cache: &Path,
        progress: &mut dyn FnMut(Progress) -> Control,
    ) -> Result<Option<Self>> {
        let walked = previews_db::containers(cache, &mut |files| {
            progress(Progress::Cache { files }) == Control::Continue
        })?;
        let Some(paths) = walked else {
            return Ok(None);
        };
        let total = paths.len() as u64;
        let mut counts = ContainerCounts {
            containers: paths.len(),
            ..ContainerCounts::default()
        };
        let mut items = Vec::with_capacity(paths.len());
        for (done, relative) in paths.into_iter().enumerate() {
            if progress(Progress::Payloads {
                done: done as u64,
                total,
            }) == Control::Stop
            {
                return Ok(None);
            }
            let item = inspect_container(cache, relative);
            match item.status {
                ContainerStatus::Usable => counts.usable += 1,
                ContainerStatus::Invalid => counts.invalid += 1,
                ContainerStatus::Unsupported => counts.unsupported += 1,
            }
            items.push(item);
        }
        if progress(Progress::Payloads { done: total, total }) == Control::Stop {
            return Ok(None);
        }
        Ok(Some(Self { items, counts }))
    }
}

/// One container's outcome: its best certified JPEG, or why it has none.
fn inspect_container(cache: &Path, relative: PathBuf) -> ContainerItem {
    let path = cache.join(&relative);
    let mut item = ContainerItem {
        filename: legacy_filename(&relative),
        relative,
        status: ContainerStatus::Invalid,
        preview: None,
    };
    let Ok(bytes) = fs::read(&path) else {
        // A file this run cannot read is one outcome, not a stopped scan: the
        // inventory is complete, and the candidate is the thing that failed.
        return item;
    };
    let (container, parsed) = lrprev::Lrprev::parse_prefix(&bytes);
    let levels = container.levels();
    let mut best: Option<(usize, (u32, u32))> = None;
    for (index, chunk) in levels.iter().enumerate() {
        let Ok(frame) = jpeg::certify_recovery(chunk.data) else {
            continue;
        };
        if !frame.complete || area(frame.dimensions) == 0 {
            continue;
        }
        // Strictly greater: an equal-area tie keeps the chunk that came first
        // in file order, which is the tie-break this mode promises.
        if best.is_none_or(|(_, size)| area(frame.dimensions) > area(size)) {
            best = Some((index, frame.dimensions));
        }
    }
    if let Some((index, dimensions)) = best {
        item.status = ContainerStatus::Usable;
        item.preview = Some(Preview {
            path,
            chunk: Some(index),
            dimensions,
            bytes: levels[index].data.len() as u64,
            sha256: hex(&Sha256::digest(levels[index].data)),
        });
        return item;
    }
    // No certified JPEG. A container that parsed, names at least one image and
    // holds only formats this reader recognizes as images is one this tool
    // cannot read rather than a damaged file, and saying so is the difference
    // between "unsupported" and "invalid".
    item.status = if parsed.is_ok()
        && !levels.is_empty()
        && levels.iter().all(|chunk| jpeg::is_other_image(chunk.data))
    {
        ContainerStatus::Unsupported
    } else {
        ContainerStatus::Invalid
    };
    item
}

/// The generated name one container gets: its exact cache-relative path, hashed
/// behind a prefix this mode owns.
///
/// The path is joined with `/` so the same cache composes the same name on a
/// target that spells separators differently, and the cache root's own name is
/// not part of it — so moving a whole cache, or adding another container beside
/// it, leaves every existing name alone. Two containers never share one, however
/// identical their previews are.
fn legacy_filename(relative: &Path) -> String {
    let mut hash = Sha256::new();
    let mut first = true;
    for component in relative.components() {
        if let Component::Normal(part) = component {
            if !first {
                hash.update(b"/");
            }
            first = false;
            hash.update(part.as_encoded_bytes());
        }
    }
    format!("legacy-preview-{}.jpg", hex(&hash.finalize()))
}

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub indexed: usize,
    pub recoverable: usize,
    pub missing: usize,
    pub invalid: usize,
    pub unsupported: usize,
}

#[derive(Debug)]
pub struct Item {
    pub filename: String,
    pub status: Status,
    pub fallback: bool,
    pub dimension_mismatch: bool,
    pub preview: Option<Preview>,
}

#[derive(Debug)]
pub struct CacheRecovery {
    pub layout: Layout,
    pub items: Vec<Item>,
    pub counts: Counts,
}

impl CacheRecovery {
    /// Scan the cache without opening a catalog. A stopped scan has no plan.
    pub fn scan(
        cache: &Path,
        progress: &mut dyn FnMut(Progress) -> Control,
    ) -> Result<Option<Self>> {
        let index = previews_db::open_required(cache)?;
        let Some(sweep) = previews_db::sweep_required(cache, &mut |files| {
            progress(Progress::Cache { files }) == Control::Continue
        })?
        else {
            return Ok(None);
        };
        let layout = sweep.layout();
        let mut on_disk: HashMap<_, _> = sweep
            .pyramids
            .into_iter()
            .map(|pyramid| (PyramidKey::new(&pyramid.uuid, &pyramid.digest), pyramid))
            .collect();
        let keys = index.keys();
        let total = keys.len() as u64;
        let mut counts = Counts {
            indexed: keys.len(),
            ..Counts::default()
        };
        let mut items = Vec::with_capacity(keys.len());
        for (done, key) in keys.into_iter().enumerate() {
            if progress(Progress::Payloads {
                done: done as u64,
                total,
            }) == Control::Stop
            {
                return Ok(None);
            }
            let mut selection = Selection::default();
            if key.has_supported_address() {
                let mut declared = index.row_candidates(cache, &key);
                previews_db::order_candidates(&mut declared);
                let mut declared: HashMap<_, _> = declared
                    .into_iter()
                    .map(|candidate| (candidate.path.clone(), candidate))
                    .collect();
                if let Some(found) = on_disk.remove(&key) {
                    // Only open paths whose literal spelling the cache walk
                    // observed. An OS may resolve a differently cased name.
                    for level in found.levels {
                        let candidate =
                            declared
                                .remove(&level.path)
                                .unwrap_or(previews_db::Candidate {
                                    path: level.path,
                                    row: None,
                                    long_edge: level.long_edge,
                                });
                        selection.inspect_level(candidate);
                    }
                    if let Some(container) = found.container {
                        selection.inspect_container(&container, index.levels(&key));
                    }
                }
                for candidate in declared.into_values() {
                    selection.missing_level(candidate);
                }
            } else {
                selection.unsupported = true;
            }
            let item = selection.finish(&key);
            match item.status {
                Status::Recoverable => counts.recoverable += 1,
                Status::Invalid => counts.invalid += 1,
                Status::Unsupported => counts.unsupported += 1,
                Status::Missing => counts.missing += 1,
            }
            items.push(item);
        }
        if progress(Progress::Payloads { done: total, total }) == Control::Stop {
            return Ok(None);
        }
        items.sort_by(|a, b| a.filename.cmp(&b.filename));
        Ok(Some(Self {
            layout,
            items,
            counts,
        }))
    }
}

#[derive(Debug)]
pub struct Preview {
    path: PathBuf,
    chunk: Option<usize>,
    pub dimensions: (u32, u32),
    pub bytes: u64,
    pub sha256: String,
}

impl Preview {
    /// Modification time of the file that contains this preview.
    pub fn modified_millis(&self) -> Option<i64> {
        fs::modified_millis(&self.path)
    }

    /// Re-read the certified bytes. A changed payload requires a fresh scan.
    pub fn read(&self) -> Result<Vec<u8>> {
        let file = fs::read(&self.path).at(&self.path)?;
        let bytes = if let Some(index) = self.chunk {
            let (levels, _) = lrprev::Lrprev::parse_prefix(&file);
            levels
                .levels()
                .get(index)
                .ok_or(Error::PreviewUnavailable)?
                .data
                .to_vec()
        } else {
            file
        };
        if !self.matches(&bytes) {
            return Err(Error::PreviewUnavailable);
        }
        Ok(bytes)
    }

    pub fn matches(&self, bytes: &[u8]) -> bool {
        bytes.len() as u64 == self.bytes && hex(&Sha256::digest(bytes)) == self.sha256
    }
}

#[derive(Default)]
struct Selection {
    best: Option<Preview>,
    mismatch: bool,
    invalid: bool,
    unsupported: bool,
    unavailable_area: u64,
    missing_level_rows: Vec<(u32, u64)>,
    container_level_rows: Vec<u32>,
}

fn area((width, height): (u32, u32)) -> u64 {
    u64::from(width) * u64::from(height)
}

impl Selection {
    fn bytes(&mut self, path: &Path) -> Option<Vec<u8>> {
        match fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(_) => {
                self.invalid = true;
                None
            }
        }
    }

    fn inspect_level(&mut self, level: previews_db::Candidate<'_>) {
        let expected = level.row.map(|row| (row.width, row.height));
        let Some(bytes) = self.bytes(&level.path) else {
            self.missing_level(level);
            return;
        };
        self.inspect_jpeg(&level.path, None, &bytes, expected, Some(level.long_edge));
    }

    fn missing_level(&mut self, level: previews_db::Candidate<'_>) {
        if let Some(row) = level.row {
            self.missing_level_rows
                .push((row.level, area((row.width, row.height))));
        }
    }

    fn inspect_container(&mut self, path: &Path, rows: &[crate::model::PyramidLevel]) {
        let Some(bytes) = self.bytes(path) else {
            return;
        };
        if jpeg::is_other_image(&bytes) {
            self.unsupported = true;
            return;
        }
        let (container, parsed) = lrprev::Lrprev::parse_prefix(&bytes);
        let levels = container.levels();
        if parsed.is_err() || levels.is_empty() {
            self.invalid = true;
        }
        for (index, chunk) in levels.into_iter().enumerate() {
            let row = lrprev::level_index(&chunk.name)
                .and_then(|number| rows.iter().find(|row| row.level == number));
            let expected = row.map(|row| (row.width, row.height));
            if self.inspect_jpeg(path, Some(index), chunk.data, expected, None)
                && let Some(row) = row
            {
                self.container_level_rows.push(row.level);
            }
        }
    }

    fn inspect_jpeg(
        &mut self,
        path: &Path,
        chunk: Option<usize>,
        bytes: &[u8],
        expected: Option<(u32, u32)>,
        long_edge: Option<u32>,
    ) -> bool {
        let certified = jpeg::certify_recovery(bytes).ok();
        let dimensions = certified.as_ref().map(|frame| frame.dimensions);
        let Some(dimensions) = dimensions.filter(|&size| {
            area(size) > 0 && certified.as_ref().is_some_and(|frame| frame.complete)
        }) else {
            if jpeg::is_other_image(bytes) {
                self.unsupported = true;
            } else {
                self.invalid = true;
            }
            self.unavailable_area = self
                .unavailable_area
                .max(expected.map_or(0, area))
                .max(dimensions.map_or(0, area));
            return false;
        };
        if self
            .best
            .as_ref()
            .is_some_and(|best| area(best.dimensions) >= area(dimensions))
        {
            return true;
        }
        self.mismatch = expected
            .filter(|&size| area(size) > 0)
            .is_some_and(|(width, height)| {
                dimensions != (width, height) && dimensions != (height, width)
            })
            || long_edge.is_some_and(|edge| edge != dimensions.0.max(dimensions.1));
        self.best = Some(Preview {
            path: path.to_path_buf(),
            chunk,
            dimensions,
            bytes: bytes.len() as u64,
            sha256: hex(&Sha256::digest(bytes)),
        });
        true
    }

    fn finish(self, key: &PyramidKey) -> Item {
        let status = match (&self.best, self.invalid, self.unsupported) {
            (Some(_), _, _) => Status::Recoverable,
            (_, true, _) => Status::Invalid,
            (_, _, true) => Status::Unsupported,
            _ => Status::Missing,
        };
        let mut hash = Sha256::new();
        hash.update(key.uuid());
        hash.update([0]);
        hash.update(key.digest());
        // An indexed level found in a container is available even though
        // the corresponding modern path does not exist.
        let unavailable_area = self
            .missing_level_rows
            .iter()
            .filter(|(level, _)| !self.container_level_rows.contains(level))
            .map(|(_, area)| *area)
            .fold(self.unavailable_area, u64::max);
        Item {
            filename: format!("preview-{}.jpg", hex(&hash.finalize())),
            status,
            fallback: self
                .best
                .as_ref()
                .is_some_and(|best| unavailable_area > area(best.dimensions)),
            dimension_mismatch: self.mismatch,
            preview: self.best,
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
