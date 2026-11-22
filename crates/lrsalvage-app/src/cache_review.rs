//! Cache-only projections for the existing review and destination writer.
//!
//! The three published files are `report`'s to write, as the catalog run's are:
//! this module hands it the scan and the batch's counts and keeps the page-side
//! projections (`summary`, `rows`) that no file shape freezes.

use std::path::Path;

use lrsalvage_core::cache_recovery::{CacheRecovery, Item, Preview};
use lrsalvage_core::extract::{self, Outcome, OutcomeStatus};
use lrsalvage_core::report::{self, CacheBatch};
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

    /// The three published files, from `report`'s encoders.
    ///
    /// `report.csv` is the scan: one row per indexed pyramid. `extract.csv` and
    /// the manifest describe the batch that was committed, so their counts are
    /// the outcomes' — the same three files, one encoder each, that a
    /// catalog-backed run writes (`specs/modules/report.md`).
    pub(crate) fn reports(&self, outcomes: &[Outcome], now: u64) -> Value {
        let batch = batch(outcomes, None);
        json!({
            "report.csv": report::cache_csv(&self.0),
            "extract.csv": extract::cache_only_outcomes_csv(outcomes),
            "manifest.json": report::cache_manifest_json(
                &self.0,
                &json!(outcomes),
                batch,
                VERSION,
                now,
            ),
        })
    }

    /// The path-free verification report, on the host's clock.
    ///
    /// `writes` is the batch's own account, which the page measured: this module
    /// never sees the destination, so the counts arrive with the request rather
    /// than being guessed here.
    pub(crate) fn verification(&self, writes: CacheBatch, now: u64) -> String {
        report::cache_verification_json(&self.0, writes, VERSION, now)
    }
}

/// The batch's counts, from the outcomes the page prepared.
fn batch(outcomes: &[Outcome], confirmed_on_disk: Option<usize>) -> CacheBatch {
    let count =
        |status: OutcomeStatus| outcomes.iter().filter(|item| item.status == status).count();
    CacheBatch {
        saved: count(OutcomeStatus::Written),
        already_recovered: count(OutcomeStatus::Skipped),
        failed: count(OutcomeStatus::Failed),
        confirmed_on_disk,
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
