//! The browser app's one entry: a source in, a scan's report out.
//!
//! `0.1.0` is a static page (#117, ADR 0012 D10), and this crate is that page's
//! Rust half: the `cdylib` the page loads and the module a native test drives.
//! It owns no format and no decision — [`scan`] composes `lrsalvage_core`'s own
//! `snapshot`, `catalog`, `previews_db` and `plan` in the order #149 recorded,
//! and turns their answers into the JSON the page renders. Everything a user
//! could disagree with is in a module with a spec; everything here is plumbing.
//!
//! The order is the entry decision (`#117`, *Entry and review*), and it is why
//! the page cannot reorder it:
//!
//! 1. **Copy** the catalog and its sidecars into the origin-private pool and
//!    checkpoint them (`snapshot::capture_verified`) — the private copy *is* the
//!    snapshot, so `--verify-source`'s two digests are the same two digests.
//! 2. **Open** that copy. A copy that is not the catalog is a refusal, never a
//!    warning.
//! 3. **Index**: copy `previews.db` into the pool too, because SQLite can only
//!    open what the pool holds, and let `previews_db::open` degrade with a
//!    reason rather than refuse (an absent index is a mode —
//!    `adr-0006-degrade-without-guessing`).
//! 4. **Scan**: one reported build, stoppable at a unit boundary.
//!
//! The scan writes only to the pool. Review retains the plan for preview reads
//! and for export through the shared core.

use std::path::{Path, PathBuf};

use lrsalvage_core::catalog::Catalog;
use lrsalvage_core::fs;
use lrsalvage_core::plan::{Control, Options, Plan, Progress};
use lrsalvage_core::previews_db::{self, IndexProblem};
use lrsalvage_core::snapshot::CatalogSnapshot;
use serde_json::{Value, json};

mod cache_review;
mod review;

pub use review::Review;

/// The app line's version, compiled in rather than read from a file.
///
/// ADR 0012 D2 puts this number and the core's on the receipt and in the About
/// panel, so the page has to read both from the module it loaded: a `build.json`
/// is a deployment artefact, and a tree that never ran the static builder has
/// none (`specs/app.md`, *Receipt, reports and diagnostics*).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(not(target_arch = "wasm32"))]
pub mod vault;
#[cfg(target_arch = "wasm32")]
pub mod wasm;

/// Virtual output root. The browser commits only relative paths against a
/// validated destination handle. No scan mount answers for this root.
const OUTPUT_ROOT: &str = "/destination";

/// The step a failure happened in, so a page can say which one.
///
/// A page has to tell "the copy did not fit" from "that file is not a catalog",
/// and those are the same `error::Error` shape to this crate; the stage is what
/// the caller knows and the error does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// The catalog and its sidecars, into the private pool.
    Copy,
    /// Opening the private copy.
    Open,
    /// Copying the index into the pool, and reading it.
    Index,
    /// Opening the required index for cache-only recovery.
    CacheIndex,
    /// Walking the preview cache to record the exact paths it holds.
    CacheWalk,
    /// The reported build over the catalog and the cache.
    Scan,
}

impl Stage {
    /// The word this stage is reported as.
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Copy => "copy",
            Stage::Open => "open",
            Stage::Index => "index",
            Stage::CacheIndex => "cache-index",
            Stage::CacheWalk => "cache-walk",
            Stage::Scan => "scan",
        }
    }
}

/// Why a scan did not finish, in terms a page can render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub stage: Stage,
    /// A stable word for the class of failure — `not-a-catalog`, `out-of-room`,
    /// `catalog-changed`, `unreadable`, `pool-refused`, `io`, `other`. The page
    /// maps these to sentences; the message travels beside it so a class nobody
    /// anticipated still says something true.
    pub kind: &'static str,
    pub message: String,
}

