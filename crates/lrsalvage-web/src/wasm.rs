//! The page's entry points: install the seam, then run it.
//!
//! `wasm-bindgen --target web` turns these into the module's exports, which is why
//! this file is the only place that mentions JavaScript. The worker that loads it
//! is `web/worker.js`.
//!
//! The contract, in the order the worker calls them:
//!
//! 1. `start_install(mounts, capacity, clear)` — registers the mount roots and
//!    begins installing the origin-private store.
//! 2. `poll_install()` — `"pending"`, `"done"` or `"failed: …"`, called from a
//!    macrotask until it settles. The store's install is a promise and this host
//!    has no async runtime, so it is polled rather than awaited; `#144`'s spec
//!    records why the runtime is not worth one promise.
//! 3. `scan(catalog, cache, output)` — one run of [`crate::run`], as JSON.
//! 4. `probe(dir, store)` — one run of [`crate::probe`], as JSON.
//!
//! `mounts` is a JSON array of `{ "path": "/source", "store": false }`. A page
//! supplies the same list twice — here for the routing, and to
//! `web/lrsalvage-fs.js` for the handles — because the two halves answer
//! different operations over the same namespace (`specs/modules/fs.md`).

use serde_json::Value;
use wasm_bindgen::prelude::*;

use std::path::Path;

/// The host's blocking call, from `web/lrsalvage-fs.js`.
///
/// Declared as four numbers rather than pointers because that is what crosses the
/// boundary; `wasm32` is 32 bits, so a pointer fits and needs no widening.
#[wasm_bindgen]
unsafe extern "C" {
    #[wasm_bindgen(js_namespace = "lrsalvage", js_name = "fs_call")]
    fn host_fs_call(op: u32, request_ptr: u32, request_len: u32, out_ptr: u32, out_cap: u32)
    -> i32;
}

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
#[wasm_bindgen]
pub fn start_install(mounts: &str, capacity: u32, clear: bool) -> Result<(), JsValue> {
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
#[wasm_bindgen]
pub fn poll_install() -> String {
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

/// One scan-and-extract run, as the JSON a second target has to meet.
#[wasm_bindgen]
pub fn scan(catalog: &str, cache: &str, output: &str) -> Result<String, JsValue> {
    let report = crate::run(Path::new(catalog), Path::new(cache), Path::new(output))
        .map_err(|error| JsValue::from_str(&error))?;
    serde_json::to_string(&report).map_err(|error| JsValue::from_str(&error.to_string()))
}

/// The seam's own tests, on this target.
#[wasm_bindgen]
pub fn probe(dir: &str, store: &str) -> String {
    let checks = crate::probe(Path::new(dir), Path::new(store));
    crate::probe_report(&checks).to_string()
}
