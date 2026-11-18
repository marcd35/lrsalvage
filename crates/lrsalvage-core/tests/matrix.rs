//! The fixture matrix, rows whose medium is the plan.
//!
//! Each test builds the vault `specs/fixtures.md` describes with the shared
//! builder, runs `Plan::build` (and `extract` where the row's assertion is
//! `both` or `bytes`), and asserts the structured fields the row names:
//! `Status`, `width`/`height`, `declared`, `tier`, `selected`, the payload's
//! kind, and the bytes of the file that was written. Note prose is never
//! asserted.
//!
//! **The image totals stay open, and shape facts do not.** `images`,
//! `recovered`, `suspect`, `missing_payload`, `no_cache_entry`,
//! `original_present`, `below_floor`, `levels` and `selected_images` belong to
//! the slice that completes each vault. So `modern` holds only the rows asserted
//! below, and its `images` count is the count of this file's rows, not the
//! fourteen the spec pins.
//!
//! What is asserted per vault is every **cache-level** fact, because ADR 0004
//! decides those once per cache: the layout word, the two version strings, the
//! three cache counts (`superseded`, `deleted`, `unrecognised`) and the `shapes`
//! triple. Each `shapes` count is derived from the files the composition
//! actually wrote — `vault.level_files()` and `vault.containers()` — so a
//! builder that writes one file fewer fails the row instead of silently
//! shrinking it.
//!
//! `unindexed` is here because an absent index is a mode: `Vault::plan` builds
//! one the same way it builds any other.

use std::fs;
use std::path::{Path, PathBuf};

use lrsalvage_core::Orientation;
use lrsalvage_core::extract::{self, ExtractOptions, Outcome};
use lrsalvage_core::jpeg;
use lrsalvage_core::plan::{Options, Payload, Plan, PlanItem, Status, Tier};
use lrsalvage_core::previews_db::{IndexProblem, Layout};
use lrsalvage_fixtures::bytes;
use lrsalvage_fixtures::vault::{ImageRow, Level, PyramidRow, UnrecognisedFile, Vault, digest_of};

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

/// The XMP packet M13's payload carries.
///
/// A foreign packet: it is left byte-identical, so it can never be the one our
/// writer would produce for the rating.
const PAYLOAD_XMP: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:Rating="2"/>
 </rdf:RDF>
</x:xmpmeta>"#;

/// The ICC profile M13's payload carries.
///
/// Deliberately not one of the two this repository builds: a reconstruction may
/// never displace it, so the bytes themselves prove which won.
const PAYLOAD_ICC: &[u8] = b"a profile this repository did not build";

/// Every APP segment M13's payload carries, in the order it carries them.
fn m13_segments() -> [Vec<u8>; 3] {
    [
        bytes::exif_app1(6),
        bytes::xmp_app1(PAYLOAD_XMP),
        bytes::icc_app2(PAYLOAD_ICC),
    ]
}

/// M13's payload: its own EXIF, XMP and ICC, in front of a bare frame.
fn m13_payload() -> Vec<u8> {
    bytes::jpeg_with(1616, 1077, &m13_segments())
}

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

