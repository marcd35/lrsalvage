//! The page's entry points: install the seam, then run it.
//!
//! `wasm-bindgen --target web` turns these into the module's exports, which is
//! why this file is the only place in this crate that mentions JavaScript. The
//! worker that loads it is `web/worker.js`, and the seam's registration order is
//! `specs/modules/fs.md`'s (*The registration point*): bridge, mounts, store,
//! seal. The shim lives here rather than in `lrsalvage-web` because the app is the
//! shipped page; the parity harness links this crate and calls the same
//! functions, so the two pages cannot disagree about how the seam is installed
//! (`#149`, DEC-S5).
//!
//! Two host calls, both installed on `globalThis.lrsalvage` by the seam's worker
//! half (`web/seam/lrsalvage-fs.js`):
//!
//! * `fs_call(...)`, the seam's blocking bridge.
//! * `scan_progress(phase, done, total)`, one progress report, answered with `0`
//!   to carry on and `1` to stop. The page loads a stop flag out of a
//!   `SharedArrayBuffer` inside that call, which is the only way a control on the
//!   page can reach a scan running behind a synchronous wasm frame (`#149`,
//!   DEC-S3). The counts cross as `f64` on purpose: a `u64` argument would arrive
//!   as a `BigInt`, and no catalog has more than 2^53 images.

use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

use std::cell::RefCell;
use std::path::Path;

thread_local! {
    static REVIEW: RefCell<crate::Review> = RefCell::new(crate::Review::default());
}

use lrsalvage_core::Error;
use lrsalvage_core::plan::{Control, Progress};

/// The host's blocking seam call, from `web/seam/lrsalvage-fs.js`.
///
/// Declared as four numbers rather than pointers because that is what crosses the
/// boundary; `wasm32` is 32 bits, so a pointer fits and needs no widening.
#[wasm_bindgen]
unsafe extern "C" {
    #[wasm_bindgen(js_namespace = "lrsalvage", js_name = "fs_call")]
    fn host_fs_call(op: u32, request_ptr: u32, request_len: u32, out_ptr: u32, out_cap: u32)
    -> i32;

    /// One progress report. `0` carries on; anything else stops the build.
    #[wasm_bindgen(js_namespace = "lrsalvage", js_name = "scan_progress")]
    fn host_scan_progress(phase: u32, done: f64, total: f64) -> i32;

    #[wasm_bindgen(js_namespace = JSON, js_name = parse)]
    fn parse_json(text: &str) -> JsValue;

}

/// The three stages of a scan, as the number the page is handed.
///
/// `previews_db`'s walk, the measured pass over the cache, and the catalog's
/// images — `plan::Progress`'s own three, in its own order.
const PHASE_CACHE: u32 = 0;
const PHASE_PAYLOADS: u32 = 1;
const PHASE_IMAGES: u32 = 2;

/// The seam's shape, over the host's call.
unsafe extern "C" fn bridge(
    op: u32,
    request: *const u8,
    request_len: usize,
    out: *mut u8,
    out_cap: usize,
) -> i32 {
    // The four numbers are the seam's own buffer and the host's answer; the call
    // is `unsafe` in the seam's vocabulary because of what it may write, not
    // because the binding is.
    let _ = (request, out);
    host_fs_call(
        op,
        request as u32,
        request_len as u32,
        out as u32,
        out_cap as u32,
    )
}