impl Failure {
    /// Classify one `error::Error` as the failure of `stage`.
    ///
    /// The classes are the distinctions a person can act on, not every variant:
    /// a catalog that is not a catalog, one that changed under the copy, a store
    /// that refused the copy for space, and everything else, which the message
    /// is left to describe (`#149`, *Decisions*).
    pub fn of(stage: Stage, error: &lrsalvage_core::Error) -> Self {
        use lrsalvage_core::Error;
        let kind = match error {
            Error::RequiredPreviewIndex { kind, .. } => kind.as_str(),
            Error::NotACatalog { .. } => "not-a-catalog",
            Error::SourceChanged { .. } => "catalog-changed",
            Error::Sqlite(_) => "unreadable",
            Error::Io { source, .. } => match source.kind() {
                std::io::ErrorKind::StorageFull => "out-of-room",
                std::io::ErrorKind::NotFound => "not-found",
                std::io::ErrorKind::Unsupported => "unsupported",
                std::io::ErrorKind::WouldBlock => "busy",
                _ => "io",
            },
            _ => "other",
        };
        Self {
            stage,
            kind,
            // The whole chain: a pool's message is the only description of some
            // failures, and the path context is what a person needs to act.
            message: format!("{error}"),
        }
    }

    /// This failure as the JSON the page reads.
    pub fn to_json(&self) -> Value {
        json!({
            "stage": self.stage.as_str(),
            "kind": self.kind,
            "message": self.message,
        })
    }
}

/// One scan of one source: the report a page renders, or the failure to explain.
///
/// The three paths are mount-relative spellings on `wasm32` and ordinary paths
/// natively, which is what makes the same call run in both places.
pub fn scan(
    catalog: &Path,
    cache: &Path,
    progress: &mut dyn FnMut(Progress) -> Control,
) -> Result<Value, Failure> {
    scan_for_review(catalog, cache, progress).map(|completed| completed.report)
}

/// The scan report and the certified payload choices for the open recovery.
pub struct ScanResult {
    pub report: Value,
    pub review: Review,
}

/// Run the same scan and retain its preview choices for review and export.
pub fn scan_for_review(
    catalog: &Path,
    cache: &Path,
    progress: &mut dyn FnMut(Progress) -> Control,
) -> Result<ScanResult, Failure> {
    // 1. The catalog and its `-wal`/`-shm`, into the store, checkpointed. One
    //    call: a copy is not something a page can stop halfway through, which is
    //    why the page names this stage instead of drawing a bar for it.
    let snapshot = CatalogSnapshot::capture_verified(catalog)
        .map_err(|error| Failure::of(Stage::Copy, &error))?;
    // The pair is retained as a value, not only as two strings for the report:
    // the verification report has to hand `report` the digests this copy proved,
    // and `SourceDigests` is the only thing that can say "a verified acquisition
    // produced these" (`specs/modules/report.md`).
    let digests = snapshot.digests().cloned();
    let digest_strings = digests
        .as_ref()
        .map(|digests| (digests.source().to_string(), digests.snapshot().to_string()));

    // 2. The private copy, read-only and immutable now that its WAL is folded in.
    let catalog_db =
        Catalog::from_snapshot(&snapshot).map_err(|error| Failure::of(Stage::Open, &error))?;

    // 3. The index, copied where SQLite can open it. `open` answers a missing
    //    file with its own reason, so an absent index is copied by not being
    //    copied and the plan then reports every image as `no-cache-entry`.
    let index_dir = fs::scratch_dir("lrsalvage-app-index-")
        .map_err(|source| Failure::of(Stage::Index, &io_without_path(source)))?;
    let index_copy = index_dir.path().join(previews_db::INDEX_FILE);
    let index_present = match fs::copy(&cache.join(previews_db::INDEX_FILE), &index_copy) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(source) => {
            return Err(Failure::of(
                Stage::Index,
                &lrsalvage_core::Error::Io {
                    path: index_copy,
                    source,
                },
            ));
        }
    };
    let opened = previews_db::open(&index_copy);

    // 4. The build. `None` is a stop, and a stop goes back to source selection
    //    rather than presenting a plan about the images it did not reach. The
    //    options are a value this crate keeps, because the verification report
    //    echoes the knobs a run applied and a plan does not record them.
    let options = Options::default();
    let built = Plan::build_reported(
        &catalog_db,
        &opened.index,
        cache,
        Path::new(OUTPUT_ROOT),
        &options,
        progress,
    )
    .map_err(|error| Failure::of(Stage::Scan, &error))?;

    let Some(plan) = built else {
        return Ok(ScanResult {
            report: json!({ "ok": true, "stopped": true }),
            review: Review::default(),
        });
    };
    let mut report = report(
        &plan,
        &opened.problem,
        index_present,
        digest_strings,
        &catalog_db,
    );
    let review = Review::new(plan, options, digests);
    report["photos"] = review.rows();
    Ok(ScanResult { report, review })
}

