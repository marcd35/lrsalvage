//! The fixture matrix, rows whose medium is the plan.
//!
//! Each test builds the vault `specs/fixtures.md` describes with the shared
//! builder, runs `Plan::build` (and `extract` where the row's assertion is
//! `both` or `bytes`), and asserts the structured fields the row names:
//! `Status`, `width`/`height`, `declared`, `tier`, `selected`, the payload's
//! kind, and the bytes of the file that was written. Note prose is never
//! asserted.
//!
//! **No total is asserted here.** The pinned totals belong to the slice that
//! completes each vault, because they include counts this slice cannot produce:
//! `modern`'s `unmapped 2` needs the orphan sweep over level files, `mixed`'s
//! `superseded 1` is a stat that does not exist yet, and `legacy`'s totals
//! include the unclaimed container L6. Rows still without a test are M4 and M11
//! (the bounded glob), M6 and M10 (the sweep and a tombstone count), M13 and M14
//! (`payload_metadata`, `effective_orientation`), X1 (the `superseded` stat),
//! and every `unrecognised`/layout row.
//!
//! `modern` holds only the rows asserted below, so its `images` count is the
//! count of this file's rows, not the thirteen the spec pins.

use std::fs;
use std::path::{Path, PathBuf};

use lrsalvage_core::extract::{self, ExtractOptions, Outcome};
use lrsalvage_core::jpeg;
use lrsalvage_core::plan::{Options, Payload, Plan, PlanItem, Status, Tier};
use lrsalvage_fixtures::bytes;
use lrsalvage_fixtures::vault::{ImageRow, Level, PyramidRow, Vault, digest_of};

// ---------------------------------------------------------------------------
// Composition
// ---------------------------------------------------------------------------

/// Every image carries XMP, so the metadata writer always has something to say
/// and an output really is `payload + one segment of ours`.
const XMP: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:tiff="http://ns.adobe.com/tiff/1.0/"
    xmp:CreateDate="2015-03-16T10:00:00"
    tiff:Make="SONY"/>
 </rdf:RDF>
</x:xmpmeta>"#;

/// One distinct uuid per row, so no two pyramids share an address.
fn uuid(row: u32) -> String {
    format!("AAAA{row:04}-1111-4111-8111-{row:012}")
}

/// `<cache>/<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>_<long_edge>`.
fn level_path(cache: &Path, uuid: &str, long_edge: u32) -> PathBuf {
    let (first, first_four) = bytes::leaf_dir(uuid);
    cache
        .join(first)
        .join(first_four)
        .join(bytes::leaf(uuid, &digest_of(uuid), long_edge))
}

/// The slim modern pyramid of M1: one row, one file, a bare payload.
fn m1_uuid() -> String {
    uuid(1)
}

