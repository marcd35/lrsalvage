//! The browser parity harness's wasm exports: the seam's install, and one run.
//!
//! The *install* half is not this crate's any more. A page has to register the
//! seam, mount its roots and install the origin-private pool before any run, and
//! the app's page has to do exactly that — so the shim lives with the app
//! (`lrsalvage_app::wasm`) and this crate links it (`#149`, DEC-S5). What stays
//! here is what is specific to the harness: the driver that produces the numbers
//! a native run of the same vault has to be compared against.
//!
//! The contract, in the order the worker calls them:
//!
//! 1. `start_install(mounts, capacity, clear)` — the app's, forwarded so this
//!    page's worker calls one module's exports.
//! 2. `poll_install()` — `"pending"`, `"done"` or `"failed: …"`, called from a
//!    macrotask until it settles.
//! 3. `scan_vault(catalog, cache, output)` — one run of [`crate::run`], as JSON.
//! 4. `probe(dir, store)` — one run of [`crate::probe`], as JSON.
//!
//! The three names carry a suffix because the app's exports are linked into this
//! crate too, and `wasm-bindgen` refuses two exports with one name in one module.
//! The app keeps the plain names — it is the shipped page — and the harness, which
//! is a measurement, says which it is measuring.

use wasm_bindgen::prelude::*;

use std::path::Path;

/// Register the mounts and begin installing the origin-private store.
#[wasm_bindgen(js_name = "install_vault")]
pub fn start_install(mounts: &str, capacity: u32, clear: bool) -> Result<(), JsValue> {
    lrsalvage_app::wasm::install(mounts, capacity, clear)
}

/// Advance the install: `"pending"`, `"done"`, or `"failed: …"`.
#[wasm_bindgen(js_name = "poll_vault")]
pub fn poll_install() -> String {
    lrsalvage_app::wasm::poll()
}

/// One scan-and-extract run, as the JSON a second target has to meet.
#[wasm_bindgen(js_name = "scan_vault")]
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
