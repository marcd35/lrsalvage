//! A cache Lightroom built 1:1 previews for, and the numbers it produces.
//!
//! `format-notes` §5.10's catalog C is the third of three caches rendered from
//! scratch on 2026-11-15: the same ten images as the `Auto (3840px)` cache, five
//! stars on all ten, and then `Library > Previews > Build 1:1 Previews`. A 1:1
//! build **replaces** the standard pyramid rather than adding to it, so C holds
//! ten pyramids and zero companions, forty level files, every one of them
//! carrying `Exif`, an XMP packet and an ICC profile, and no `standard` or
//! `thumbnail` row left behind. It is also the only evidence on this machine for
//! the absence case ADR 0008 and ADR 0009 leave room for: a cache where nothing
//! is a companion.
//!
//! It runs only where the machine holds such a cache, through
//! `scripts/check.sh real-1to1`, and it is the one gate that holds C's column of
//! §5.10 to the cache those numbers were measured on — and, in
//! `a_cap_chooses_the_largest_level_at_or_below_it`, holds the cap's winner rule
//! to the same cache's ladders (§5.12).
//!
//! **Both halves matter.** `scan --json` proves the counts; `extract` proves one
//! file is written per image and no companion is invented — and `--all-levels`
//! is what reaches all forty level files' own metadata, the half a scan cannot
//! see.

use std::collections::BTreeSet;

use serde_json::Value;

mod support;

fn items(manifest: &Value) -> &Vec<Value> {
    manifest["items"]
        .as_array()
        .unwrap_or_else(|| panic!("manifest.json carries no items: {manifest:#?}"))
}

/// `format-notes` §5.10, catalog C: ten images, ten pyramids, zero companions,
/// forty metadata-bearing level files, no `standard`/`thumbnail` row.
#[test]
#[ignore = "needs LRSALVAGE_ONE_TO_ONE_CATALOG and LRSALVAGE_ONE_TO_ONE_PREVIEWS (scripts/check.sh real-1to1)"]
fn a_one_to_one_build_reports_one_pyramid_per_image_and_writes_no_companion() {
    let (catalog, previews) = support::pair(
        "LRSALVAGE_ONE_TO_ONE_CATALOG",
        "LRSALVAGE_ONE_TO_ONE_PREVIEWS",
        "real-1to1",
    );
    let (code, report) = support::scan(&catalog, &previews, &["--include-present"]);
    assert_eq!(code, 0, "{report:#?}");

    // §5.10's counts for C. If one of these moves, either a rule changed or the
    // cache did — and the notes have to say which.
    assert_eq!(report["layout"], "modern");
    assert_eq!(report["images"], 10, "ten images in the 1:1 catalog");
    assert_eq!(
        report["recovered"], 10,
        "every image has a readable payload"
    );
    assert_eq!(
        report["thumbnail_pyramids"], 0,
        "a 1:1 build replaces the standard pyramid, so it leaves no companion (§5.10)"
    );
    assert_eq!(
        report["unmapped"], 0,
        "ten pyramids and ten claims: the absence case ADR 0008 leaves room for"
    );
    assert_eq!(report["shapes"]["level_file"], 40, "forty level files");
    assert_eq!(
        report["shapes"]["suffix_matches_sof"], 40,
        "every leaf names its own measured long edge"
    );
    assert_eq!(
        report["missing_payload"], 0,
        "no row of a claimed pyramid is missing its file"
    );

    // The first half a scan cannot see: one file per image, never one per
    // pyramid, and none of them from a 320 companion.
    let (out, manifest) = support::extract(&catalog, &previews, &[]);
    assert_eq!(manifest["stats"]["images"], 10, "ten images planned");
    assert_eq!(items(&manifest).len(), 10, "one item per image");
    let mut written = support::jpeg_long_edges(out.path());
    assert_eq!(
        written.len(),
        10,
        "one file per image, never one per pyramid"
    );
    written.sort_unstable();
    assert_eq!(
        written,
        vec![1200, 4096, 4096, 4096, 4096, 4096, 4096, 4096, 6000, 6000],
        "each image's own largest level; a companion's 320 px payload is never written"
    );

    // The second half: all forty level files, each carrying its own segments.
    let (_every, every) = support::extract(&catalog, &previews, &["--all-levels"]);
    let levels = items(&every);
    assert_eq!(levels.len(), 40, "one item per level file (§5.10)");
    assert_eq!(
        levels
            .iter()
            .filter(|level| level["payload_metadata"] == "exif,xmp,icc")
            .count(),
        40,
        "every level file of a 1:1 build is metadata-bearing: Exif, XMP and ICC (§5.10)"
    );
    // §5.10's `Pyramid.quality` column: the two to five levels of one pyramid
    // inherit its value, so the forty rows being all `final` is "ten pyramids,
    // none of them `standard` and none `thumbnail`" — C's two zeroes.
    let qualities: BTreeSet<&str> = levels
        .iter()
        .filter_map(|level| level["quality"].as_str())
        .collect();
    assert_eq!(
        qualities,
        BTreeSet::from(["final"]),
        "no `standard` or `thumbnail` pyramid survives a 1:1 build (§5.10)"
    );
    assert_eq!(
        levels
            .iter()
            .filter(|level| level["color_profile"] == "AdobeRGB")
            .count(),
        40,
        "every level of the 1:1 pyramid is AdobeRGB (§5.10)"
    );

    // §5.10's series for C: a 320 floor plus a doubling ladder ending at each
    // original's own long edge — not a standard series and not the conversion's.
    let mut edges: Vec<u64> = levels
        .iter()
        .map(|level| {
            level["width"]
                .as_u64()
                .unwrap_or_default()
                .max(level["height"].as_u64().unwrap_or_default())
        })
        .collect();
    edges.sort_unstable();
    let mut expected = Vec::new();
    expected.extend(std::iter::repeat_n(320, 10));
    expected.extend(std::iter::repeat_n(750, 2));
    expected.extend(std::iter::repeat_n(1024, 7));
    expected.extend(std::iter::repeat_n(1200, 1));
    expected.extend(std::iter::repeat_n(1500, 2));
    expected.extend(std::iter::repeat_n(2048, 7));
    expected.extend(std::iter::repeat_n(3000, 2));
    expected.extend(std::iter::repeat_n(4096, 7));
    expected.extend(std::iter::repeat_n(6000, 2));
    expected.sort_unstable();
    assert_eq!(
        edges, expected,
        "320 plus a doubling ladder ending at each original's own long edge (§5.10)"
    );

    // Ten pyramids, not forty: the rows group into one image each, which is the
    // state the companion rule is absent in — and the ten the `final` cell counts.
    let images: BTreeSet<&str> = levels
        .iter()
        .filter_map(|level| level["original_path"].as_str())
        .collect();
    assert_eq!(
        images.len(),
        10,
        "the forty level files belong to ten pyramids, one per image (§5.10)"
    );
}