/// Register the mounts and begin installing the origin-private store.
///
/// Called before every run, because a run may add a mount — an originals folder
/// the user has just reconnected (`#149`, DEC-S7) — and `reset` is what makes the
/// mount table this run's rather than the last run's.
pub fn install(mounts: &str, capacity: u32, clear: bool) -> Result<(), JsValue> {
    lrsalvage_core::fs::wasm::reset();
    lrsalvage_core::fs::wasm::install_bridge(bridge);

    let mounts: Value = serde_json::from_str(mounts)
        .map_err(|error| JsValue::from_str(&format!("mounts: {error}")))?;
    let mounts = mounts
        .as_array()
        .ok_or_else(|| JsValue::from_str("mounts: a JSON array of {path, store}"))?;
    for mount in mounts {
        let path = mount["path"]
            .as_str()
            .ok_or_else(|| JsValue::from_str("mounts: a mount without a path"))?;
        let store = mount["store"].as_bool().unwrap_or(false);
        lrsalvage_core::fs::wasm::mount(Path::new(path), store);
    }

    lrsalvage_core::fs::wasm::start_install(capacity as usize, clear);
    Ok(())
}

/// Advance the install: `"pending"`, `"done"`, or `"failed: …"`.
pub fn poll() -> String {
    match lrsalvage_core::fs::wasm::poll_install() {
        Ok(false) => "pending".to_string(),
        Ok(true) if lrsalvage_core::fs::wasm::store_installed() => {
            lrsalvage_core::fs::wasm::seal();
            "done".to_string()
        }
        Ok(true) => "failed: the store reported success and left none".to_string(),
        Err(error) => format!("failed: {error}"),
    }
}

/// The app's one entry, as the JSON the page renders.
///
/// Never a thrown value: a scan that could not happen is a report whose `ok` is
/// `false` and whose `problem` names the stage, so the page has one code path and
/// a person gets a sentence instead of an exception.
#[wasm_bindgen]
pub fn scan(catalog: &str, cache: &str) -> String {
    scan_with(|progress| {
        crate::scan_for_review(Path::new(catalog), Path::new(cache), progress)
            .map_err(|failure| failure.to_json())
    })
}

/// Scan a cache without mounting or opening a catalog.
#[wasm_bindgen]
pub fn scan_cache(cache: &str) -> String {
    scan_with(|progress| {
        crate::scan_cache(Path::new(cache), progress).map_err(|failure| failure.to_json())
    })
}

/// Scan a cache with no `previews.db`, one `.lrprev` at a time (#198).
#[wasm_bindgen]
pub fn scan_legacy(cache: &str) -> String {
    scan_with(|progress| {
        crate::scan_legacy(Path::new(cache), progress).map_err(|failure| failure.to_json())
    })
}

fn scan_with(
    run: impl FnOnce(&mut dyn FnMut(Progress) -> Control) -> Result<crate::ScanResult, Value>,
) -> String {
    let mut progress = |progress: Progress| {
        let (phase, done, total) = match progress {
            Progress::Cache { files } => (PHASE_CACHE, files, 0),
            Progress::Payloads { done, total } => (PHASE_PAYLOADS, done, total),
            Progress::Images { done, total } => (PHASE_IMAGES, done, total),
        };
        if host_scan_progress(phase, done as f64, total as f64) == 0 {
            Control::Continue
        } else {
            Control::Stop
        }
    };

    REVIEW.with(|review| review.replace(crate::Review::default()));
    match run(&mut progress) {
        Ok(result) => {
            REVIEW.with(|review| review.replace(result.review));
            result.report.to_string()
        }
        Err(problem) => json!({ "ok": false, "problem": problem }).to_string(),
    }
}

/// Release the catalog and index store before handing the plan to review.
#[wasm_bindgen]
pub fn pause_store() -> Result<(), JsValue> {
    lrsalvage_core::fs::wasm::pause_store().map_err(|error| JsValue::from_str(&error.to_string()))
}

/// Read the scan's certified payload choice by its opaque catalog ID.
#[wasm_bindgen]
pub fn read_preview(id: &str) -> Result<Vec<u8>, JsValue> {
    let id = id
        .parse()
        .map_err(|_| review_problem(Error::PreviewUnavailable))?;
    REVIEW.with(|review| review.borrow().read(id).map_err(review_problem))
}

/// Return the planned relative paths in catalog order.
#[wasm_bindgen]
pub fn export_paths(flat: bool) -> String {
    REVIEW.with(|review| review.borrow().export_paths(flat).to_string())
}