/// `modern`, built from the rows this slice owns.
///
/// | id | row |
/// |----|-----|
/// | 1 | M1 recovered, one level file, bare payload |
/// | 2 | M2 recovered, five level files |
/// | 3 | M3 pruned level: the 1616 row's file is gone, `_808` survives |
/// | 4 | M5 suspect: row 1616x1077, file `_1616` whose SOF is 800x600 |
/// | 5 | M5b swapped: row 1616x1077, file SOF 1077x1616 |
/// | 6 | M7 missing-payload without a tombstone |
/// | 7 | M8 no index row |
/// | 8 | M9 original still on disk |
/// | 9 | M12 `longDimension` stored REAL, below the floor |
fn modern() -> Vault {
    let (m1, m2) = (uuid(1), uuid(2));
    let (m3, m5) = (uuid(3), uuid(4));
    let (m5b, m7) = (uuid(5), uuid(6));
    let (m9, m12) = (uuid(8), uuid(9));

    Vault::modern()
        // M1
        .image(ImageRow::new(1, "IMG_0001").cache(&m1).xmp(XMP))
        .pyramid(PyramidRow::new(&m1).level(1616, 1077))
        .level_file(Level::sized(&m1, 1616, 1077))
        // M2: five levels, largest wins
        .image(ImageRow::new(2, "IMG_0002").cache(&m2))
        .pyramid(
            PyramidRow::new(&m2)
                .level(101, 68)
                .level(202, 135)
                .level(404, 269)
                .level(808, 539)
                .level(1616, 1077),
        )
        .level_file(Level::sized(&m2, 101, 68))
        .level_file(Level::sized(&m2, 202, 135))
        .level_file(Level::sized(&m2, 404, 269))
        .level_file(Level::sized(&m2, 808, 539))
        .level_file(Level::sized(&m2, 1616, 1077))
        // M3: the 1616 row survives, only `_808` is on disk
        .image(ImageRow::new(3, "IMG_0003").cache(&m3))
        .pyramid(PyramidRow::new(&m3).level(808, 539))
        .row_without_file(Level::sized(&m3, 1616, 1077))
        .level_file(Level::sized(&m3, 808, 539))
        // M5: the file is named `_1616` but its frame is 800x600
        .image(ImageRow::new(4, "IMG_0004").cache(&m5))
        .pyramid(PyramidRow::new(&m5).level(1616, 1077))
        .level_file(Level::sized(&m5, 800, 600).long_dimension(1616.0))
        // M5b: the frame is the row's shape, rotated
        .image(ImageRow::new(5, "IMG_0005").cache(&m5b))
        .pyramid(PyramidRow::new(&m5b).level(1616, 1077))
        .level_file(Level::sized(&m5b, 1077, 1616))
        // M7: a row and no file anywhere
        .image(ImageRow::new(6, "IMG_0006").cache(&m7))
        .pyramid(PyramidRow::new(&m7).level(1616, 1077))
        .row_without_file(Level::sized(&m7, 1616, 1077))
        // M8: an image the cache never claimed
        .image(ImageRow::new(7, "IMG_0007"))
        // M9: the original is still on disk
        .image(ImageRow::new(8, "IMG_0008").cache(&m9).original_on_disk())
        .pyramid(PyramidRow::new(&m9).level(1616, 1077))
        .level_file(Level::sized(&m9, 1616, 1077))
        // M12: a REAL long dimension whose leaf is the rounded integer
        .image(ImageRow::new(9, "IMG_0009").cache(&m12))
        .pyramid(PyramidRow::new(&m12).level(101, 68))
        .level_file(Level::sized(&m12, 101, 68).long_dimension(101.0))
        .write()
}

/// `damaged`, built from the rows this slice owns: D1 (whose pyramid also
/// carries D5), D2, D3 and D4.
fn damaged() -> Vault {
    let (d1, d2) = (uuid(1), uuid(2));
    let (d3, d4) = (uuid(3), uuid(4));

    Vault::damaged()
        // D1/D5: a truncated 1616 winner with an intact 808 beside it
        .image(ImageRow::new(1, "IMG_0001").cache(&d1))
        .pyramid(PyramidRow::new(&d1).level(1616, 1077).level(808, 539))
        .level_file(Level::sized(&d1, 1616, 1077).raw(&bytes::truncated_jpeg(1616, 1077)))
        .level_file(Level::sized(&d1, 808, 539))
        // D2: bytes that were never a JPEG
        .image(ImageRow::new(2, "IMG_0002").cache(&d2))
        .pyramid(PyramidRow::new(&d2).level(1616, 1077))
        .level_file(Level::sized(&d2, 1616, 1077).raw(&bytes::not_a_jpeg()))
        // D3: no SOI, so nothing may be scanned for a frame behind the damage
        .image(ImageRow::new(3, "IMG_0003").cache(&d3))
        .pyramid(PyramidRow::new(&d3).level(1616, 1077))
        .level_file(Level::sized(&d3, 1616, 1077).raw(&bytes::no_soi(1616, 1077)))
        // D4: an intact sibling, so the run has something to write
        .image(ImageRow::new(4, "IMG_0004").cache(&d4))
        .pyramid(PyramidRow::new(&d4).level(1616, 1077))
        .level_file(Level::sized(&d4, 1616, 1077))
        .write()
}

