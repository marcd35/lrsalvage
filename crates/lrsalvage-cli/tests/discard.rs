//! A cache Lightroom **discarded** the standard and 1:1 previews of, and the
//! numbers it leaves behind.
//!
//! `format-notes` §5.11 measured what `Library > Previews > Discard Standard and
//! 1:1 Previews` does to a rendered cache on 2026-11-16: every preview pyramid
//! shrinks rather than empties — the 3840 px level goes and the 1920, 960 and
//! 320 px levels stay — the unclaimed companion pyramid is deleted outright, and
//! `Pyramid.quality` is rewritten to `bigThumbnail`, the levels a pyramid now
//! holds rather than the build that wrote it. No image loses its largest preview
//! and no `PyramidLevel` row is left without its file, so a discard is a
//! cache-size event rather than a loss.
//!
//! The synthetic row `M10h` of `specs/fixtures.md` pins the reader's rule in that
//! state; this row is the one that asserts the published numbers against the copy
//! Lightroom itself wrote.
//!
//! It runs only where the machine holds such a copy, through
//! `scripts/check.sh real-discard`, and it is deliberately scoped to its own test
//! target: the four evidence families fail separately rather than borrowing each
//! other's cache.
//!
//! **Both halves matter.** `scan --json` proves the counts; `extract` proves a
//! demoted pyramid is still a recoverable image, written under its own name, with
//! its own quality — which is the promise the state is worth to a user.
//!
//! **One number is deliberately not asserted here: the byte total.** §5.11
//! publishes one, but from a build this one is not, so a total inherited from it
//! would be a memory rather than a measurement — the TODO at the end of the test
//! says what turns it into an assertion. Everything else §5.11 publishes about
//! this state is held here, the demoted `quality` included.

use std::collections::BTreeMap;

mod support;

