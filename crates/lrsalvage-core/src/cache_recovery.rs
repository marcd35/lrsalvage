//! Plan recovery of indexed JPEGs when the catalog is unavailable.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::IoContext;
use crate::plan::{Control, Progress};
use crate::previews_db::{self, Layout, PyramidKey, RequiredIndexProblem};
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
    ) -> Result<Option<Self>, RequiredIndexProblem> {
        let index = previews_db::open_required(cache)?;
        let Some(sweep) = previews_db::sweep(cache, &mut |files| {
            progress(Progress::Cache { files }) == Control::Continue
        }) else {
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
                let found = on_disk.remove(&key);
                let mut levels = index.row_candidates(cache, &key);
                if let Some(found) = found {
                    levels.extend(
                        found
                            .levels
                            .into_iter()
                            .map(|level| previews_db::Candidate {
                                path: level.path,
                                row: None,
                                long_edge: level.long_edge,
                            }),
                    );
                }
                // Keep the row when a file was found both ways. Iteration order
                // breaks ties after size and representation.
                previews_db::order_candidates(&mut levels);
                for level in levels {
                    selection.inspect_level(level);
                }
                selection.inspect_container(&index.payload_path(cache, &key), index.levels(&key));
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
            if let Some(row) = level.row {
                self.missing_level_rows
                    .push((row.level, area((row.width, row.height))));
            }
            return;
        };
        self.inspect_jpeg(&level.path, None, &bytes, expected, Some(level.long_edge));
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
        let certified = jpeg::certify(bytes).ok();
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