/// `legacy`, built from L1-L5: one container per pyramid.
fn legacy() -> Vault {
    let (l1, l2) = (uuid(1), uuid(2));
    let (l3, l4) = (uuid(3), uuid(4));

    Vault::legacy()
        // L1: three levels, the largest chunk is the last one
        .image(ImageRow::new(1, "IMG_0001").cache(&l1))
        .pyramid(
            PyramidRow::new(&l1)
                .level(101, 68)
                .level(404, 269)
                .level(1616, 1077),
        )
        .level_file(Level::sized(&l1, 101, 68))
        .level_file(Level::sized(&l1, 404, 269))
        .level_file(Level::sized(&l1, 1616, 1077))
        // L2: chunks in an arbitrary order, so `level_2` holds the largest
        .image(ImageRow::new(2, "IMG_0002").cache(&l2))
        .pyramid(
            PyramidRow::new(&l2)
                .level(101, 68)
                .level(1616, 1077)
                .level(404, 269),
        )
        .level_file(Level::sized(&l2, 101, 68))
        .level_file(Level::sized(&l2, 1616, 1077))
        .level_file(Level::sized(&l2, 404, 269))
        // L3: the best chunk disagrees with the row
        .image(ImageRow::new(3, "IMG_0003").cache(&l3))
        .pyramid(PyramidRow::new(&l3).level(1616, 1077))
        .level_file(Level::sized(&l3, 800, 600))
        // L4: a row and no container
        .image(ImageRow::new(4, "IMG_0004").cache(&l4))
        .pyramid(PyramidRow::new(&l4).level(1616, 1077))
        .row_without_file(Level::sized(&l4, 1616, 1077))
        // L5: an image with no cache row
        .image(ImageRow::new(5, "IMG_0005"))
        .write()
}

/// `mixed`, built from X2 and X3: one pyramid per representation.
fn mixed() -> Vault {
    let (container, level_file) = (uuid(1), uuid(2));

    Vault::mixed()
        // X2: a `.lrprev`-only pyramid inside a modern cache
        .image(ImageRow::new(1, "IMG_0001").cache(&container))
        .pyramid(PyramidRow::new(&container).level(1616, 1077))
        .container(&[Level::sized(&container, 1616, 1077)])
        // X3: a level-file-only pyramid inside the same cache
        .image(ImageRow::new(2, "IMG_0002").cache(&level_file))
        .pyramid(PyramidRow::new(&level_file).level(1616, 1077))
        .level_file(Level::sized(&level_file, 1616, 1077))
        .write()
}

// ---------------------------------------------------------------------------
// Reading a plan back
// ---------------------------------------------------------------------------

fn items_for(plan: &Plan, image_id: i64) -> Vec<&PlanItem> {
    plan.items
        .iter()
        .filter(|item| {
            item.image
                .as_ref()
                .is_some_and(|image| image.id_local == image_id)
        })
        .collect()
}

fn item_for(plan: &Plan, image_id: i64) -> &PlanItem {
    let mut items = items_for(plan, image_id);
    assert_eq!(
        items.len(),
        1,
        "image {image_id} should be one item: {:#?}",
        plan.items
            .iter()
            .map(|item| (item.status, item.width, item.height))
            .collect::<Vec<_>>()
    );
    items.remove(0)
}

/// The certified `level_N` chunk of a container payload.
fn certified_level(item: &PlanItem) -> Option<&str> {
    match item.payload.as_ref() {
        Some(Payload::Container { level, .. }) => level.as_deref(),
        _ => None,
    }
}

fn is_level_file(item: &PlanItem) -> bool {
    matches!(item.payload, Some(Payload::LevelFile { .. }))
}

fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Write every selected item of `plan` and return the outcomes.
fn run_extract(vault: &Vault, plan: &Plan) -> Vec<Outcome> {
    let options = ExtractOptions {
        output_root: vault.out().to_path_buf(),
        // Deterministic output: a mtime is not what any row is about.
        set_timestamps: false,
        ..ExtractOptions::default()
    };
    extract::extract(plan, &options, &mut |_| {}).expect("extract what was planned")
}

/// The bytes of the file an item was written to.
fn written(item: &PlanItem) -> Vec<u8> {
    let output = item.output.as_ref().expect("a selected item has an output");
    fs::read(output).unwrap_or_else(|error| panic!("read {}: {error}", output.display()))
}

/// Payloads of every `FF E1` segment that opens with `Exif\0\0`.
fn exif_app1(bytes: &[u8]) -> Vec<&[u8]> {
    let certified = jpeg::certify(bytes).expect("a written file is a JPEG");
    certified
        .segments
        .iter()
        .filter(|segment| segment.marker == 0xE1)
        .map(|segment| segment.payload(bytes))
        .filter(|payload| payload.starts_with(b"Exif\0\0"))
        .collect()
}

