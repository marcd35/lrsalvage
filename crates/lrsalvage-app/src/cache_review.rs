//! Cache-only projections for the existing review and destination writer.

use std::fmt::Write;
use std::path::Path;

use lrsalvage_core::cache_recovery::{CacheRecovery, Item, Preview};
use lrsalvage_core::extract::{self, Outcome, OutcomeStatus};
use lrsalvage_core::{Error, Result, VERSION};
use serde_json::{Value, json};

pub(crate) struct CacheReview(pub(crate) CacheRecovery);

impl CacheReview {
    pub(crate) fn summary(&self) -> Value {
        json!({
            "mode": "cache-only",
            "layout": self.0.layout.as_str(),
            "scan": self.0.counts,
        })
    }

    pub(crate) fn rows(&self) -> Value {
        Value::Array(
            self.0
                .items
                .iter()
                .enumerate()
                .filter_map(|(id, item)| {
                    let preview = item.preview.as_ref()?;
                    Some(json!({
                        "id": id.to_string(),
                        "name": item.filename,
                        "folder": null,
                        "status": item.status,
                        "preview": true,
                        "checked": true,
                        "size": preview.dimensions,
                        "fallback": item.fallback,
                        "dimension_mismatch": item.dimension_mismatch,
                    }))
                })
                .collect(),
        )
    }

    fn preview(&self, id: i64) -> Result<(&Item, &Preview)> {
        let item = usize::try_from(id)
            .ok()
            .and_then(|index| self.0.items.get(index))
            .ok_or(Error::PreviewUnavailable)?;
        Ok((
            item,
            item.preview.as_ref().ok_or(Error::PreviewUnavailable)?,
        ))
    }

    pub(crate) fn paths(&self) -> Value {
        Value::Array(
            self.0
                .items
                .iter()
                .enumerate()
                .filter_map(|(id, item)| {
                    Some(json!({
                        "id": id.to_string(),
                        "path": item.filename,
                        "bytes": item.preview.as_ref()?.bytes,
                    }))
                })
                .collect(),
        )
    }

    pub(crate) fn read(&self, id: i64) -> Result<Vec<u8>> {
        self.preview(id)?.1.read()
    }

    pub(crate) fn prepare(&self, id: i64) -> Result<(Vec<u8>, Outcome)> {
        let (item, preview) = self.preview(id)?;
        Ok((
            preview.read()?,
            outcome(item, preview, OutcomeStatus::Written),
        ))
    }

    pub(crate) fn existing(&self, id: i64, bytes: &[u8]) -> Result<Option<Outcome>> {
        let (item, preview) = self.preview(id)?;
        if !preview.matches(bytes) || preview.read()? != bytes {
            return Ok(None);
        }
        Ok(Some(outcome(item, preview, OutcomeStatus::Skipped)))
    }

    pub(crate) fn reports(&self, outcomes: &[Outcome], now: u64) -> Value {
        let mut scan_csv =
            String::from("mode,filename,status,width,height,fallback,dimension_mismatch\r\n");
        for item in &self.0.items {
            let (width, height) = item.preview.as_ref().map_or_else(
                || (String::new(), String::new()),
                |preview| {
                    (
                        preview.dimensions.0.to_string(),
                        preview.dimensions.1.to_string(),
                    )
                },
            );
            let _ = write!(
                scan_csv,
                "cache-only,{},{},{width},{height},{},{}\r\n",
                item.filename,
                item.status.as_str(),
                item.fallback,
                item.dimension_mismatch
            );
        }
        let mut manifest = self.summary();
        manifest["version"] = json!(VERSION);
        manifest["generated_unix"] = json!(now);
        manifest["outputs"] = json!(outcomes);
        manifest["batch"] = json!({
            "saved": outcomes.iter().filter(|item| item.status == OutcomeStatus::Written).count(),
            "already_recovered": outcomes.iter().filter(|item| item.status == OutcomeStatus::Skipped).count(),
            "failed": outcomes.iter().filter(|item| item.status == OutcomeStatus::Failed).count(),
        });
        json!({
            "report.csv": scan_csv,
            "extract.csv": extract::cache_only_outcomes_csv(outcomes),
            "manifest.json": serde_json::to_string_pretty(&manifest).expect("serializable manifest"),
        })
    }

    pub(crate) fn verification(&self, now: u64) -> String {
        let mut report = self.summary();
        report["version"] = json!(VERSION);
        report["generated_unix"] = json!(now);
        serde_json::to_string_pretty(&report).expect("serializable verification report")
    }
}

fn outcome(item: &Item, preview: &Preview, status: OutcomeStatus) -> Outcome {
    Outcome {
        output: Path::new(&item.filename).to_path_buf(),
        status,
        bytes: preview.bytes,
        sha256: Some(preview.sha256.clone()),
        note: None,
    }
}