/// `format-notes` §5.11: ten images, ten claimed pyramids — nine demoted to
/// `bigThumbnail`, one left `final` — twenty-nine level files, no companion.
#[test]
#[ignore = "needs LRSALVAGE_DISCARD_CATALOG and LRSALVAGE_DISCARD_PREVIEWS (scripts/check.sh real-discard)"]
fn a_discarded_cache_reports_a_shrunk_pyramid_per_image_and_still_writes_ten() {
    let (catalog, previews) = support::pair(
        "LRSALVAGE_DISCARD_CATALOG",
        "LRSALVAGE_DISCARD_PREVIEWS",
        "real-discard",
    );
    let (code, report) = support::scan(&catalog, &previews, &["--include-present"]);
    assert_eq!(code, 0, "{report:#?}");

    // §5.11's counts, asserted as the notes publish them. If one of these moves,
    // either a rule changed or the copy did — and the notes have to say which.
    assert_eq!(report["layout"], "modern");
    assert_eq!(report["images"], 10, "ten images in the discarded catalog");
    assert_eq!(
        report["recovered"], 10,
        "a discard is a cache-size event, not a loss: every image still has a \
         readable payload at the largest level that survived (§5.11)"
    );
    assert_eq!(
        report["tiers"]["large"], 9,
        "nine images keep a 1920 px preview — above the large floor, even after \
         the 3840 px level was deleted (§5.11)"
    );
    assert_eq!(
        report["tiers"]["standard"], 1,
        "the one image whose series ended at 1200 px"
    );
    assert_eq!(
        report["thumbnail_pyramids"], 0,
        "the companion is not spared: a render's second pyramid is deleted \
         outright, so ADR 0008's cache fact is also a disposable one (§5.11)"
    );
    assert_eq!(
        report["unmapped"], 0,
        "a demoted pyramid is shrunk, not orphaned: the (uuid, digest) pair still \
         claims it (ADR 0002, ADR 0009)"
    );
    assert_eq!(
        report["missing_payload"], 0,
        "the discard deletes `PyramidLevel` rows and their files together, so no \
         row outlives its payload (§5.11)"
    );
    assert_eq!(
        report["below_floor"], 0,
        "every surviving largest level is above the default floor, so nothing is \
         excluded from a default run either"
    );
    assert_eq!(
        report["superseded"], 0,
        "no container payload survives a 13.x render for a discard to supersede"
    );
    assert_eq!(
        report["deleted"], 0,
        "`deleted` counts `missing-payload` items carrying a `DeletedPyramid` row; \
         the one tombstone left names a pair that is also live, and the discard \
         wrote no tombstone for the levels it removed (§5.11)"
    );
    assert_eq!(
        report["shapes"]["level_file"], 29,
        "the deletion is visible in the cache's shape: 48 level files before, 29 \
         after (§5.11)"
    );
    assert_eq!(
        report["shapes"]["suffix_matches_sof"], 29,
        "every surviving leaf still names its own measured long edge"
    );

    // The human surface too: `--json` replaces the summary rather than rendering
    // it, so the object's run says nothing about the command a person types.
    let (code, summary) = support::scan_text(&catalog, &previews, &["--include-present"]);
    assert_eq!(code, 0, "{summary}");
    assert!(
        summary.contains("cache layout: modern"),
        "the summary opens with the layout it read: {summary}"
    );

    // ... and the half a scan cannot see: the demoted pyramids are still written,
    // one per image, under the image's own name.
    let (out, manifest) = support::extract(&catalog, &previews, &["--min-long-edge", "0"]);
    assert_eq!(manifest["stats"]["images"], 10, "ten images planned");
    let items = manifest["items"]
        .as_array()
        .unwrap_or_else(|| panic!("manifest.json carries no items: {manifest:#?}"));
    assert_eq!(items.len(), 10, "one item per image");

    // The run's own record, not the plan's expectation: ten written, none failed.
    // An exit 0 already means no item failed — a partial run exits 2 — but the
    // rows say which is which instead of leaving it to the code.
    let outcomes = support::outcomes(out.path());
    assert_eq!(outcomes.len(), 10, "ten outcomes for ten selected images");
    let written = outcomes
        .iter()
        .filter(|(status, _)| status == "written")
        .count();
    assert_eq!(written, 10, "ten written: {outcomes:?}");
    assert_eq!(
        outcomes
            .iter()
            .filter(|(status, _)| status == "failed")
            .count(),
        0,
        "nothing failed: {outcomes:?}"
    );

    let mut edges = support::jpeg_long_edges(out.path());
    assert_eq!(edges.len(), 10, "ten JPEGs on disk");
    edges.sort_unstable();
    let mut expected = vec![1200];
    expected.extend(std::iter::repeat_n(1920, 9));
    expected.sort_unstable();
    assert_eq!(
        edges, expected,
        "each image's largest surviving level: nine at 1920 and the 1200-wide \
         image, never a 320 px thumbnail (§5.11)"
    );

    // The demotion, on the surface a user reads: nine claimed pyramids now say
    // `bigThumbnail`, and the one `final` pyramid the command left alone still
    // says `final` — `quality` describes the levels a pyramid holds, so a claimed
    // pyramid can carry the value ADR 0009 refuses to treat as a companion.
    let mut qualities: BTreeMap<&str, usize> = BTreeMap::new();
    for item in items {
        let quality = item["quality"]
            .as_str()
            .unwrap_or_else(|| panic!("an item carries no quality: {item:#?}"));
        *qualities.entry(quality).or_default() += 1;
    }
    assert_eq!(
        qualities,
        BTreeMap::from([("bigThumbnail", 9), ("final", 1)]),
        "§5.11's `Pyramid.quality` column, through the manifest: the nine demoted \
         pyramids report `bigThumbnail` and the untouched 1:1 pyramid reports \
         `final`"
    );

    // TODO — the byte total, and why it is not asserted yet.
    //
    // §5.11 publishes `2.6 MiB` for this run, measured by an `0.1.0` build whose
    // verification report echoed `options.min_long_edge: 640`: a number about a
    // tree this one is not, so asserting it would be inheriting a measurement
    // rather than holding one. When the copy is rebuilt and re-measured at the
    // current version, the total — the sum of these same rows' bytes, and the
    // `stats.bytes` the scan reports — becomes a strict `assert_eq!` here, and
    // §5.11's paragraph stops being the only place the number lives.
}