// ---------------------------------------------------------------------------
// modern
// ---------------------------------------------------------------------------

#[test]
fn m1_a_level_file_is_the_payload_and_the_row_is_the_claim() {
    let vault = modern();
    let plan = vault.plan(&Options::default());

    let m1 = item_for(&plan, 1);
    assert_eq!(m1.status, Status::Recoverable);
    assert_eq!((m1.width, m1.height), (Some(1616), Some(1077)));
    assert_eq!(
        m1.declared,
        Some((1616, 1077)),
        "the chosen level's own row"
    );
    assert!(m1.selected);
    assert!(is_level_file(m1), "{:?}", m1.payload);

    // The leaf is derived from the row, never globbed.
    let expected = level_path(vault.previews(), &m1_uuid(), 1616);
    assert_eq!(
        m1.payload.as_ref().map(|payload| slashed(payload.path())),
        Some(slashed(&expected))
    );
    assert!(expected.is_file(), "{}", expected.display());

    // `both`: the file it produced is the payload plus one segment of ours.
    let payload = fs::read(&expected).expect("read the level file");
    let outcomes = run_extract(&vault, &plan);
    let m1_outcome = outcomes
        .iter()
        .find(|outcome| outcome.output == *m1.output.as_ref().expect("output"))
        .expect("the M1 outcome");
    assert_eq!(m1_outcome.status, extract::OutcomeStatus::Written);

    let written = written(m1);
    let certified = jpeg::certify(&written).expect("the output is a JPEG");
    assert!(certified.complete, "the output ends with EOI");
    assert_eq!(certified.dimensions, (1616, 1077));
    let ours = exif_app1(&written);
    assert_eq!(ours.len(), 1, "exactly one Exif APP1 of ours");
    assert!(
        ours[0]
            .windows(b"2015-03-16 10:00:00".len())
            .any(|window| window == b"2015-03-16 10:00:00"),
        "the segment carries the catalog's capture time"
    );
    let segment = certified
        .segments
        .iter()
        .find(|segment| segment.marker == 0xE1)
        .expect("the APP1 is in the header");
    assert_eq!(
        segment.range.start, 2,
        "our metadata goes immediately after SOI"
    );
    assert_eq!(
        &written[segment.range.end..],
        &payload[2..],
        "the payload's own bytes ride verbatim after our segment"
    );
}

#[test]
fn m2_the_largest_level_present_wins_its_image() {
    let vault = modern();
    let plan = vault.plan(&Options::default());

    // Five files, one item: 101, 202, 404 and 808 are not items.
    let m2 = item_for(&plan, 2);
    assert_eq!(m2.status, Status::Recoverable);
    assert_eq!((m2.width, m2.height), (Some(1616), Some(1077)));
    assert_eq!(m2.declared, Some((1616, 1077)));
    assert_eq!(
        slashed(m2.payload.as_ref().expect("payload").path()),
        slashed(&level_path(vault.previews(), &uuid(2), 1616))
    );
    // The sheet gets the smallest level of the same enumeration.
    assert_eq!(
        m2.smallest_level.as_deref().map(slashed),
        Some(slashed(&level_path(vault.previews(), &uuid(2), 101))),
        "the smallest level is the 101 leaf, not the winner"
    );
}

#[test]
fn m3_a_pruned_level_leaves_a_usable_smaller_one() {
    let vault = modern();
    let plan = vault.plan(&Options::default());

    let m3 = item_for(&plan, 3);
    assert_eq!(m3.status, Status::Recoverable);
    assert_eq!((m3.width, m3.height), (Some(808), Some(539)));
    assert_eq!(
        m3.declared,
        Some((808, 539)),
        "the surviving level is judged by its own row, not the pyramid's largest"
    );
    assert!(m3.selected);
    assert_eq!(
        slashed(m3.payload.as_ref().expect("payload").path()),
        slashed(&level_path(vault.previews(), &uuid(3), 808))
    );
    // Premise: the 1616 row really has no file behind it.
    assert!(!level_path(vault.previews(), &uuid(3), 1616).exists());
}