/// `format-notes` §5.12: the cap's winner rule measured on C's own ladder rather
/// than on a vault this repository composed.
///
/// No level of C measures 1,920 px — its ladders step `320, 1024, 2048, 4096` and
/// `320, 750, 1500, 3000, 6000`, and the 1200 × 800 image's largest level is its
/// own size (§5.10) — so a run capped there must choose a smaller step rather
/// than the level named after the cap. That is the difference between a winner
/// rule and a filter after the fact, and it is what M16 pins synthetically.
#[test]
#[ignore = "needs LRSALVAGE_ONE_TO_ONE_CATALOG and LRSALVAGE_ONE_TO_ONE_PREVIEWS (scripts/check.sh real-1to1)"]
fn a_cap_chooses_the_largest_level_at_or_below_it() {
    let (catalog, previews) = support::pair(
        "LRSALVAGE_ONE_TO_ONE_CATALOG",
        "LRSALVAGE_ONE_TO_ONE_PREVIEWS",
        "real-1to1",
    );

    // Without `--all-levels` the plan is one item per image, and every image
    // clears the cap — so `above_cap` is 0 while a level above the cap exists,
    // which is `adr-0013`'s "invisible unless it binds" measured.
    let (code, json) = support::scan(
        &catalog,
        &previews,
        &["--include-present", "--max-long-edge", "1920"],
    );
    assert_eq!(code, 0, "{json:#?}");
    assert_eq!(json["images"], 10);
    assert_eq!(json["recovered"], 10, "every image has a level under 1,920");
    assert_eq!(
        json["below_floor"], 0,
        "the level the cap chose is above the 640 px floor"
    );
    assert_eq!(json["above_cap"], 0, "nothing is reported above the cap");
    assert_eq!(json["failed"], 0, "a cap never fails an image");
    // An option value never reclassifies a cache's shape (`adr-0008`), and these
    // are the rows §5.12 publishes: a capped run still sees one pyramid per
    // image, no companion and no payload without its row.
    assert_eq!(json["layout"], "modern");
    assert_eq!(json["thumbnail_pyramids"], 0);
    assert_eq!(json["unmapped"], 0);
    assert_eq!(json["missing_payload"], 0);
    assert_eq!(
        json["bytes"], 885_493,
        "the ten chosen payloads, which is what §5.12 publishes"
    );

    // Selection is the verification report's: `scan --json` carries no
    // `selected_items`, and the report echoes the cap the run used (ADR 0013).
    let (code, report) = support::scan_report(
        &catalog,
        &previews,
        &["--include-present", "--max-long-edge", "1920"],
    );
    assert_eq!(code, 0, "{report:#?}");
    assert_eq!(
        report["options"]["max_long_edge"], 1920,
        "the report echoes the cap it applied"
    );
    assert_eq!(
        report["stats"]["levels"], 10,
        "one item per image without --all-levels"
    );
    assert_eq!(report["stats"]["selected_images"], 10);
    assert_eq!(
        report["stats"]["selected_items"], 10,
        "ten images whose ladder holds a level at or below 1,920"
    );

    // `--all-levels` makes the cap a count: eighteen of §5.10's forty level files
    // are above it — 2,048 ×7, 3,000 ×2, 4,096 ×7, 6,000 ×2 — and the ten 320 px
    // floors are under the 640 px floor. The twelve inside the window are the
    // items that survive both bounds.
    let every = [
        "--include-present",
        "--all-levels",
        "--max-long-edge",
        "1920",
    ];
    let (code, all_json) = support::scan(&catalog, &previews, &every);
    assert_eq!(code, 0, "{all_json:#?}");
    assert_eq!(
        all_json["shapes"]["level_file"], 40,
        "forty level files over ten pyramids (§5.10)"
    );
    assert_eq!(all_json["below_floor"], 10, "the ten 320 px floors");
    assert_eq!(
        all_json["above_cap"], 18,
        "§5.10's ladder above 1,920: 2,048 ×7, 3,000 ×2, 4,096 ×7, 6,000 ×2"
    );
    let (code, all_report) = support::scan_report(&catalog, &previews, &every);
    assert_eq!(code, 0, "{all_report:#?}");
    assert_eq!(
        all_report["stats"]["selected_items"], 12,
        "750 ×2, 1,024 ×7, 1,200 ×1, 1,500 ×2 — the two 6,000 px ladders hold two steps each"
    );
    assert_eq!(
        all_report["stats"]["selected_images"], 10,
        "twelve items over ten images: a cap never splits one image's output"
    );
    assert_eq!(
        all_json["above_cap"].as_u64().unwrap_or_default()
            + all_json["below_floor"].as_u64().unwrap_or_default()
            + all_report["stats"]["selected_items"]
                .as_u64()
                .unwrap_or_default(),
        40,
        "above the cap, below the floor and inside the window partition the forty level files"
    );

    // The half a scan cannot see: one file per image, each the largest level at
    // or below 1,920 — never the 2,048, 3,000, 4,096 or 6,000 beside it.
    let (out, manifest) = support::extract(&catalog, &previews, &["--max-long-edge", "1920"]);
    assert_eq!(manifest["stats"]["images"], 10, "ten images planned");
    assert_eq!(
        items(&manifest).len(),
        10,
        "one item per image, as uncapped"
    );
    let mut written = support::jpeg_long_edges(out.path());
    assert_eq!(written.len(), 10, "one file per image (§5.12)");
    written.sort_unstable();
    assert_eq!(
        written,
        vec![1024, 1024, 1024, 1024, 1024, 1024, 1024, 1200, 1500, 1500],
        "the largest level at or below 1,920 is what was written"
    );

    // A cap no level clears is an empty window rather than a failure: exit 3,
    // nothing written, and no image given a new status — M17's state, here on a
    // ladder the cap steps through instead of one level it misses.
    let (code, empty) = support::extract_exit(&catalog, &previews, &["--max-long-edge", "200"]);
    assert_eq!(code, 3, "an empty window is not a failure");
    assert!(
        support::jpeg_long_edges(empty.path()).is_empty(),
        "nothing is written under a cap no level clears"
    );
    assert_eq!(
        std::fs::read_dir(empty.path())
            .expect("the output root")
            .count(),
        0,
        "an empty run writes no JPEG, no report.csv and no manifest.json"
    );
}
