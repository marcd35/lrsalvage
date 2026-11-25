//! Plan projections for browser review and export.

use crate::cache_review::CacheReview;
use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use lrsalvage_core::model::{CatalogImage, XmpMetadata};
use lrsalvage_core::plan::{Options, Plan, PlanItem, Status};
use lrsalvage_core::snapshot::SourceDigests;
use lrsalvage_core::{Error, Result, VERSION, exif, extract, report};
use serde_json::{Value, json};

/// The open scan's payload choices and cached export layouts.
#[derive(Default)]
pub struct Review {
    cache: Option<CacheReview>,
    plan: Plan,
    /// The knobs this run planned with, kept because a [`Plan`] records what the
    /// run decided and the verification report echoes what shaped it.
    options: Options,
    /// The pair this run's copy proved, or `None` for a snapshot taken without
    /// the proof. Retained as a value rather than as two strings: the report has
    /// to be handed the thing that measured it.
    digests: Option<SourceDigests>,
    layouts: [OnceCell<Vec<Option<PathBuf>>>; 2],
    by_id: HashMap<i64, usize>,
}

impl Review {
    pub(crate) fn new(plan: Plan, options: Options, digests: Option<SourceDigests>) -> Self {
        let by_id = plan
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| item.image.as_ref().map(|image| (image.id_local, index)))
            .collect();
        Self {
            cache: None,
            plan,
            options,
            digests,
            by_id,
            layouts: Default::default(),
        }
    }

    pub(crate) fn from_cache(cache: CacheReview) -> Self {
        Self {
            cache: Some(cache),
            ..Self::default()
        }
    }

    pub(crate) fn rows(&self) -> Value {
        if let Some(cache) = &self.cache {
            return cache.rows();
        }
        Value::Array(self.plan.items.iter().filter_map(row).collect())
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn modified_millis(&self, id: i64) -> Option<i64> {
        if let Some(cache) = &self.cache {
            return cache.modified_millis(id);
        }
        let item = &self.plan.items[*self.by_id.get(&id)?];
        let payload = item.payload.as_ref().filter(|_| has_preview(item))?;
        lrsalvage_core::fs::modified_millis(payload.path())
    }

    fn paths(&self, flat: bool) -> &[Option<PathBuf>] {
        self.layouts[usize::from(flat)].get_or_init(|| {
            self.plan.export_paths(
                Path::new(crate::OUTPUT_ROOT),
                flat,
                self.options.min_long_edge,
            )
        })
    }

    /// The selected projection of either layout is rendered by the page.
    pub fn export_paths(&self, flat: bool) -> Value {
        if let Some(cache) = &self.cache {
            return cache.paths();
        }
        Value::Array(
            self.plan
                .items
                .iter()
                .zip(self.paths(flat))
                .filter_map(|(item, path)| {
                    let (image, path) = (item.image.as_ref()?, path.as_deref()?);
                    if !has_preview(item) {
                        return None;
                    }
                    Some(json!({
                        "id": image.id_local.to_string(),
                        "path": path.strip_prefix(crate::OUTPUT_ROOT).ok()?,
                        "bytes": item.bytes,
                    }))
                })
                .collect(),
        )
    }

    fn export_item(&self, id: i64, flat: bool) -> Option<(&PlanItem, &Path)> {
        let index = *self.by_id.get(&id)?;
        let item = &self.plan.items[index];
        let path = self.paths(flat)[index].as_deref()?;
        has_preview(item).then_some((item, path))
    }

    /// Prepare the JPEG and its prospective outcome for either save action.
    pub fn prepare(
        &self,
        id: i64,
        flat: bool,
    ) -> std::result::Result<(Vec<u8>, extract::Outcome), String> {
        if let Some(cache) = &self.cache {
            return cache
                .prepare(id)
                .map_err(|_| Error::PreviewUnavailable.to_string());
        }
        let (item, path) = self
            .export_item(id, flat)
            .ok_or_else(|| Error::PreviewUnavailable.to_string())?;
        let jpeg = extract::prepare_jpeg(item, &extract::ExtractOptions::default())?;
        let outcome = jpeg.written(path);
        Ok((jpeg.bytes, outcome))
    }

    /// Recognize an existing output using the scan's certified size.
    pub fn existing(&self, id: i64, flat: bool, bytes: &[u8]) -> Result<Option<extract::Outcome>> {
        if let Some(cache) = &self.cache {
            return cache.existing(id, bytes);
        }
        Ok(self.export_item(id, flat).and_then(|(item, path)| {
            extract::existing_outcome(path, bytes, item.width.zip(item.height)?)
        }))
    }

    /// Use the same report encoders as the CLI, over the batch's fixed selection.
    ///
    /// `report.csv` is the scan's own plan, as the CLI's is; `extract.csv` and the
    /// manifest describe the selection this batch committed. Three files, one
    /// encoder each — the page writes the text and owns the file names.
    pub fn reports(&self, flat: bool, outcomes: Vec<extract::Outcome>, now: u64) -> Value {
        self.reports_with_renames(flat, outcomes, &HashMap::new(), now)
    }

    pub(crate) fn reports_with_renames(
        &self,
        flat: bool,
        mut outcomes: Vec<extract::Outcome>,
        renamed: &HashMap<PathBuf, PathBuf>,
        now: u64,
    ) -> Value {
        if let Some(cache) = &self.cache {
            return cache.reports(&outcomes, now);
        }
        for outcome in &mut outcomes {
            if outcome.output.is_relative() {
                outcome.output = Path::new(crate::OUTPUT_ROOT).join(&outcome.output);
            }
        }
        let selected: HashSet<_> = outcomes
            .iter()
            .map(|outcome| outcome.output.as_path())
            .collect();
        let root = Path::new(crate::OUTPUT_ROOT);
        let paths: Vec<_> = self
            .paths(flat)
            .iter()
            .map(|path| {
                path.as_ref().map(|path| {
                    path.strip_prefix(root)
                        .ok()
                        .and_then(|relative| renamed.get(relative))
                        .map_or_else(|| path.clone(), |actual| root.join(actual))
                })
            })
            .collect();
        let plan = self.plan.export_report(root, &paths, &selected, flat);
        json!({
            "report.csv": report::csv(&self.plan),
            "extract.csv": extract::outcomes_csv(&outcomes),
            "manifest.json": report::manifest_json(&plan, VERSION, now),
        })
    }

    /// The path-free verification report, on the host's clock.
    ///
    /// The same object `scan --report` writes, from the same three facts: this
    /// run's plan, the options it applied, and the digest pair its copy proved.
    /// A cache-only run measures none of those, so it publishes its own frozen
    /// member set and takes the batch's counts from the page, which is the only
    /// place the destination was seen.
    pub fn verification(&self, writes: report::CacheBatch, now: u64) -> String {
        if let Some(cache) = &self.cache {
            return cache.verification(writes, now);
        }
        report::verification_json(
            &self.plan,
            &self.options,
            self.digests.as_ref(),
            VERSION,
            now,
        )
    }

    /// Read the preview for a catalog image, using its certified payload choice.
    ///
    /// The browser applies any embedded EXIF orientation. Add the catalog
    /// fallback as EXIF when needed, without touching the compressed image.
    pub fn read(&self, id: i64) -> Result<Vec<u8>> {
        if let Some(cache) = &self.cache {
            return cache.read(id);
        }
        let item = self
            .by_id
            .get(&id)
            .map(|&index| &self.plan.items[index])
            .filter(|item| has_preview(item))
            .ok_or(Error::PreviewUnavailable)?;
        let preview = extract::read_preview(item)?;
        if preview.orientation.is_some() || item.effective_orientation.is_upright() {
            return Ok(preview.bytes);
        }
        let orientation = exif::from_catalog(&XmpMetadata::default(), item.effective_orientation);
        exif::inject(&preview.bytes, &orientation)
    }
}