#[test]
fn m5_a_payload_of_the_wrong_size_is_suspect_and_still_written() {
    let vault = modern();
    let plan = vault.plan(&Options::default());

    let m5 = item_for(&plan, 4);
    assert_eq!(m5.status, Status::Suspect);
    assert_eq!((m5.width, m5.height), (Some(800), Some(600)));
    assert_eq!(m5.declared, Some((1616, 1077)));
    assert!(m5.selected, "a real photograph at the wrong size is kept");

    let outcomes = run_extract(&vault, &plan);
    assert_eq!(
        outcomes
            .iter()
            .find(|outcome| outcome.output == *m5.output.as_ref().expect("output"))
            .map(|outcome| outcome.status),
        Some(extract::OutcomeStatus::Written)
    );
    let written = written(m5);
    assert_eq!(
        jpeg::certify(&written).expect("a JPEG").dimensions,
        (800, 600),
        "the payload's own frame is what was written"
    );
}

#[test]
fn m5b_swapped_dimensions_are_not_a_disagreement() {
    let vault = modern();
    let plan = vault.plan(&Options::default());

    let m5b = item_for(&plan, 5);
    assert_eq!(m5b.status, Status::Recoverable);
    assert_eq!((m5b.width, m5b.height), (Some(1077), Some(1616)));
    assert_eq!(m5b.declared, Some((1616, 1077)));
    assert!(m5b.selected);
    // The view from disk: the leaf is the long edge, which the swap preserves.
    assert_eq!(
        slashed(m5b.payload.as_ref().expect("payload").path()),
        slashed(&level_path(vault.previews(), &uuid(5), 1616))
    );
}

#[test]
fn m7_a_pyramid_with_no_file_at_all_is_missing_payload() {
    let vault = modern();
    let plan = vault.plan(&Options::default());

    let m7 = item_for(&plan, 6);
    assert_eq!(m7.status, Status::MissingPayload);
    assert!(!m7.selected);
    assert!(m7.output.is_none());
    assert_eq!(
        certified_level(m7),
        None,
        "nothing was certified, so no level is named"
    );
    // With no file anywhere the representation is unknowable, so the item names
    // the container address every cache version has.
    assert!(
        slashed(m7.payload.as_ref().expect("payload").path())
            .ends_with(&format!("{}.lrprev", digest_of(&uuid(6)))),
        "{:?}",
        m7.payload
    );
}

#[test]
fn m8_an_image_without_a_cache_row_has_no_payload() {
    let vault = modern();
    let plan = vault.plan(&Options::default());

    let m8 = item_for(&plan, 7);
    assert_eq!(m8.status, Status::NoCacheEntry);
    assert!(m8.payload.is_none());
    assert!(!m8.selected);
}

#[test]
fn m9_an_image_whose_original_survives_is_not_a_salvage_target() {
    let vault = modern();
    let plan = vault.plan(&Options::default());

    let m9 = item_for(&plan, 8);
    assert_eq!(m9.status, Status::OriginalPresent);
    assert!(!m9.selected);
    assert!(m9.output.is_none());
    // Premise: the payload is there too, so only the original stops it.
    assert!(level_path(vault.previews(), &uuid(8), 1616).is_file());
}

#[test]
fn m12_a_real_long_dimension_names_the_leaf_and_the_floor_only_excludes() {
    let vault = modern();
    let plan = vault.plan(&Options::default());

    let m12 = item_for(&plan, 9);
    assert_eq!(m12.status, Status::Recoverable);
    assert_eq!((m12.width, m12.height), (Some(101), Some(68)));
    assert_eq!(m12.tier, Some(Tier::Thumbnail));
    assert!(!m12.selected, "below the default floor, so not written");
    // §5.7: `longDimension` came back REAL (`101.0`); the suffix is the
    // rounded integer, and the leaf below is the one the builder wrote.
    let expected = level_path(vault.previews(), &uuid(9), 101);
    assert_eq!(
        slashed(m12.payload.as_ref().expect("payload").path()),
        slashed(&expected)
    );
    assert!(expected.is_file(), "{}", expected.display());
}

// ---------------------------------------------------------------------------
// damaged
// ---------------------------------------------------------------------------

