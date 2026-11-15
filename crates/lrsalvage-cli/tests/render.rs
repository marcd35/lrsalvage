//! A cache Lightroom **rendered** from scratch, and the numbers it produces.
//!
//! `format-notes` §5.10 measured three caches rendered from scratch and found
//! the state ADR 0008 is about: every image holds two pyramids — the one an
//! `ImageCacheEntry` claims and an unclaimed `thumbnail` pyramid beside it. The
//! synthetic rows `M10c`–`M10e` pin the reader's rule in that state; this row is
//! the one that asserts the published numbers against the real cache they were
//! measured on.
//!
//! It runs only where the machine holds such a cache, through
//! `scripts/check.sh real-render`, and it is the only gate that can tell a
//! fixture's decision from Adobe's behaviour.
//!
//! **Both halves matter.** `scan --json` proves the counts; `extract` proves the
//! companion is never written, which is the rule's promise to a user and the
//! half a scan alone cannot see.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use lrsalvage_core::jpeg;
use serde_json::Value;

/// The rendered cache's catalog and cache, as the gate's two names.
fn render_pair() -> (PathBuf, PathBuf) {
    let read = |name: &str| {
        PathBuf::from(std::env::var(name).unwrap_or_else(|_| {
            panic!("set {name} to the rendered cache's half; scripts/check.sh real-render does")
        }))
    };
    (
        read("LRSALVAGE_RENDER_CATALOG"),
        read("LRSALVAGE_RENDER_PREVIEWS"),
    )
}

fn scan(catalog: &Path, previews: &Path, extra: &[&str]) -> (i32, Value) {
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

/// `fresh-default` is the `Auto (3840px)` cache of §5.10: ten images, two
/// pyramids each, 48 level files.
#[test]
#[ignore = "needs LRSALVAGE_RENDER_CATALOG and LRSALVAGE_RENDER_PREVIEWS (scripts/check.sh real-render)"]
fn a_fresh_render_reports_its_companions_and_writes_none_of_them() {
    let (catalog, previews) = render_pair();
    let (code, report) = scan(&catalog, &previews, &["--include-present"]);
    assert_eq!(code, 0, "{report:#?}");

    // §5.10's counts. If one of these moves, either the rule changed or the
    // cache did — and the notes have to say which.
    assert_eq!(report["layout"], "modern");
    assert_eq!(report["images"], 10, "ten images in the rendered catalog");
    assert_eq!(
        report["recovered"], 10,
        "every image has a readable payload"
    );
    assert_eq!(
        report["unmapped"], 0,
        "a render's second pyramid is counted, not orphaned (ADR 0008)"
    );
    assert_eq!(
        report["thumbnail_pyramids"], 10,
        "one companion per image (§5.10)"
    );
    assert_eq!(report["shapes"]["level_file"], 48, "48 level files");
    assert_eq!(
        report["shapes"]["suffix_matches_sof"], 48,
        "every leaf names its own measured long edge"
    );
    assert_eq!(report["tiers"]["large"], 9, "nine images at 3840");
    assert_eq!(report["tiers"]["standard"], 1, "the 1200-wide image");
    assert_eq!(
        report["missing_payload"], 0,
        "no row of a claimed pyramid is missing its file"
    );

    // ... and the half a scan cannot see: 10 written, none of them a 320.
    let out = tempfile::tempdir().expect("an output root");
    let output = Command::cargo_bin("lrsalvage")
        .expect("the binary")
        .arg("extract")
        .arg("--catalog")
        .arg(&catalog)
        .arg("--previews")
        .arg(&previews)
        .arg("--include-present")
        .arg("--out")
        .arg(out.path())
        .output()
        .expect("run extract");
    assert!(
        output.status.success(),
        "extract failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut edges = BTreeSet::new();
    let mut written = 0;
    // Output lands under capture-date folders, so the walk is recursive.
    for entry in walkdir::WalkDir::new(out.path())
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        let path = entry.path();
        if !entry.file_type().is_file()
            || path.extension().is_none_or(|extension| extension != "jpg")
        {
            continue;
        }
        written += 1;
        let bytes = std::fs::read(path).unwrap_or_else(|why| panic!("{}: {why}", path.display()));
        let (width, height) =
            jpeg::dimensions(&bytes).unwrap_or_else(|why| panic!("{}: {why:?}", path.display()));
        edges.insert(width.max(height));
    }

    assert_eq!(written, 10, "one file per image, never one per pyramid");
    assert_eq!(
        edges,
        BTreeSet::from([1200, 3840]),
        "no output came from a companion: the 320 px payloads are never written"
    );
}
