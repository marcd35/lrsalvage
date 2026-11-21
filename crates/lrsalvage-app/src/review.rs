//! Read-only projections of the plan for the browser review.

use std::collections::HashMap;

use lrsalvage_core::model::{CatalogImage, XmpMetadata};
use lrsalvage_core::plan::{Plan, PlanItem, Status};
use lrsalvage_core::{Error, Result, exif, extract};
use serde_json::{Value, json};

/// The plan's payload choices. Only the opaque catalog IDs cross to the page.
#[derive(Default)]
pub struct Review {
    items: Vec<PlanItem>,
    by_id: HashMap<i64, usize>,
}

impl Review {
    pub(crate) fn new(plan: Plan) -> Self {
        let items: Vec<_> = plan
            .items
            .into_iter()
            .filter(|item| item.image.is_some())
            .collect();
        let by_id = items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| item.image.as_ref().map(|image| (image.id_local, index)))
            .collect();
        Self { items, by_id }
    }

    pub(crate) fn rows(&self) -> Value {
        Value::Array(self.items.iter().filter_map(row).collect())
    }

    /// Read the preview for a catalog image, using its certified payload choice.
    ///
    /// The browser applies any embedded EXIF orientation. Add the catalog
    /// fallback as EXIF when needed, without touching the compressed image.
    pub fn read(&self, id: i64) -> Result<Vec<u8>> {
        let item = self
            .by_id
            .get(&id)
            .map(|&index| &self.items[index])
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
