//! Plan projections for browser review and export.

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use lrsalvage_core::model::{CatalogImage, XmpMetadata};
use lrsalvage_core::plan::{Options, Plan, PlanItem, Status};
use lrsalvage_core::{Error, Result, VERSION, exif, extract, report};
use serde_json::{Value, json};

/// The open scan's payload choices and cached export layouts.
#[derive(Default)]
pub struct Review {
    plan: Plan,
    layouts: [OnceCell<Vec<Option<PathBuf>>>; 2],
    by_id: HashMap<i64, usize>,
}

impl Review {
    pub(crate) fn new(plan: Plan) -> Self {
        let by_id = plan
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| item.image.as_ref().map(|image| (image.id_local, index)))
            .collect();
        Self {
            plan,
            by_id,
            layouts: Default::default(),
        }
    }

    pub(crate) fn rows(&self) -> Value {
        Value::Array(self.plan.items.iter().filter_map(row).collect())
    }

    fn paths(&self, flat: bool) -> &[Option<PathBuf>] {
        self.layouts[usize::from(flat)].get_or_init(|| {
            self.plan.export_paths(
                Path::new(crate::OUTPUT_ROOT),
                flat,
                Options::default().min_long_edge,
            )
        })
    }

    /// The selected projection of either layout is rendered by the page.
    pub fn export_paths(&self, flat: bool) -> Value {
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

    /// Prepare the JPEG with the core marker and metadata for either save action.
    pub fn prepare(
        &self,
        id: i64,
        flat: bool,
    ) -> std::result::Result<(extract::PreparedJpeg, &Path), String> {
        let (item, path) = self
            .export_item(id, flat)
            .ok_or_else(|| Error::PreviewUnavailable.to_string())?;
        let jpeg = extract::prepare_jpeg(item, &extract::ExtractOptions::default())?;
        Ok((jpeg, path))
    }

    /// Recognize an existing output using the scan's certified size.
    pub fn existing(&self, id: i64, flat: bool, bytes: &[u8]) -> Option<extract::Outcome> {
        let (item, path) = self.export_item(id, flat)?;
        extract::existing_outcome(path, bytes, item.width.zip(item.height)?)
    }

    /// Use the same report encoders as the CLI, over the batch's fixed selection.
    pub fn reports(&self, flat: bool, mut outcomes: Vec<extract::Outcome>, now: u64) -> Value {
        for outcome in &mut outcomes {
            if outcome.output.is_relative() {
                outcome.output = Path::new(crate::OUTPUT_ROOT).join(&outcome.output);
            }
        }
        let selected: HashSet<_> = outcomes
            .iter()
            .map(|outcome| outcome.output.as_path())
            .collect();
        let plan = self.plan.export_report(
            Path::new(crate::OUTPUT_ROOT),
            self.paths(flat),
            &selected,
            flat,
        );
        json!({
            "extract.csv": extract::outcomes_csv(&outcomes),
            "manifest.json": report::manifest_json(&plan, VERSION, now),
        })
    }

    /// Read the preview for a catalog image, using its certified payload choice.
    ///
    /// The browser applies any embedded EXIF orientation. Add the catalog
    /// fallback as EXIF when needed, without touching the compressed image.
    pub fn read(&self, id: i64) -> Result<Vec<u8>> {
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
