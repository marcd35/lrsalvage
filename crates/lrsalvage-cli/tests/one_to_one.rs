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
//! §5.10 to the cache those numbers were measured on.
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