#[test]
fn d1_a_truncated_winner_is_failed_and_never_falls_back() {
    let vault = damaged();
    let plan = vault.plan(&Options::default());

    let d1 = item_for(&plan, 1);
    assert_eq!(d1.status, Status::Failed);
    assert!(!d1.selected, "nothing is written for a failed image");
    assert_eq!(
        (d1.width, d1.height),
        (Some(1616), Some(1077)),
        "the frame was readable; it is the end-of-image that is missing"
    );

    // The intact 808 beside it is not silently promoted to be the image.
    let outcomes = run_extract(&vault, &plan);
    assert!(
        outcomes
            .iter()
            .all(|outcome| outcome.output != *d1.output.as_ref().expect("output")),
        "a broken winner is not extracted"
    );
}

#[test]
fn d2_bytes_that_were_never_a_jpeg_are_failed() {
    let vault = damaged();
    let plan = vault.plan(&Options::default());

    let d2 = item_for(&plan, 2);
    assert_eq!(d2.status, Status::Failed);
    assert!(!d2.selected);
    assert_eq!((d2.width, d2.height), (None, None));
    assert!(is_level_file(d2), "it is still the payload that failed");
}

#[test]
fn d3_a_payload_without_an_soi_is_failed_not_scanned() {
    let vault = damaged();
    let plan = vault.plan(&Options::default());

    let d3 = item_for(&plan, 3);
    assert_eq!(d3.status, Status::Failed);
    assert!(!d3.selected);
    assert_eq!(
        (d3.width, d3.height),
        (None, None),
        "the frame behind the damage is never looked for"
    );
}

#[test]
fn d4_one_failure_does_not_stop_its_healthy_sibling() {
    let vault = damaged();
    let plan = vault.plan(&Options::default());

    let d4 = item_for(&plan, 4);
    assert_eq!(d4.status, Status::Recoverable);
    assert_eq!((d4.width, d4.height), (Some(1616), Some(1077)));
    assert!(d4.selected);

    let outcomes = run_extract(&vault, &plan);
    assert_eq!(outcomes.len(), 1, "only the healthy image is written");
    assert_eq!(outcomes[0].status, extract::OutcomeStatus::Written);
    let written = written(d4);
    assert_eq!(
        jpeg::certify(&written).expect("a JPEG").dimensions,
        (1616, 1077)
    );
}