fn has_preview(item: &PlanItem) -> bool {
    matches!(item.status, Status::Recoverable | Status::Suspect)
}

fn row(item: &PlanItem) -> Option<Value> {
    let image = item.image.as_ref()?;
    let available = has_preview(item);
    Some(json!({
        "id": image.id_local.to_string(),
        "name": name(image),
        "folder": folder(image),
        "status": item.status.as_str(),
        "preview": available,
        "checked": available && item.selected,
        "size": item.width.zip(item.height),
        "captured": image.xmp.as_ref().and_then(|xmp| xmp.capture_time.as_deref()),
        "declared": item.declared,
        "rotation": item.effective_orientation.degrees(),
    }))
}

fn name(image: &CatalogImage) -> String {
    let mut name = if image.base_name.is_empty() {
        format!("Photo {}", image.id_local)
    } else if image.extension.is_empty() {
        image.base_name.clone()
    } else {
        format!("{}.{}", image.base_name, image.extension)
    };
    if let Some(copy) = image.copy_name.as_deref().filter(|copy| !copy.is_empty()) {
        name.push_str(" / ");
        name.push_str(copy);
    }
    name
}

fn folder(image: &CatalogImage) -> Option<String> {
    if image.root_folder.is_empty() {
        return None;
    }
    let root = image.root_folder.replace('\\', "/");
    let mut folder = root.trim_end_matches('/').to_owned();
    for component in image
        .path_from_root
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
    {
        folder.push('/');
        folder.push_str(component);
    }
    if folder.is_empty() || (folder.ends_with(':') && root.ends_with('/')) {
        folder.push('/');
    }
    Some(folder)
}
