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

mod support;

/// `fresh-default` is the `Auto (3840px)` cache of §5.10: ten images, two
/// pyramids each, 48 level files.
#[test]
#[ignore = "needs LRSALVAGE_RENDER_CATALOG and LRSALVAGE_RENDER_PREVIEWS (scripts/check.sh real-render)"]
fn a_fresh_render_reports_its_companions_and_writes_none_of_them() {
    let (catalog, previews) = support::pair(
        "LRSALVAGE_RENDER_CATALOG",
        "LRSALVAGE_RENDER_PREVIEWS",
        "real-render",
    );
    let (code, report) = support::scan(&catalog, &previews, &["--include-present"]);
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
    let (out, _manifest) = support::extract(&catalog, &previews, &[]);
    let written = support::jpeg_long_edges(out.path());
    assert_eq!(
        written.len(),
        10,
        "one file per image, never one per pyramid"
    );
    let edges: BTreeSet<u32> = written.into_iter().collect();
    assert_eq!(
        edges,
        BTreeSet::from([1200, 3840]),
        "no output came from a companion: the 320 px payloads are never written"
    );
}
