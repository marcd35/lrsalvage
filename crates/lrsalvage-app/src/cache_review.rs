//! Cache-only and indexless projections for the existing review and destination
//! writer.
//!
//! The two modes that need no catalog share a protocol and differ in what they
//! read: one indexed pyramid per JPEG, or one `.lrprev` container per JPEG. They
//! share the review rows' shape, the batch's outcomes and the three published
//! files' names, and each brings its own mode word, its own totals and (for
//! `report.csv`) its own frozen column set — because a cache-relative container
//! path is an identity the indexed mode does not have. The enum below is that one
//! difference; every method that follows branches once, at its top.
//!
//! The three published files are `report`'s to write, as the catalog run's are:
//! this module hands it the scan and the batch's counts and keeps the page-side
//! projections (`summary`, `rows`) that no file shape freezes.

use std::path::Path;

use lrsalvage_core::cache_recovery::{CacheRecovery, LegacyRecovery, Preview};
use lrsalvage_core::extract::{self, Outcome, OutcomeStatus};
use lrsalvage_core::report::{self, CacheBatch};
use lrsalvage_core::{Error, Result, VERSION};
use serde_json::{Value, json};

/// Which recovery a review came from: the index that named every pyramid, or the
/// containers a walk found with no index at all.
pub(crate) enum Recovery {
    Indexed(CacheRecovery),
    Legacy(LegacyRecovery),
}

pub(crate) struct CacheReview(pub(crate) Recovery);

impl CacheReview {
    /// The scan's own accounting, in the mode's own words.
    ///
    /// `mode` is the value the report files carry too, so the page reads the
    /// same word from the same place. A legacy run publishes no `layout`: it
    /// classifies no representation, because every candidate it reads is a
    /// `.lrprev` by definition of the walk.
    pub(crate) fn summary(&self) -> Value {
        match &self.0 {
            Recovery::Indexed(recovery) => json!({
                "mode": "cache-only",
                "layout": recovery.layout.as_str(),
                "scan": recovery.counts,
            }),
            Recovery::Legacy(recovery) => json!({
                "mode": "legacy",
                "scan": recovery.counts,
            }),
        }
    }

    /// The rows review renders: one per candidate that yielded a JPEG.
    ///
    /// A list without a folder, because neither mode has one to show. The two
    /// flags a catalog's rows carry stay absent from an indexed row's meaning
    /// here and are `false` on both modes' rows, so the page reads one shape.
    pub(crate) fn rows(&self) -> Value {
        match &self.0 {
            Recovery::Indexed(recovery) => Value::Array(
                recovery
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
            ),
            Recovery::Legacy(recovery) => Value::Array(
                recovery
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
                            "fallback": false,
                            "dimension_mismatch": false,
                        }))
                    })
                    .collect(),
            ),
        }
    }

    pub(crate) fn paths(&self) -> Value {
        match &self.0 {
            Recovery::Indexed(recovery) => Value::Array(
                recovery
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
            ),
            Recovery::Legacy(recovery) => Value::Array(
                recovery
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
            ),
        }
    }

    /// The generated name and the certified payload one row stands for.
    ///
    /// One lookup for both modes: a row's id is its index in the scan's item
    /// list, and an item can yield a JPEG only when it has one.
    fn chosen(&self, id: i64) -> Result<(&str, &Preview)> {
        let index = usize::try_from(id).map_err(|_| Error::PreviewUnavailable)?;
        match &self.0 {
            Recovery::Indexed(recovery) => {
                let item = recovery.items.get(index).ok_or(Error::PreviewUnavailable)?;
                Ok((&item.filename, available(&item.preview)?))
            }
            Recovery::Legacy(recovery) => {
                let item = recovery.items.get(index).ok_or(Error::PreviewUnavailable)?;
                Ok((&item.filename, available(&item.preview)?))
            }
        }
    }

    pub(crate) fn read(&self, id: i64) -> Result<Vec<u8>> {
        self.chosen(id)?.1.read()
    }

    pub(crate) fn prepare(&self, id: i64) -> Result<(Vec<u8>, Outcome)> {
        let (filename, preview) = self.chosen(id)?;
        Ok((
            preview.read()?,
            outcome(filename, preview, OutcomeStatus::Written),
        ))
    }

    pub(crate) fn existing(&self, id: i64, bytes: &[u8]) -> Result<Option<Outcome>> {
        let (filename, preview) = self.chosen(id)?;
        if !preview.matches(bytes) || preview.read()? != bytes {
            return Ok(None);
        }
        Ok(Some(outcome(filename, preview, OutcomeStatus::Skipped)))
    }

    /// The three published files, from `report`'s encoders.
    ///
    /// `report.csv` is the scan: one row per indexed pyramid, or one per
    /// container. `extract.csv` and the manifest describe the batch that was
    /// committed, so their counts are the outcomes' — the same three files, one
    /// encoder each, that a catalog-backed run writes
    /// (`specs/modules/report.md`).
    pub(crate) fn reports(&self, outcomes: &[Outcome], now: u64) -> Value {
        let batch = batch(outcomes, None);
        match &self.0 {
            Recovery::Indexed(recovery) => json!({
                "report.csv": report::cache_csv(recovery),
                "extract.csv": extract::cache_only_outcomes_csv(outcomes),
                "manifest.json": report::cache_manifest_json(
                    recovery,
                    &json!(outcomes),
                    batch,
                    VERSION,
                    now,
                ),
            }),
            Recovery::Legacy(recovery) => json!({
                "report.csv": report::legacy_csv(recovery),
                "extract.csv": extract::legacy_outcomes_csv(outcomes),
                "manifest.json": report::legacy_manifest_json(
                    recovery,
                    &json!(outcomes),
                    batch,
                    VERSION,
                    now,
                ),
            }),
        }
    }

    /// The path-free verification report, on the host's clock.
    ///
    /// `writes` is the batch's own account, which the page measured: this module
    /// never sees the destination, so the counts arrive with the request rather
    /// than being guessed here.
    pub(crate) fn verification(&self, writes: CacheBatch, now: u64) -> String {
        match &self.0 {
            Recovery::Indexed(recovery) => {
                report::cache_verification_json(recovery, writes, VERSION, now)
            }
            Recovery::Legacy(recovery) => {
                report::legacy_verification_json(recovery, writes, VERSION, now)
            }
        }
    }
}

fn available(preview: &Option<Preview>) -> Result<&Preview> {
    preview.as_ref().ok_or(Error::PreviewUnavailable)
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

fn outcome(filename: &str, preview: &Preview, status: OutcomeStatus) -> Outcome {
    Outcome {
        output: Path::new(filename).to_path_buf(),
        status,
        bytes: preview.bytes,
        sha256: Some(preview.sha256.clone()),
        note: None,
    }
}