#[test]
fn d5_all_levels_judges_each_level_by_its_own_row() {
    let vault = damaged();
    let options = Options {
        all_levels: true,
        ..Options::default()
    };
    let plan = vault.plan(&options);

    let items = items_for(&plan, 1);
    assert_eq!(items.len(), 2, "one item per level: {items:#?}");

    let broken = items
        .iter()
        .find(|item| item.width == Some(1616))
        .expect("the 1616 level");
    assert_eq!(broken.status, Status::Failed);
    assert!(!broken.selected);
    assert_eq!(
        broken
            .output
            .as_ref()
            .expect("output path")
            .file_name()
            .map(|name| name.to_string_lossy().into_owned()),
        Some("IMG_0001.jpg".to_string()),
        "the plain name still belongs to the pyramid's largest level"
    );

    let intact = items
        .iter()
        .find(|item| item.width == Some(808))
        .expect("the 808 level");
    assert_eq!(intact.status, Status::Recoverable);
    assert_eq!(intact.declared, Some((808, 539)));
    assert!(intact.selected, "a good level is still worth having");
    assert_eq!(
        intact
            .output
            .as_ref()
            .expect("output path")
            .file_name()
            .map(|name| name.to_string_lossy().into_owned()),
        Some("IMG_0001 - 808x539.jpg".to_string()),
        "only the extra level carries a size suffix"
    );

    let outcomes = run_extract(&vault, &plan);
    let written_names: Vec<String> = outcomes
        .iter()
        .filter(|outcome| outcome.status == extract::OutcomeStatus::Written)
        .filter_map(|outcome| outcome.output.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    assert!(
        !written_names.iter().any(|name| name == "IMG_0001.jpg"),
        "the failed 1616 owns the plain name and writes nothing: {written_names:?}"
    );
    let written = written(intact);
    let certified = jpeg::certify(&written).expect("a JPEG");
    assert!(certified.complete);
    assert_eq!(certified.dimensions, (808, 539));
}

// ---------------------------------------------------------------------------
// legacy
// ---------------------------------------------------------------------------

#[test]
fn l1_a_container_writes_the_chunk_the_plan_certified() {
    let vault = legacy();
    let plan = vault.plan(&Options::default());

    let l1 = item_for(&plan, 1);
    assert_eq!(l1.status, Status::Recoverable);
    assert_eq!((l1.width, l1.height), (Some(1616), Some(1077)));
    assert_eq!(
        certified_level(l1),
        Some("level_3"),
        "the item names the chunk that was certified"
    );
    assert!(matches!(
        l1.payload.as_ref().expect("payload"),
        Payload::Container { .. }
    ));

    let outcomes = run_extract(&vault, &plan);
    let l1_outcome = outcomes
        .iter()
        .find(|outcome| outcome.output == *l1.output.as_ref().expect("output"))
        .expect("the L1 outcome");
    assert_eq!(l1_outcome.status, extract::OutcomeStatus::Written);
    let written = written(l1);
    let certified = jpeg::certify(&written).expect("a JPEG");
    assert!(certified.complete);
    assert_eq!(certified.dimensions, (1616, 1077));
}

#[test]
fn l2_a_container_is_ranked_by_decoded_area_not_chunk_number() {
    let vault = legacy();
    let plan = vault.plan(&Options::default());

    let l2 = item_for(&plan, 2);
    assert_eq!(l2.status, Status::Recoverable);
    assert_eq!((l2.width, l2.height), (Some(1616), Some(1077)));
    assert_eq!(
        certified_level(l2),
        Some("level_2"),
        "the payload is written from the chunk whose frame is largest"
    );
}

#[test]
fn l3_a_container_keeps_its_comparison() {
    let vault = legacy();
    let plan = vault.plan(&Options::default());

    let l3 = item_for(&plan, 3);
    assert_eq!(l3.status, Status::Suspect);
    assert_eq!((l3.width, l3.height), (Some(800), Some(600)));
    assert!(l3.selected);
}

#[test]
fn l4_a_legacy_row_with_no_container_is_missing_payload() {
    let vault = legacy();
    let plan = vault.plan(&Options::default());

    let l4 = item_for(&plan, 4);
    assert_eq!(l4.status, Status::MissingPayload);
    assert!(!l4.selected);
    assert_eq!(certified_level(l4), None);
    assert!(
        slashed(l4.payload.as_ref().expect("payload").path()).ends_with(".lrprev"),
        "the missing thing is the container the row names"
    );
}

#[test]
fn l5_a_legacy_image_without_a_cache_row_is_no_cache_entry() {
    let vault = legacy();
    let plan = vault.plan(&Options::default());

    let l5 = item_for(&plan, 5);
    assert_eq!(l5.status, Status::NoCacheEntry);
    assert!(l5.payload.is_none());
}

// ---------------------------------------------------------------------------
// mixed
// ---------------------------------------------------------------------------

#[test]
fn x2_a_container_only_pyramid_in_a_modern_cache_is_read_from_the_container() {
    let vault = mixed();
    let plan = vault.plan(&Options::default());

    let x2 = item_for(&plan, 1);
    assert_eq!(x2.status, Status::Recoverable);
    assert_eq!((x2.width, x2.height), (Some(1616), Some(1077)));
    assert_eq!(certified_level(x2), Some("level_1"));
    assert!(
        slashed(x2.payload.as_ref().expect("payload").path()).ends_with(".lrprev"),
        "coexistence resolves per pyramid, not per cache"
    );
}

#[test]
fn x3_a_level_file_only_pyramid_in_a_mixed_cache_is_read_from_the_file() {
    let vault = mixed();
    let plan = vault.plan(&Options::default());

    let x3 = item_for(&plan, 2);
    assert_eq!(x3.status, Status::Recoverable);
    assert_eq!((x3.width, x3.height), (Some(1616), Some(1077)));
    assert!(is_level_file(x3), "{:?}", x3.payload);
    assert_eq!(
        slashed(x3.payload.as_ref().expect("payload").path()),
        slashed(&level_path(vault.previews(), &uuid(2), 1616)),
        "the level file's own leaf is the payload"
    );
}