/// Open a preview cache directly, with the same review and writer protocol as
/// the catalog-backed entry point.
pub fn scan_cache(
    cache: &Path,
    progress: &mut dyn FnMut(Progress) -> Control,
) -> Result<ScanResult, Failure> {
    let plan =
        lrsalvage_core::cache_recovery::CacheRecovery::scan(cache, progress).map_err(|error| {
            let stage = if matches!(error, lrsalvage_core::Error::RequiredPreviewIndex { .. }) {
                Stage::CacheIndex
            } else {
                Stage::CacheWalk
            };
            Failure::of(stage, &error)
        })?;
    let Some(plan) = plan else {
        return Ok(ScanResult {
            report: json!({ "ok": true, "stopped": true }),
            review: Review::default(),
        });
    };
    let cache = cache_review::CacheReview(plan);
    let mut report = cache.summary();
    let review = Review::from_cache(cache);
    report["ok"] = json!(true);
    report["stopped"] = json!(false);
    report["photos"] = review.rows();
    Ok(ScanResult { report, review })
}

/// The conditions and counts one finished scan reports.
///
/// Every number here was observed by the module that owns it; the page renders
/// them and invents none (`#117` user stories 4 and 5).
fn report(
    plan: &Plan,
    problem: &Option<IndexProblem>,
    index_present: bool,
    digests: Option<(String, String)>,
    catalog: &Catalog,
) -> Value {
    let stats = &plan.stats;
    let cache = &plan.cache;
    json!({
        "ok": true,
        "stopped": false,
        "source_sha256": digests.as_ref().map(|(source, _)| source.clone()),
        "snapshot_sha256": digests.as_ref().map(|(_, snapshot)| snapshot.clone()),
        "index": {
            "present": index_present,
            "problem": match problem {
                None => Value::Null,
                Some(IndexProblem::Missing) => json!({ "kind": "missing" }),
                Some(IndexProblem::Unreadable(text)) => json!({ "kind": "unreadable", "text": text }),
            },
        },
        "cache": {
            "layout": cache.layout.as_str(),
            "db_version": cache.db_version,
            "crs_version": cache.crs_version,
            "shapes": {
                "level_file": cache.shapes.level_file,
                "suffix_matches_sof": cache.shapes.suffix_matches_sof,
                "container": cache.shapes.container,
            },
            "thumbnail_pyramids": cache.thumbnail_pyramids,
        },
        "stats": {
            "images": stats.images,
            "recovered": stats.recoverable,
            "tiers": lrsalvage_core::report::tiers_json(stats),
            "missing_payload": stats.missing_payload,
            "no_cache_entry": stats.no_cache_entry,
            "unmapped": stats.unmapped,
            "suspect": stats.suspect,
            "failed": stats.failed,
            "original_present": stats.original_present,
            "below_floor": stats.below_floor,
            "above_cap": stats.above_cap,
            "levels": stats.levels,
            "selected_items": stats.selected_items,
            "selected_images": stats.selected_images,
            "superseded": stats.superseded,
            "deleted": stats.deleted,
            "unrecognised": stats.unrecognised,
            "bytes": stats.bytes,
        },
        "roots": roots(catalog),
    })
}

/// The catalog's recorded root folders, each with whether this page can reach it.
///
/// Reachable means the seam answers for the recorded spelling: a mount covers it
/// and something is under it. A drive that is not connected, and a folder this
/// page was never given, are the same answer — `false` — and that is what makes
/// "check again" meaningful: mount the folder and ask the same question again
/// (`#149`, *Decisions*).
fn roots(catalog: &Catalog) -> Value {
    let roots = catalog.root_folders().unwrap_or_default();
    Value::Array(
        roots
            .into_iter()
            .map(|root| {
                let reachable = fs::exists(Path::new(&root.path)).unwrap_or(false);
                json!({
                    "path": root.path,
                    // A path that no mount covers cannot be asked about at all;
                    // the page says so instead of pretending the folder is empty.
                    "images": root.images,
                    "in_reach": reachable,
                })
            })
            .collect(),
    )
}

/// An `io::Error` that arrived without a path, as the path-carrying variant.
///
/// The seam's own errors are bare `io::Error`s (`specs/modules/fs.md`), and this
/// crate is what attaches the path a page should have named.
fn io_without_path(source: std::io::Error) -> lrsalvage_core::Error {
    lrsalvage_core::Error::Io {
        path: PathBuf::from(OUTPUT_ROOT),
        source,
    }
}
