//! What the two evidence families' rows share.
//!
//! `render.rs` and `one_to_one.rs` drive the same binary against the same shape
//! of pair and read the same two artefacts back, so the harness lives here: a
//! correction to how a row scans a cache or walks an output tree cannot land in
//! one family and leave the other asserting a stale shape. Each target keeps
//! only its own numbers.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use lrsalvage_core::jpeg;
use serde_json::Value;

/// A gate's pair of names, as its catalog and cache halves.
///
/// The names stay in the row rather than in this module: which pair a gate owns
/// is the family's business, and a missing half must name the variable a user
/// has to set.
pub fn pair(catalog: &str, previews: &str, gate: &str) -> (PathBuf, PathBuf) {
    let read = |name: &str| {
        PathBuf::from(std::env::var(name).unwrap_or_else(|_| {
            panic!("set {name} to one half of the cache; scripts/check.sh {gate} does")
        }))
    };
    (read(catalog), read(previews))
}

/// `scan --json`, as its exit code and the object it printed.
pub fn scan(catalog: &Path, previews: &Path, extra: &[&str]) -> (i32, Value) {
    let output = Command::cargo_bin("lrsalvage")
        .expect("the binary")
        .arg("scan")
        .arg("--catalog")
        .arg(catalog)
        .arg("--previews")
        .arg(previews)
        .arg("--json")
        .args(extra)
        .output()
        .expect("run scan --json");
    let json: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|why| panic!("scan --json printed no object ({why}): {output:?}"));
    (output.status.code().unwrap_or(-1), json)
}

/// An `extract` under a fresh temp root, as that root and the `manifest.json` it
/// wrote. Holding the `TempDir` keeps the output alive for the assertions.
///
/// `--include-present` is always passed: both caches' originals still exist on
/// the disk they were rendered from, so without it there is nothing to write.
/// The manifest is the per-item record — quality, metadata and the payload each
/// item came from — so a row's own segments are visible without parsing
/// `report.csv`'s quoting by hand.
pub fn extract(catalog: &Path, previews: &Path, extra: &[&str]) -> (tempfile::TempDir, Value) {
    let out = tempfile::tempdir().expect("an output root");
    let output = Command::cargo_bin("lrsalvage")
        .expect("the binary")
        .arg("extract")
        .arg("--catalog")
        .arg(catalog)
        .arg("--previews")
        .arg(previews)
        .arg("--include-present")
        .args(extra)
        .arg("--out")
        .arg(out.path())
        .output()
        .expect("run extract");
    assert!(
        output.status.success(),
        "extract failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bytes = std::fs::read(out.path().join("manifest.json"))
        .expect("extract writes manifest.json beside its output");
    let manifest = serde_json::from_slice(&bytes).expect("manifest.json is an object");
    (out, manifest)
}

/// Every `.jpg` under `root`, as its own measured long edge — read from the
/// file's `SOF` rather than from the manifest, so a file the manifest does not
/// name is still counted.
pub fn jpeg_long_edges(root: &Path) -> Vec<u32> {
    let mut edges = Vec::new();
    // Output lands under capture-date folders, so the walk is recursive.
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        let path = entry.path();
        if !entry.file_type().is_file()
            || path.extension().is_none_or(|extension| extension != "jpg")
        {
            continue;
        }
        let bytes = std::fs::read(path).unwrap_or_else(|why| panic!("{}: {why}", path.display()));
        let (width, height) =
            jpeg::dimensions(&bytes).unwrap_or_else(|why| panic!("{}: {why:?}", path.display()));
        edges.push(width.max(height));
    }
    edges
}