/// A JPEG and its prospective outcome. The page records it after close.
#[wasm_bindgen]
pub struct ExportJpeg {
    bytes: Vec<u8>,
    outcome: String,
}

#[wasm_bindgen]
impl ExportJpeg {
    pub fn take_bytes(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.bytes)
    }

    pub fn outcome(&self) -> String {
        self.outcome.clone()
    }
}

#[wasm_bindgen]
pub fn prepare_export(id: &str, flat: bool) -> Result<ExportJpeg, JsValue> {
    let id = id
        .parse()
        .map_err(|_| JsValue::from_str("This preview is unavailable."))?;
    REVIEW.with(|review| {
        let review = review.borrow();
        let (bytes, outcome) = review
            .prepare(id, flat)
            .map_err(|error| JsValue::from_str(&error))?;
        let outcome = serde_json::to_string(&outcome)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        Ok(ExportJpeg { bytes, outcome })
    })
}

#[wasm_bindgen]
pub fn existing_export(id: &str, flat: bool, bytes: &[u8]) -> Result<String, JsValue> {
    let id = id
        .parse()
        .map_err(|_| JsValue::from_str("This preview is unavailable."))?;
    REVIEW.with(|review| {
        let outcome = review
            .borrow()
            .existing(id, flat, bytes)
            .map_err(|_| JsValue::from_str("This preview is unavailable."))?;
        serde_json::to_string(&outcome).map_err(|error| JsValue::from_str(&error.to_string()))
    })
}

#[wasm_bindgen]
pub fn export_reports(flat: bool, outcomes: &str, now: f64) -> Result<String, JsValue> {
    let outcomes: Vec<lrsalvage_core::extract::Outcome> =
        serde_json::from_str(outcomes).map_err(|error| JsValue::from_str(&error.to_string()))?;
    Ok(REVIEW.with(|review| {
        review
            .borrow()
            .reports(flat, outcomes, now as u64)
            .to_string()
    }))
}

/// The path-free verification report, as the JSON the page writes to a file.
///
/// The page's clock rather than the module's, for the same reason the manifest
/// takes one: a browser has a `Date` and this target has no trustworthy
/// `SystemTime` (`specs/modules/report.md`). `writes` is the batch's own account
/// and the only fact here the page measured — a catalog run ignores it, and the
/// encoder is still the one that writes the object, so no JavaScript
/// re-serialises a published file.
#[wasm_bindgen]
pub fn verification_report(now: f64, writes: &str) -> Result<String, JsValue> {
    let writes: lrsalvage_core::report::CacheBatch =
        serde_json::from_str(writes).map_err(|error| JsValue::from_str(&error.to_string()))?;
    Ok(REVIEW.with(|review| review.borrow().verification(writes, now as u64)))
}

/// The two release numbers ADR 0012 D2 puts on the receipt.
///
/// Compiled constants, so the page reads them from the module it loaded instead
/// of from a `build.json` a development tree does not have.
#[wasm_bindgen]
pub fn versions() -> String {
    json!({
        "app": crate::VERSION,
        "core": lrsalvage_core::VERSION,
    })
    .to_string()
}

/// Export only details that can be shown without exposing source paths.
fn review_problem(error: Error) -> JsValue {
    let problem = match error {
        Error::PreviewDimensionsChanged { expected, actual } => {
            json!({ "kind": "changed-size", "expected": expected, "actual": actual })
        }
        _ => json!({ "kind": "unreadable" }),
    };
    parse_json(&problem.to_string())
}

/// Register the mounts and begin installing the store. See [`install`].
#[wasm_bindgen]
pub fn start_install(mounts: &str, capacity: u32, clear: bool) -> Result<(), JsValue> {
    install(mounts, capacity, clear)
}

/// Advance the install. See [`poll`].
#[wasm_bindgen]
pub fn poll_install() -> String {
    poll()
}