/// M11's leaf carries a digest its row does not name, so it is another pyramid
/// (`adr-0005-discover-payloads-by-index` ¶6).
const DIGEST_B: &str = "b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0";

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
/// | 10 | M4 level file found only by the widening, its 1616 row's file absent |
/// | 11 | M11 row digestA, file digestB: missing-payload plus one orphan |
/// | 12 | M6 missing-payload whose pyramid carries a tombstone |
/// | 13 | M13 payload carrying its own EXIF, XMP and ICC |
/// | 14 | M14 bare payload, catalog claims the orientation and the profile |
/// | — | M10 an unclaimed pyramid of two level files (uuid 20) |
fn modern() -> Vault {
    let (m1, m2) = (uuid(1), uuid(2));
    let (m3, m5) = (uuid(3), uuid(4));
    let (m5b, m7) = (uuid(5), uuid(6));
    let (m9, m12) = (uuid(8), uuid(9));
    let (m4, m11) = (uuid(10), uuid(11));
    let m6 = uuid(12);
    let (m13, m14) = (uuid(13), uuid(14));
    let m10 = uuid(20);

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
        // M4: the 1616 row's file is gone; the `_640` file has no row at all and
        // is found by the widening inside this pyramid's own shard folder.
        .image(ImageRow::new(10, "IMG_0010").cache(&m4))
        .pyramid(PyramidRow::new(&m4).level(1616, 1077))
        .file_without_row(Level::sized(&m4, 640, 427))
        // M11: the row names digestA, the file carries digestB, so the image has
        // no payload and the file is another pyramid's.
        .image(ImageRow::new(11, "IMG_0011").cache(&m11))
        .pyramid(PyramidRow::new(&m11).level(1616, 1077))
        .file_without_row(Level::sized(&m11, 1616, 1077).digest(DIGEST_B))
        // M6: a row, no file, and the tombstone Lightroom wrote when it
        // deleted the pyramid.
        .image(ImageRow::new(12, "IMG_0012").cache(&m6))
        .pyramid(PyramidRow::new(&m6).level(1616, 1077))
        .tombstone(&m6, 700_000_000.0)
        // M13: the payload carries its own EXIF, XMP and ICC profile. The
        // catalog claims an upright image and a different profile, so both
        // precedence decisions are visible in the bytes.
        .image(ImageRow::new(13, "IMG_0013").cache(&m13).xmp(XMP))
        .pyramid(PyramidRow::new(&m13).profile("AdobeRGB").level(1616, 1077))
        .level_file(Level::sized(&m13, 1616, 1077).raw(&m13_payload()))
        // M14: a bare payload; the catalog supplies the orientation and the
        // profile name, and neither is the payload's own.
        .image(
            ImageRow::new(14, "IMG_0014")
                .cache(&m14)
                .orientation("BC")
                .xmp(XMP),
        )
        .pyramid(PyramidRow::new(&m14).profile("AdobeRGB").level(1616, 1077))
        .level_file(Level::sized(&m14, 1616, 1077))
        // M10: a pyramid no image claims, two level files, one orphan.
        .unclaimed(&[Level::sized(&m10, 640, 427), Level::sized(&m10, 1616, 1077)])
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

/// `legacy`, built from L1-L6: one container per pyramid.
fn legacy() -> Vault {
    let (l1, l2) = (uuid(1), uuid(2));
    let (l3, l4) = (uuid(3), uuid(4));
    let l6 = uuid(20);

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
        // L6: a container no image claims — the container arm of the sweep.
        .unclaimed(&[Level::sized(&l6, 1616, 1077)])
        .write()
}

/// `mixed`, built from X1, X2 and X3: one pyramid per representation.
fn mixed() -> Vault {
    let (superseded, container) = (uuid(1), uuid(2));
    let level_file = uuid(3);

    Vault::mixed()
        // X1: level files beside a `.lrprev` that is deliberately not a
        // container, so reading it would fail the row (ADR 0006 ¶6).
        .image(ImageRow::new(1, "IMG_0001").cache(&superseded))
        .pyramid(PyramidRow::new(&superseded).level(1616, 1077))
        .superseded(&[Level::sized(&superseded, 1616, 1077)])
        // X2: a `.lrprev`-only pyramid inside a modern cache
        .image(ImageRow::new(2, "IMG_0002").cache(&container))
        .pyramid(PyramidRow::new(&container).level(1616, 1077))
        .container(&[Level::sized(&container, 1616, 1077)])
        // X3: a level-file-only pyramid inside the same cache
        .image(ImageRow::new(3, "IMG_0003").cache(&level_file))
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

/// The ICC profile a JPEG carries, reassembled from its `ICC_PROFILE` APP2
/// segments by the same signature the writer uses.
fn embedded_icc(bytes: &[u8]) -> Option<Vec<u8>> {
    let certified = jpeg::certify(bytes).expect("a written file is a JPEG");
    let mut profile = Vec::new();
    for segment in &certified.segments {
        let payload = segment.payload(bytes);
        if segment.marker == 0xE2 && payload.starts_with(b"ICC_PROFILE\0") {
            profile.extend_from_slice(&payload[14..]);
        }
    }
    (!profile.is_empty()).then_some(profile)
}

/// An IFD0 integer read by an independent parser, not by `exif`'s own reader.
fn ifd0_uint(bytes: &[u8], tag: exif::Tag) -> Option<u32> {
    exif::Reader::new()
        .read_from_container(&mut std::io::Cursor::new(bytes))
        .ok()?
        .get_field(tag, exif::In::PRIMARY)?
        .value
        .get_uint(0)
}

/// An IFD0 ASCII value read by an independent parser.
fn ifd0_text(bytes: &[u8], tag: exif::Tag) -> Option<String> {
    let parsed = exif::Reader::new()
        .read_from_container(&mut std::io::Cursor::new(bytes))
        .ok()?;
    match &parsed.get_field(tag, exif::In::PRIMARY)?.value {
        exif::Value::Ascii(values) => values
            .first()
            .map(|value| String::from_utf8_lossy(value).into_owned()),
        _ => None,
    }
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
fn m6_a_tombstoned_pyramid_is_still_missing_payload_and_counted_as_deleted() {
    let vault = modern();
    let plan = vault.plan(&Options::default());

    let m6 = item_for(&plan, 12);
    assert_eq!(
        m6.status,
        Status::MissingPayload,
        "a tombstone is Lightroom's record that it deleted the row, not a cause"
    );
    assert!(!m6.selected);
    // The count is the cache's, not the item's: the note quotes the deletion
    // and no field claims who removed the file.
    assert_eq!(plan.stats.deleted, 1, "modern's pinned `deleted 1` is M6");
    assert!(
        m6.note
            .as_deref()
            .is_some_and(|note| note.contains("deletion at 700000000")),
        "the note quotes the timestamp previews.db recorded: {:?}",
        m6.note
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
fn m4_a_level_file_only_the_widening_found_recovers_the_image() {
    let vault = modern();
    let plan = vault.plan(&Options::default());

    let m4 = item_for(&plan, 10);
    assert_eq!(m4.status, Status::Recoverable);
    assert_eq!(
        (m4.width, m4.height),
        (Some(640), Some(427)),
        "the payload's own SOF, since no row declares this leaf"
    );
    assert_eq!(
        m4.declared, None,
        "a file with no row has nothing declared for it"
    );
    assert!(m4.selected);
    assert!(is_level_file(m4), "{:?}", m4.payload);

    // Premise: the row's own 1616 file is absent, so only the widening found it.
    let expected = level_path(vault.previews(), &uuid(10), 640);
    assert!(!level_path(vault.previews(), &uuid(10), 1616).exists());
    assert_eq!(
        m4.payload.as_ref().map(|payload| slashed(payload.path())),
        Some(slashed(&expected))
    );
    assert_eq!(
        m4.smallest_level.as_deref().map(slashed),
        Some(slashed(&expected)),
        "the sheet gets the one level the same widening found"
    );
    // `levels +1`: one item, so one level, for this image.
    assert_eq!(items_for(&plan, 10).len(), 1);

    // `bytes`: the file it produced is that payload, certified at its own size.
    run_extract(&vault, &plan);
    let written = written(m4);
    let certified = jpeg::certify(&written).expect("the output is a JPEG");
    assert!(certified.complete, "the output ends with EOI");
    assert_eq!(certified.dimensions, (640, 427));
}

#[test]
fn m10_an_unclaimed_pyramid_is_one_orphan_item() {
    let vault = modern();
    let plan = vault.plan(&Options::default());

    let pyramid = uuid(20);
    let items: Vec<&PlanItem> = plan
        .items
        .iter()
        .filter(|item| {
            item.payload
                .as_ref()
                .is_some_and(|payload| slashed(payload.path()).contains(&pyramid))
        })
        .collect();
    assert_eq!(
        items.len(),
        1,
        "two level files, one pyramid, one orphan: {items:#?}"
    );

    let orphan = items[0];
    assert_eq!(orphan.status, Status::Unmapped);
    assert!(orphan.image.is_none(), "nothing claims it");
    assert!(!orphan.selected, "listed, never selected");
    assert!(orphan.output.is_none(), "an orphan has no output path");
    assert!(is_level_file(orphan), "{:?}", orphan.payload);
    assert_eq!(
        slashed(orphan.payload.as_ref().expect("payload").path()),
        slashed(&level_path(vault.previews(), &pyramid, 1616)),
        "the payload is the largest level it holds, chosen from the name alone"
    );
    assert!(
        orphan
            .note
            .as_deref()
            .is_some_and(|note| note.contains(&pyramid)),
        "the note names the payload: {:?}",
        orphan.note
    );
}

/// M10c — a render writes two pyramids for one uuid, and the reader reads both
/// halves of the pair from the cache entry.
///
/// The thumbnail row is written **last**, which is the order that used to decide
/// the answer: keyed by uuid, the later `Pyramid` row won, so the preview's
/// `AdobeRGB` profile and its 3840 level were returned by luck of row order
/// rather than by rule (issue #42).
#[test]
fn m10c_two_pyramids_of_one_uuid_are_read_by_the_pair() {
    let pyramid = uuid(21);
    let thumbnail = uuid(21);
    let thumbnail_digest = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
    let vault = Vault::modern()
        .image(ImageRow::new(21, "IMG_0021").cache(&pyramid))
        .pyramid(
            PyramidRow::new(&pyramid)
                .profile("AdobeRGB")
                .quality("standard")
                .level(320, 213)
                .level(960, 640)
                .level(1920, 1280)
                .level(3840, 2560),
        )
        .level_file(Level::sized(&pyramid, 320, 213))
        .level_file(Level::sized(&pyramid, 960, 640))
        .level_file(Level::sized(&pyramid, 1920, 1280))
        .level_file(Level::sized(&pyramid, 3840, 2560))
        // The render's second pyramid, written after the one the entry claims.
        .pyramid(
            PyramidRow::new(&thumbnail)
                .digest(thumbnail_digest)
                .profile("sRGB")
                .quality("thumbnail")
                .level(320, 213),
        )
        .level_file(Level::sized(&thumbnail, 320, 213).digest(thumbnail_digest))
        .write();

    let plan = vault.plan(&Options::default());
    let item = item_for(&plan, 21);
    assert_eq!(item.status, Status::Recoverable);
    assert_eq!(
        item.color_profile.as_deref(),
        Some("AdobeRGB"),
        "the row the entry's pair names, not the later thumbnail row"
    );
    assert_eq!(item.quality.as_deref(), Some("standard"));
    assert_eq!(
        (item.width, item.height),
        (Some(3840), Some(2560)),
        "the preview pyramid's best level, never the companion's 320"
    );

    assert_eq!(
        plan.stats.unmapped, 0,
        "a render companion is not an orphan"
    );
    assert_eq!(plan.cache.thumbnail_pyramids, 1);
    assert_eq!(
        plan.cache.shapes.level_file, 5,
        "both pyramids' files are still counted: nothing is hidden from the tree"
    );
}

/// M10d — a `thumbnail` row whose uuid no live image claims.
///
/// The clause that keeps a lost photograph visible: a pyramid nothing maps stays
/// an orphan even when its row calls it a thumbnail.
#[test]
fn m10d_a_thumbnail_row_with_no_live_claim_stays_unmapped() {
    let pyramid = uuid(22);
    let vault = Vault::modern()
        // The catalog holds the image; no cache entry names any pyramid of it.
        .image(ImageRow::new(22, "IMG_0022"))
        .pyramid(
            PyramidRow::new(&pyramid)
                .profile("sRGB")
                .quality("thumbnail")
                .level(320, 213),
        )
        .level_file(Level::sized(&pyramid, 320, 213))
        .write();

    let plan = vault.plan(&Options::default());
    assert_eq!(item_for(&plan, 22).status, Status::NoCacheEntry);
    assert_eq!(
        plan.stats.unmapped, 1,
        "nothing claims it, so it is an orphan"
    );
    assert_eq!(plan.cache.thumbnail_pyramids, 0);

    let orphan = plan
        .items
        .iter()
        .find(|item| item.status == Status::Unmapped)
        .expect("the orphan");
    assert_eq!(
        orphan.quality.as_deref(),
        Some("thumbnail"),
        "an orphan now carries the row facts its pair reaches"
    );
    assert_eq!(orphan.color_profile.as_deref(), Some("sRGB"));
}

/// M10e — a `thumbnail` row beside a claimed pyramid, holding a level that is
/// not small.
///
/// The row's word is never enough on its own: a row that lies about its size may
/// not hide a payload the tool would otherwise be willing to write.
#[test]
fn m10e_a_thumbnail_row_with_a_large_level_stays_unmapped() {
    let pyramid = uuid(23);
    let thumbnail_digest = "b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1";
    let vault = Vault::modern()
        .image(ImageRow::new(23, "IMG_0023").cache(&pyramid))
        .pyramid(
            PyramidRow::new(&pyramid)
                .profile("AdobeRGB")
                .quality("standard")
                .level(3840, 2560),
        )
        .level_file(Level::sized(&pyramid, 3840, 2560))
        // The row says `thumbnail` and declares 320; the only file of it measures
        // 3840, so the bytes outrank the word.
        .pyramid(
            PyramidRow::new(&pyramid)
                .digest(thumbnail_digest)
                .profile("sRGB")
                .quality("thumbnail")
                .level(320, 213),
        )
        .file_without_row(Level::sized(&pyramid, 3840, 2560).digest(thumbnail_digest))
        .write();

    let plan = vault.plan(&Options::default());
    assert_eq!(item_for(&plan, 23).status, Status::Recoverable);
    assert_eq!(
        plan.stats.unmapped, 1,
        "a level above the thumbnail tier disqualifies the companion"
    );
    assert_eq!(plan.cache.thumbnail_pyramids, 0);
}

/// M10f — the image's own pyramid has no file, and its companion is the last
/// payload left.
///
/// The state a 14.0+ background prune leaves (unmeasured in the field, §5.5): the
/// rows survive, the pyramid's files do not, and the render's second pyramid is
/// all that is left of the photograph. It is `recovered`, not `missing-payload`.
#[test]
fn m10f_an_image_whose_pyramid_is_gone_is_rescued_from_its_companion() {
    let pyramid = uuid(24);
    let companion_digest = "c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2";
    let vault = Vault::modern()
        .image(ImageRow::new(24, "IMG_0024").cache(&pyramid))
        // The pyramid the entry names: rows, and no file anywhere.
        .pyramid(
            PyramidRow::new(&pyramid)
                .profile("AdobeRGB")
                .quality("standard")
                .level(3840, 2560),
        )
        .row_without_file(Level::sized(&pyramid, 3840, 2560))
        // Its companion, the only payload the image has left.
        .pyramid(
            PyramidRow::new(&pyramid)
                .digest(companion_digest)
                .profile("sRGB")
                .quality("thumbnail")
                .level(320, 213),
        )
        .level_file(Level::sized(&pyramid, 320, 213).digest(companion_digest))
        .write();

    let plan = vault.plan(&Options::default());
    let item = item_for(&plan, 24);
    assert_eq!(
        item.status,
        Status::Recoverable,
        "a surviving payload is not `missing-payload`"
    );
    assert_eq!((item.width, item.height), (Some(320), Some(213)));
    assert_eq!(item.tier, Some(Tier::Thumbnail));
    assert_eq!(
        item.quality.as_deref(),
        Some("thumbnail"),
        "the row the payload actually came from"
    );
    assert_eq!(item.color_profile.as_deref(), Some("sRGB"));
    assert!(!item.selected, "below the floor: counted, not written");
    assert_eq!(plan.stats.missing_payload, 0);
    assert_eq!(plan.stats.below_floor, 1);
    assert_eq!(
        plan.cache.thumbnail_pyramids, 1,
        "the companion is still a cache fact, whatever the plan did with it"
    );
    assert!(
        item.note
            .as_deref()
            .is_some_and(|note| note.contains("render companion")),
        "the item says where it came from: {:?}",
        item.note
    );

    // Respecting the floor is not refusing it: `--min-long-edge 0` writes the
    // rescued payload under the image's own name.
    let plan = vault.plan(&Options {
        min_long_edge: 0,
        ..Options::default()
    });
    assert!(item_for(&plan, 24).selected);
    let outcomes = run_extract(&vault, &plan);
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].status, extract::OutcomeStatus::Written);
    let bytes = fs::read(&outcomes[0].output).expect("the rescued file");
    assert_eq!(
        jpeg::dimensions(&bytes).expect("a readable frame"),
        (320, 213)
    );
}

/// M10g — a sibling the companion test rejects does not rescue.
///
/// A `bigThumbnail` pyramid on the same uuid holds a perfectly good 320 payload
/// and is still not used: the rescue is bound to the pyramid §5.10 measured a
/// render writing, so the image stays `missing-payload` and the sibling stays an
/// orphan.
#[test]
fn m10g_a_sibling_of_another_quality_does_not_rescue() {
    let pyramid = uuid(25);
    let sibling_digest = "d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3";
    let vault = Vault::modern()
        .image(ImageRow::new(25, "IMG_0025").cache(&pyramid))
        .pyramid(
            PyramidRow::new(&pyramid)
                .profile("AdobeRGB")
                .quality("standard")
                .level(3840, 2560),
        )
        .row_without_file(Level::sized(&pyramid, 3840, 2560))
        .pyramid(
            PyramidRow::new(&pyramid)
                .digest(sibling_digest)
                .profile("sRGB")
                .quality("bigThumbnail")
                .level(320, 213),
        )
        .level_file(Level::sized(&pyramid, 320, 213).digest(sibling_digest))
        .write();

    let plan = vault.plan(&Options::default());
    assert_eq!(item_for(&plan, 25).status, Status::MissingPayload);
    assert_eq!(plan.stats.missing_payload, 1);
    assert_eq!(
        plan.stats.unmapped, 1,
        "the sibling is an orphan, not a companion"
    );
    assert_eq!(plan.cache.thumbnail_pyramids, 0);
}

/// M10h — a claimed pyramid in the state a discard leaves it.
///
/// `Library > Previews > Discard Standard and 1:1 Previews` deletes each preview
/// pyramid's standard levels **and their rows**, rewrites `Pyramid.quality` to
/// `bigThumbnail` and takes the render companion with it (§5.11). The demotion
/// changes what `quality` means and decides nothing: the pair still claims the
/// pyramid, so the largest surviving level is the best level, the pyramid is
/// neither an orphan nor a rescue candidate, and nothing is below the floor.
#[test]
fn m10h_a_discarded_pyramid_is_still_the_one_the_entry_claims() {
    let pyramid = uuid(26);
    let vault = Vault::modern()
        .image(ImageRow::new(26, "IMG_0026").cache(&pyramid))
        .pyramid(
            PyramidRow::new(&pyramid)
                .profile("AdobeRGB")
                .quality("bigThumbnail")
                // The pre-discard value: §5.11 never reported what the discard
                // does to `croppedWidth`/`croppedHeight`, and nothing reads them
                // for a level-file pyramid. Stated rather than inherited from
                // the builder's first-level fallback, which would claim 320.
                .cropped(3840, 2560)
                .level(320, 213)
                .level(960, 640)
                .level(1920, 1280),
        )
        .level_file(Level::sized(&pyramid, 320, 213))
        .level_file(Level::sized(&pyramid, 960, 640))
        .level_file(Level::sized(&pyramid, 1920, 1280))
        .write();

    let plan = vault.plan(&Options::default());
    let item = item_for(&plan, 26);
    assert_eq!(item.status, Status::Recoverable);
    assert_eq!(
        (item.width, item.height),
        (Some(1920), Some(1280)),
        "the largest surviving level, which the discard left alone"
    );
    assert_eq!(
        item.tier,
        Some(Tier::Large),
        "1920 is below the 3840 render it was built from, and `large` anyway"
    );
    assert_eq!(
        item.quality.as_deref(),
        Some("bigThumbnail"),
        "quality names the levels the pyramid holds now, not the build that wrote it"
    );
    assert_eq!(
        item.declared,
        Some((1920, 1280)),
        "the surviving row is the claim, not the pyramid's cropped size"
    );
    assert!(item.selected, "1920 px is above the 640 floor");

    assert_eq!(plan.stats.recoverable, 1);
    assert_eq!(
        plan.stats.levels, 1,
        "the best level is the item; the two smaller files are not"
    );
    assert_eq!(
        plan.stats.unmapped, 0,
        "a demoted pyramid the entry claims is not an orphan"
    );
    assert_eq!(
        plan.cache.thumbnail_pyramids, 0,
        "the discard took the companion, and a claimed `bigThumbnail` pyramid is not one"
    );
    assert_eq!(plan.stats.missing_payload, 0);
    assert_eq!(plan.stats.below_floor, 0);
}

#[test]
fn m11_a_digest_mismatched_file_is_another_pyramid() {
    let vault = modern();
    let plan = vault.plan(&Options::default());

    // The image's row names digestA; the only file carries digestB.
    let m11 = item_for(&plan, 11);
    assert_eq!(m11.status, Status::MissingPayload);
    assert!(!m11.selected);
    let digest_a = digest_of(&uuid(11));
    assert_ne!(digest_a, DIGEST_B);
    assert!(
        !vault
            .previews()
            .join("A")
            .join("AAAA")
            .join(bytes::leaf(&uuid(11), &digest_a, 1616))
            .exists()
    );

    // ... so the file is a pyramid of its own, and one orphan.
    let orphans: Vec<&PlanItem> = plan
        .items
        .iter()
        .filter(|item| item.status == Status::Unmapped)
        .filter(|item| {
            item.payload
                .as_ref()
                .is_some_and(|payload| slashed(payload.path()).contains(DIGEST_B))
        })
        .collect();
    assert_eq!(orphans.len(), 1, "{:#?}", plan.items);
    let (first, first_four) = bytes::leaf_dir(&uuid(11));
    assert_eq!(
        slashed(orphans[0].payload.as_ref().expect("payload").path()),
        slashed(
            &vault
                .previews()
                .join(first)
                .join(first_four)
                .join(bytes::leaf(&uuid(11), DIGEST_B, 1616))
        ),
        "only the pair maps, so the file is named by the pair it carries"
    );
}

#[test]
fn modern_counts_one_unmapped_item_per_unclaimed_pyramid() {
    let plan = modern().plan(&Options::default());
    assert_eq!(
        plan.stats.unmapped, 2,
        "M10's two level files are one orphan, M11's file is the other"
    );
}

#[test]
fn modern_reports_a_modern_layout_and_the_shapes_its_files_took() {
    let vault = modern();
    let plan = vault.plan(&Options::default());

    assert_eq!(plan.cache.layout, Layout::Modern);
    assert_eq!(plan.cache.layout.as_str(), "modern");
    assert_eq!(plan.cache.db_version.as_deref(), Some("1504001"));
    assert_eq!(plan.cache.crs_version.as_deref(), Some("18.0.0"));

    // The cache counts are per vault, not per row: no pyramid here holds a
    // legacy container, and none of the files is a shape this reader cannot
    // name.
    assert_eq!(plan.stats.superseded, 0);
    assert_eq!(plan.stats.unrecognised, 0);

    // `shapes` is derived from the files the composition wrote, never from a
    // number copied out of `fixtures.md`.
    assert_eq!(
        plan.cache.shapes.level_file,
        vault.level_files().len() as u64,
        "every level file the builder wrote: {:?}",
        vault.level_files()
    );
    assert_eq!(plan.cache.shapes.container, 0);
    // Exactly one leaf names a long edge its own bytes do not measure: M5's
    // file is named `_1616` and its `SOF` says 800x600. That single difference
    // is what makes the count a measurement rather than a second reading of
    // the name.
    assert_eq!(
        plan.cache.shapes.suffix_matches_sof,
        plan.cache.shapes.level_file - 1,
        "only M5's `_1616` leaf disagrees with its own frame"
    );
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

#[test]
fn m13_a_payload_that_carries_metadata_keeps_the_stronger_evidence() {
    let vault = modern();
    let plan = vault.plan(&Options::default());
    let item = item_for(&plan, 13);

    assert_eq!(item.status, Status::Recoverable);
    assert_eq!(
        item.payload_metadata.to_string(),
        "exif,xmp,icc",
        "the inventory names what the payload already carried"
    );
    assert_eq!(
        item.orientation,
        Orientation::Normal,
        "the catalog's own claim stays on the item"
    );
    assert_eq!(
        item.effective_orientation,
        Orientation::Rotate90,
        "the payload's own EXIF orientation describes these pixels and wins"
    );

    run_extract(&vault, &plan);
    let output = written(item);
    let certified = jpeg::certify(&output).expect("a complete JPEG");
    assert!(certified.complete);

    // Ours replaces the payload's segment, never beside it, and it names us.
    let ours: Vec<jpeg::Segment> = certified
        .segments
        .iter()
        .filter(|segment| {
            segment.marker == 0xE1 && segment.payload(&output).starts_with(b"Exif\0\0")
        })
        .cloned()
        .collect();
    assert_eq!(ours.len(), 1, "exactly one EXIF segment: {ours:#?}");
    let software = ifd0_text(&output, exif::Tag::Software).expect("the marker");
    assert_eq!(
        software,
        lrsalvage_core::exif::marker(env!("CARGO_PKG_VERSION")),
        "the marker names the version this crate is, not one a test typed"
    );
    assert_eq!(
        ifd0_uint(&output, exif::Tag::Orientation),
        Some(6),
        "the payload's value is what the file states"
    );

    // The profile and the packet are the payload's own, byte for byte.
    assert_eq!(
        embedded_icc(&output).as_deref(),
        Some(PAYLOAD_ICC),
        "a reconstruction never displaces a real profile"
    );
    let segments = m13_segments();
    let mut tail = segments[1].clone();
    tail.extend_from_slice(&segments[2]);
    tail.extend_from_slice(&bytes::jpeg(1616, 1077)[2..]);
    assert_eq!(
        &output[ours[0].range.end..],
        &tail[..],
        "everything after our EXIF is the payload's own bytes (ADR 0003)"
    );
}

#[test]
fn m14_a_bare_payload_takes_the_catalogs_orientation_and_profile() {
    let vault = modern();
    let plan = vault.plan(&Options::default());
    let item = item_for(&plan, 14);

    assert_eq!(item.status, Status::Recoverable);
    assert!(
        item.payload_metadata.is_empty(),
        "ours is not the payload's: {}",
        item.payload_metadata
    );
    assert_eq!(
        item.orientation,
        Orientation::Rotate90,
        "the catalog says BC"
    );
    assert_eq!(
        item.effective_orientation,
        Orientation::Rotate90,
        "with no payload claim the catalog's orientation is the effective one"
    );

    run_extract(&vault, &plan);
    let output = written(item);

    assert_eq!(
        ifd0_uint(&output, exif::Tag::Orientation),
        Some(6),
        "0x0112 is written from the catalog's code"
    );
    let profile = lrsalvage_core::exif::icc_profile("AdobeRGB").expect("a built-in profile");
    assert_eq!(
        embedded_icc(&output).as_deref(),
        Some(profile),
        "our reconstructed profile is embedded when the payload carries none"
    );
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
    assert_eq!(
        (d1.width, d1.height),
        (Some(1616), Some(1077)),
        "the frame was readable; it is the end-of-image that is missing"
    );

    // A broken winner is reported, not hidden: the item is evaluated, and it
    // writes nothing. The intact 808 beside it is not silently promoted.
    let output = d1.output.as_ref().expect("output").clone();
    let outcomes = run_extract(&vault, &plan);
    let d1_outcome = outcomes
        .iter()
        .find(|outcome| outcome.output == output)
        .expect("the failed winner is still evaluated");
    assert_eq!(d1_outcome.status, extract::OutcomeStatus::Failed);
    assert!(!output.exists(), "nothing is written for a failed image");
}

#[test]
fn d2_bytes_that_were_never_a_jpeg_are_failed() {
    let vault = damaged();
    let plan = vault.plan(&Options::default());

    let d2 = item_for(&plan, 2);
    assert_eq!(d2.status, Status::Failed);
    assert!(!d2.output.as_ref().expect("output").exists());
    assert_eq!((d2.width, d2.height), (None, None));
    assert!(is_level_file(d2), "it is still the payload that failed");
}

#[test]
fn d3_a_payload_without_an_soi_is_failed_not_scanned() {
    let vault = damaged();
    let plan = vault.plan(&Options::default());

    let d3 = item_for(&plan, 3);
    assert_eq!(d3.status, Status::Failed);
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
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| outcome.status == extract::OutcomeStatus::Written)
            .count(),
        1,
        "only the healthy image is written: {outcomes:#?}"
    );
    assert_eq!(
        outcomes
            .iter()
            .find(|outcome| outcome.output == *d4.output.as_ref().expect("output"))
            .map(|outcome| outcome.status),
        Some(extract::OutcomeStatus::Written)
    );
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

#[test]
fn damaged_counts_the_frames_it_can_measure_even_when_they_are_incomplete() {
    let vault = damaged();
    let plan = vault.plan(&Options::default());

    assert_eq!(plan.cache.layout, Layout::Modern);
    assert_eq!(
        plan.cache.shapes.level_file,
        vault.level_files().len() as u64
    );
    // D1's truncated 1616 still declares its frame, so its leaf still matches;
    // D2's non-JPEG and D3's body-without-SOI declare nothing, so two of the
    // four files do not. Incompleteness is not the same as unreadable.
    assert_eq!(
        plan.cache.shapes.suffix_matches_sof,
        plan.cache.shapes.level_file - 2,
        "only the two unparseable files fail to name their own long edge"
    );
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
fn l6_an_unclaimed_container_is_one_orphan_item() {
    let vault = legacy();
    let plan = vault.plan(&Options::default());

    let pyramid = uuid(20);
    let items: Vec<&PlanItem> = plan
        .items
        .iter()
        .filter(|item| item.status == Status::Unmapped)
        .collect();
    assert_eq!(items.len(), 1, "one orphan pyramid: {items:#?}");

    let orphan = items[0];
    assert!(orphan.image.is_none());
    assert!(!orphan.selected);
    assert!(orphan.output.is_none());
    assert_eq!(
        slashed(orphan.payload.as_ref().expect("payload").path()),
        slashed(
            &vault
                .previews()
                .join("A")
                .join("AAAA")
                .join(format!("{pyramid}-{}.lrprev", digest_of(&pyramid)))
        ),
        "a pyramid with no level file is named by its container"
    );
    assert_eq!(
        certified_level(orphan),
        None,
        "nothing was certified, because nothing is written"
    );
    assert_eq!(plan.stats.unmapped, 1, "legacy's pinned `unmapped 1` is L6");
}

#[test]
fn legacy_reports_a_legacy_layout_with_no_level_files() {
    let vault = legacy();
    let plan = vault.plan(&Options::default());

    assert_eq!(plan.cache.layout, Layout::Legacy);
    assert_eq!(plan.cache.db_version.as_deref(), Some("1200001"));
    assert_eq!(plan.cache.crs_version.as_deref(), Some("15.4.0"));
    assert_eq!(
        plan.cache.shapes.container,
        vault.containers().len() as u64,
        "one container per pyramid, L1 and L6"
    );
    assert_eq!(plan.cache.shapes.level_file, 0);
    assert_eq!(plan.cache.shapes.suffix_matches_sof, 0);
    // A `.lrprev`-only pyramid is legacy and is read, never superseded.
    assert_eq!(plan.stats.superseded, 0);
}

#[test]
fn n1_a_wrongly_shaped_file_is_not_an_orphan() {
    let u = uuid(1);
    let vault = Vault::unrecognised()
        .image(ImageRow::new(1, "IMG_0001").cache(&u))
        .pyramid(PyramidRow::new(&u).level(1616, 1077))
        .row_without_file(Level::sized(&u, 1616, 1077))
        .unrecognised_file(UnrecognisedFile::no_suffix())
        .unrecognised_file(UnrecognisedFile::wrong_shard())
        .unrecognised_file(UnrecognisedFile::bad_uuid())
        .write();
    let plan = vault.plan(&Options::default());

    // Three files that are payload-like and matched no shape: counted, and one
    // item each they are not.
    assert_eq!(
        plan.stats.unmapped, 0,
        "no suffix, wrong shard and an unparseable uuid are not payloads: {:#?}",
        plan.items
    );
    assert_eq!(plan.stats.unrecognised, 3, "N1's three files");
    assert_eq!(
        plan.cache.layout,
        Layout::Unknown,
        "`unknown` is not `none`: the cache holds something this reader cannot name"
    );
    assert_eq!(plan.cache.shapes.level_file, 0);
    assert_eq!(plan.cache.shapes.suffix_matches_sof, 0);
    assert_eq!(plan.cache.shapes.container, 0);
    assert_eq!(item_for(&plan, 1).status, Status::MissingPayload);
}

#[test]
fn n2_a_version_no_release_produced_is_reported_verbatim_and_reads_anyway() {
    let u = uuid(1);
    let vault = Vault::unrecognised()
        .image(ImageRow::new(1, "IMG_0001").cache(&u))
        .pyramid(PyramidRow::new(&u).level(1616, 1077))
        .row_without_file(Level::sized(&u, 1616, 1077))
        .write();
    let plan = vault.plan(&Options::default());

    // No version refuses a run: the numbers are reasons to report, never to
    // gate (`adr-0004-detect-layout-not-version`).
    assert_eq!(plan.cache.db_version.as_deref(), Some("9900001"));
    assert_eq!(plan.cache.crs_version.as_deref(), Some("99.0.0"));
}

#[test]
fn e1_an_index_that_holds_nothing_is_the_none_layout() {
    let vault = Vault::empty()
        .image(ImageRow::new(1, "IMG_0001"))
        .image(ImageRow::new(2, "IMG_0002"))
        .write();
    let plan = vault.plan(&Options::default());

    assert_eq!(
        plan.cache.layout,
        Layout::None,
        "\"this cache holds nothing\" is a layout, not an error"
    );
    assert_eq!(plan.stats.no_cache_entry, 2);
    assert_eq!(plan.cache.shapes, Default::default(), "no file, no shape");
    assert_eq!(plan.stats.superseded, 0);
    assert_eq!(plan.stats.deleted, 0);
    assert_eq!(plan.stats.unrecognised, 0);
}

#[test]
fn x1_a_superseded_pyramid_is_read_from_its_level_files() {
    let vault = mixed();
    let plan = vault.plan(&Options::default());

    let x1 = item_for(&plan, 1);
    assert_eq!(x1.status, Status::Recoverable);
    assert_eq!((x1.width, x1.height), (Some(1616), Some(1077)));
    assert!(
        is_level_file(x1),
        "the level files are the pyramid: {:?}",
        x1.payload
    );

    // Premise: reading that container would fail the row, so a `Recoverable`
    // X1 is itself the proof that it was never read (ADR 0006 6). Its bytes
    // stay untouched too.
    let container = vault
        .containers()
        .first()
        .expect("the superseded `.lrprev`")
        .clone();
    let before = fs::read(&container).expect("read the superseded container");
    run_extract(&vault, &plan);
    assert_eq!(
        fs::read(&container).expect("read it again"),
        before,
        "a superseded container is never read and never rewritten"
    );
}

#[test]
fn mixed_reports_both_representations_and_one_superseded_pyramid() {
    let vault = mixed();
    let plan = vault.plan(&Options::default());

    assert_eq!(plan.cache.layout, Layout::Mixed);
    // X1 alone holds both representations; X2 is legacy and X3 modern, and
    // neither of those is superseded. `mixed` describes the cache, so the
    // per-pyramid fact is a separate count.
    assert_eq!(
        plan.stats.superseded, 1,
        "mixed's pinned `superseded 1` is X1"
    );
    assert_eq!(
        plan.cache.shapes.level_file,
        vault.level_files().len() as u64,
        "X1's and X3's level files"
    );
    assert_eq!(
        plan.cache.shapes.container,
        vault.containers().len() as u64,
        "X1's superseded container and X2's readable one"
    );
    assert_eq!(
        plan.cache.shapes.suffix_matches_sof, plan.cache.shapes.level_file,
        "every leaf here names the long edge its own frame measures"
    );
}

#[test]
fn x2_a_container_only_pyramid_in_a_modern_cache_is_read_from_the_container() {
    let vault = mixed();
    let plan = vault.plan(&Options::default());

    let x2 = item_for(&plan, 2);
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

    let x3 = item_for(&plan, 3);
    assert_eq!(x3.status, Status::Recoverable);
    assert_eq!((x3.width, x3.height), (Some(1616), Some(1077)));
    assert!(is_level_file(x3), "{:?}", x3.payload);
    assert_eq!(
        slashed(x3.payload.as_ref().expect("payload").path()),
        slashed(&level_path(vault.previews(), &uuid(3), 1616)),
        "the level file's own leaf is the payload"
    );
}

// ---------------------------------------------------------------------------
// `unindexed` — an absent index is a mode, not an error
// ---------------------------------------------------------------------------

/// U1's vault: two images, two pyramids (the second with two levels), no
/// `previews.db` at all.
fn unindexed() -> Vault {
    let (first, second) = (uuid(1), uuid(2));
    Vault::unindexed()
        .image(ImageRow::new(1, "IMG_0001"))
        .image(ImageRow::new(2, "IMG_0002"))
        .level_file(Level::sized(&first, 1616, 1077))
        .level_file(Level::sized(&second, 1616, 1077))
        .level_file(Level::sized(&second, 808, 539))
        .write()
}

#[test]
fn u1_an_indexless_cache_lists_every_pyramid_and_claims_none() {
    let vault = unindexed();
    let plan = vault.plan(&Options::default());

    // ADR 0006 ¶1: no index means no *mapping*, so every image has no cache
    // entry and every payload pyramid is an orphan. Nothing is invented to fill
    // the gap (ADR 0002).
    assert_eq!(plan.stats.images, 2);
    assert_eq!(plan.stats.no_cache_entry, 2);
    assert_eq!(plan.stats.unmapped, 2);
    assert_eq!(plan.stats.recoverable, 0);
    assert_eq!(
        plan.selected_images(),
        0,
        "nothing is named, so nothing is selected"
    );
    for image in plan.items.iter().filter(|item| item.image.is_some()) {
        assert_eq!(image.status, Status::NoCacheEntry);
        assert!(!image.selected);
    }

    // The layout is detected from the tree, because there are no rows to ask.
    assert_eq!(plan.cache.layout, Layout::Modern);
    assert_eq!(plan.cache.shapes.level_file, 3);
    assert_eq!(plan.cache.shapes.container, 0);
    assert_eq!(
        plan.cache.shapes.suffix_matches_sof, 3,
        "identity came from the leaf, and every leaf names its own frame"
    );

    // One orphan per pyramid, never one per level file.
    let orphans: Vec<&PlanItem> = plan
        .items
        .iter()
        .filter(|item| item.status == Status::Unmapped)
        .collect();
    assert_eq!(orphans.len(), 2, "{:#?}", plan.items);
    for orphan in &orphans {
        assert!(orphan.image.is_none(), "nothing claims it");
        assert!(!orphan.selected, "listed, never written");
        assert!(orphan.output.is_none(), "an orphan has no output path");
        assert!(is_level_file(orphan), "{:?}", orphan.payload);
    }
    assert!(
        orphans.iter().any(
            |orphan| slashed(orphan.payload.as_ref().expect("payload").path()).ends_with("_1616")
                && slashed(orphan.payload.as_ref().expect("payload").path()).contains(&uuid(2))
        ),
        "the two-level pyramid is named by its largest level: {:#?}",
        orphans
            .iter()
            .map(|orphan| &orphan.payload)
            .collect::<Vec<_>>()
    );
}

#[test]
fn an_unclaimed_pyramid_is_named_by_its_largest_measured_level() {
    // The names lie: `_640` holds a 1616 frame and `_1616` a 640 one. ADR 0005
    // ¶9 ranks by the measured `SOF` area in every mode, so the payload this
    // orphan reports is the file whose *bytes* are the largest, and the shapes
    // histogram says out loud that no leaf here names its own frame.
    let lying = uuid(30);
    let indexed = Vault::modern()
        .unclaimed(&[
            Level::sized(&lying, 640, 427).raw(&bytes::jpeg(1616, 1077)),
            Level::sized(&lying, 1616, 1077).raw(&bytes::jpeg(640, 427)),
        ])
        .write();
    let plan = indexed.plan(&Options::default());

    let orphan = plan
        .items
        .iter()
        .find(|item| item.status == Status::Unmapped)
        .unwrap_or_else(|| panic!("one orphan: {:#?}", plan.items));
    assert_eq!(
        slashed(orphan.payload.as_ref().expect("payload").path()),
        slashed(&level_path(indexed.previews(), &lying, 640)),
        "the bytes are the verdict, not the suffix"
    );
    assert_eq!(
        plan.cache.shapes.suffix_matches_sof, 0,
        "neither leaf names the long edge its own frame measures"
    );
}

#[test]
fn an_unreadable_index_is_an_empty_one() {
    // ADR 0006 ¶2: a damaged file degrades exactly like a missing one, and the
    // reason travels with it instead of aborting the run.
    let vault = Vault::modern()
        .image(ImageRow::new(1, "IMG_0001").cache(&uuid(1)))
        .pyramid(PyramidRow::new(&uuid(1)).level(1616, 1077))
        .level_file(Level::sized(&uuid(1), 1616, 1077))
        .raw_index(&bytes::not_a_jpeg())
        .write();

    let opened = lrsalvage_core::previews_db::open(&vault.previews().join("previews.db"));
    assert!(
        matches!(opened.problem, Some(IndexProblem::Unreadable(_))),
        "a torn file reports why: {:?}",
        opened.problem
    );
    assert!(opened.index.entries.is_empty());

    let plan = vault.plan(&Options::default());
    assert_eq!(plan.stats.no_cache_entry, 1);
    assert_eq!(plan.stats.unmapped, 1);
    assert_eq!(plan.cache.layout, Layout::Modern);
}

// ---------------------------------------------------------------------------
// M16–M18, L7: `--max-long-edge`
//
// The cap is the floor's counterpart, so its rows read the same way: a bound
// that chooses a level, never a status. ADR 0013 is the reasoning.
// ---------------------------------------------------------------------------

/// M16–M18's pyramid: four level files, one row each, from 320 to 3840 — the
/// doubling ladder §5.7 measured a 1:1 build writing.
fn capped() -> Vault {
    let pyramid = uuid(31);
    let mut vault = Vault::modern()
        .image(ImageRow::new(31, "IMG_0031").cache(&pyramid))
        .pyramid(
            PyramidRow::new(&pyramid)
                .profile("sRGB")
                .level(320, 213)
                .level(960, 640)
                .level(1920, 1280)
                .level(3840, 2560),
        );
    for (width, height) in [(320, 213), (960, 640), (1920, 1280), (3840, 2560)] {
        vault = vault.level_file(Level::sized(&pyramid, width, height));
    }
    vault.write()
}

#[test]
fn m16_a_cap_chooses_the_largest_level_at_or_below_it() {
    let vault = capped();
    let plan = vault.plan(&Options {
        max_long_edge: 1920,
        ..Options::default()
    });

    let m16 = item_for(&plan, 31);
    assert_eq!(m16.status, Status::Recoverable);
    assert_eq!(
        (m16.width, m16.height),
        (Some(1920), Some(1280)),
        "the winner is the 1920 level, not the 3840 present beside it"
    );
    assert!(m16.selected, "a level that clears the cap is written");
    assert_eq!(
        m16.declared,
        Some((1920, 1280)),
        "the chosen level is judged against its own row"
    );
    let expected = level_path(vault.previews(), &uuid(31), 1920);
    assert_eq!(
        slashed(m16.payload.as_ref().expect("payload").path()),
        slashed(&expected)
    );
    assert_eq!(
        plan.stats.above_cap, 0,
        "without `--all-levels` the levels above the cap are not items, \
         exactly as levels below the floor are not"
    );
    assert_eq!(plan.stats.levels, 1);

    // What the cap chose owns the image's plain name.
    let outcomes = run_extract(&vault, &plan);
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        jpeg::dimensions(&fs::read(&outcomes[0].output).expect("the written file"))
            .expect("a readable frame"),
        (1920, 1280)
    );
}

#[test]
fn m17_a_cap_no_level_clears_writes_nothing_and_is_not_a_failure() {
    let pyramid = uuid(32);
    let vault = Vault::modern()
        .image(ImageRow::new(32, "IMG_0032").cache(&pyramid))
        .pyramid(PyramidRow::new(&pyramid).level(3840, 2560))
        .level_file(Level::sized(&pyramid, 3840, 2560))
        .write();

    let plan = vault.plan(&Options {
        max_long_edge: 1920,
        ..Options::default()
    });

    let m17 = item_for(&plan, 32);
    assert_eq!(
        m17.status,
        Status::Recoverable,
        "a cap leaves an image unwritten, never failed: the payload is a good photograph"
    );
    assert_eq!((m17.width, m17.height), (Some(3840), Some(2560)));
    assert_eq!(m17.tier, Some(Tier::Large));
    assert!(
        !m17.selected,
        "above the cap: counted and named, not written"
    );
    assert_eq!(plan.stats.above_cap, 1);
    assert_eq!(plan.stats.failed, 0);
    assert_eq!(plan.selected_images(), 0);
    assert!(
        run_extract(&vault, &plan).is_empty(),
        "an above-cap item writes no JPEG"
    );
}

#[test]
fn m18_all_levels_bounds_both_ends_of_the_ladder() {
    let vault = capped();
    let plan = vault.plan(&Options {
        min_long_edge: 640,
        max_long_edge: 1920,
        all_levels: true,
        ..Options::default()
    });

    let items = items_for(&plan, 31);
    assert_eq!(
        items.len(),
        4,
        "every level is an item under `--all-levels`"
    );
    assert_eq!(
        plan.stats.levels, 4,
        "and `levels` still counts levels, not images"
    );
    assert_eq!(plan.stats.below_floor, 1, "the 320 is below the floor");
    assert_eq!(plan.stats.above_cap, 1, "the 3840 is above the cap");
    assert_eq!(plan.stats.selected_items, 2, "960 and 1920 are written");

    let mut written: Vec<u32> = items
        .iter()
        .filter(|item| item.selected)
        .filter_map(|item| item.width)
        .collect();
    written.sort_unstable();
    assert_eq!(written, vec![960, 1920]);

    // The plain name belongs to the level the cap chose; the smaller survivor
    // carries the size suffix `--all-levels` gives an extra level.
    let plain: Vec<u32> = items
        .iter()
        .filter(|item| {
            item.output
                .as_ref()
                .and_then(|path| path.file_name())
                .is_some_and(|name| name.to_string_lossy().starts_with("IMG_0031."))
        })
        .filter_map(|item| item.width)
        .collect();
    assert_eq!(plain, vec![1920], "`IMG_0031.jpg` is the cap's choice");
    let suffixed: Vec<String> = items
        .iter()
        .filter(|item| {
            item.output
                .as_ref()
                .and_then(|path| path.file_name())
                .is_some_and(|name| name.to_string_lossy().contains(" - "))
        })
        .filter_map(|item| item.output.as_ref())
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(
        suffixed.contains(&"IMG_0031 - 960x640.jpg".to_string()),
        "{suffixed:?}"
    );
}

#[test]
fn l7_a_cap_ranks_a_containers_decoded_levels() {
    let pyramid = uuid(40);
    let vault = Vault::legacy()
        .image(ImageRow::new(40, "IMG_0040").cache(&pyramid))
        .pyramid(
            PyramidRow::new(&pyramid)
                .level(101, 68)
                .level(808, 539)
                .level(1616, 1077),
        )
        .container(&[
            Level::sized(&pyramid, 101, 68),
            Level::sized(&pyramid, 808, 539),
            Level::sized(&pyramid, 1616, 1077),
        ])
        .write();

    let plan = vault.plan(&Options {
        max_long_edge: 1000,
        ..Options::default()
    });

    let l7 = item_for(&plan, 40);
    assert_eq!(l7.status, Status::Recoverable);
    assert_eq!(
        (l7.width, l7.height),
        (Some(808), Some(539)),
        "the container is ranked by decoded area and the cap picks the 808 chunk"
    );
    assert!(l7.selected);
    let outcomes = run_extract(&vault, &plan);
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        jpeg::dimensions(&fs::read(&outcomes[0].output).expect("the written file"))
            .expect("a readable frame"),
        (808, 539)
    );
}
