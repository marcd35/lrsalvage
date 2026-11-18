//! The fixture matrix, rows whose medium is a run.
//!
//! Every test drives the real binary through the command line and asserts what
//! only a run can say: the exit code a row names, and the bytes of the tree it
//! left behind. The catalog, the cache and every payload come from
//! `lrsalvage-fixtures`; no fixture lives in this file.
//!
//! **No image total and no file count is asserted.** A run's `images`/`recovered`
//! counts are the vault's pinned totals, which this slice cannot produce, so each
//! vault here is built from the rows these runs need (M1, M6 and M10 for
//! `modern`; D1 and D4 for `damaged`; M1, M5, M6 and M10 for `modern with a
//! suspect`, which the verification-report rows run against) rather than whole. A
//! count asserted by
//! accident would read as a fixture bug rather than a promise. The rows' full
//! shapes and their structured fields are asserted in `lrsalvage-core`'s matrix.
//!
//! What a run does assert is the cache's shape: the layout word, the two version
//! strings, the three cache counts and the `shapes` triple, in `--json` and in
//! `manifest.json`, plus the trailing `payload` column of a CSV row — and the
//! verification report `--report` writes (rows R1–R11), which is asserted by
//! absence as well as by field set.
//!
//! **No exit-code row was added for the widening or the orphan sweep, because no
//! run's code moved.** An unclaimed payload is listed and never selected, so it
//! can neither make a run partial nor turn "nothing to extract" into work; a
//! row-less level file is simply the level the image is written from. The codes
//! asserted by the `modern`, `damaged` and `unrecognised` runs are the same
//! before and after, which is the claim — not an omission to be filled in later.
//!
//! The `unindexed` rows *do* assert codes that moved, which is their reason to
//! exist: a missing index used to die with a SQLite error and exit 1, and a
//! damaged one the same way (ADR 0006 ¶1–¶2), while a `--previews` that is not a
//! directory stayed exit 1 on purpose (¶3).

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use lrsalvage_core::jpeg;
use lrsalvage_fixtures::bytes;
use lrsalvage_fixtures::vault::{ImageRow, Level, PyramidRow, UnrecognisedFile, Vault};
use predicates::prelude::*;
use serde_json::Value;

/// One distinct uuid per row, so no two pyramids share an address.
fn uuid(row: u32) -> String {
    format!("AAAA{row:04}-1111-4111-8111-{row:012}")
}

/// The XMP packet M13's payload carries: a foreign one, never ours.
const PAYLOAD_XMP: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:Rating="2"/>
 </rdf:RDF>
</x:xmpmeta>"#;

/// M13's payload: its own EXIF, XMP and ICC profile in front of a bare frame.
fn m13_payload() -> Vec<u8> {
    bytes::jpeg_with(
        1616,
        1077,
        &[
            bytes::exif_app1(6),
            bytes::xmp_app1(PAYLOAD_XMP),
            bytes::icc_app2(b"a profile this repository did not build"),
        ],
    )
}

fn lrsalvage() -> Command {
    Command::cargo_bin("lrsalvage").expect("the integration-test binary path")
}

/// `modern`, with the rows these runs need.
///
/// | id | row |
/// |----|-----|
/// | M1 | recovered, one level file: a bare payload |
/// | M13 | recovered, and its payload already carries EXIF, XMP and ICC |
/// | M6 | a tombstone over a pyramid with no file: the `deleted` count |
/// | M10 | one unclaimed pyramid: the orphan the reports must name |
fn modern() -> Vault {
    let (m1, m6) = (uuid(1), uuid(6));
    let m13 = uuid(13);
    let m10 = uuid(20);
    Vault::modern()
        .image(ImageRow::new(1, "IMG_0001").cache(&m1))
        .pyramid(PyramidRow::new(&m1).level(1616, 1077))
        .level_file(Level::sized(&m1, 1616, 1077))
        .image(ImageRow::new(13, "IMG_0013").cache(&m13))
        .pyramid(PyramidRow::new(&m13).profile("AdobeRGB").level(1616, 1077))
        .level_file(Level::sized(&m13, 1616, 1077).raw(&m13_payload()))
        .image(ImageRow::new(6, "IMG_0006").cache(&m6))
        .pyramid(PyramidRow::new(&m6).level(1616, 1077))
        .tombstone(&m6, 700_000_000.0)
        .unclaimed(&[Level::sized(&m10, 640, 427)])
        .write()
}

/// `unrecognised`: an index and three payload-like files of no known shape.
fn unrecognised() -> Vault {
    let u = uuid(1);
    Vault::unrecognised()
        .image(ImageRow::new(1, "IMG_0001").cache(&u))
        .pyramid(PyramidRow::new(&u).level(1616, 1077))
        .row_without_file(Level::sized(&u, 1616, 1077))
        .unrecognised_file(UnrecognisedFile::no_suffix())
        .unrecognised_file(UnrecognisedFile::wrong_shard())
        .unrecognised_file(UnrecognisedFile::bad_uuid())
        .write()
}

fn scan(vault: &Vault) -> Command {
    let mut command = lrsalvage();
    command
        .arg("scan")
        .arg("--catalog")
        .arg(vault.catalog())
        .arg("--previews")
        .arg(vault.previews());
    command
}

/// The stdout of a run, parsed as the one object `--json` promises.
fn scan_json(vault: &Vault) -> Value {
    let output = scan(vault).arg("--json").output().expect("run scan --json");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not one JSON object ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// `damaged`, with the two rows an exit-2 run needs: D1 (truncated 1616 winner,
/// intact 808 beside it, which is also D5) and the healthy D4.
fn damaged() -> Vault {
    let (d1, d4) = (uuid(1), uuid(4));
    Vault::damaged()
        .image(ImageRow::new(1, "IMG_0001").cache(&d1))
        .pyramid(PyramidRow::new(&d1).level(1616, 1077).level(808, 539))
        .level_file(Level::sized(&d1, 1616, 1077).raw(&bytes::truncated_jpeg(1616, 1077)))
        .level_file(Level::sized(&d1, 808, 539))
        .image(ImageRow::new(4, "IMG_0004").cache(&d4))
        .pyramid(PyramidRow::new(&d4).level(1616, 1077))
        .level_file(Level::sized(&d4, 1616, 1077))
        .write()
}

fn extract(vault: &Vault) -> Command {
    let mut command = lrsalvage();
    command
        .arg("extract")
        .arg("--catalog")
        .arg(vault.catalog())
        .arg("--previews")
        .arg(vault.previews())
        .arg("--out")
        .arg(vault.out());
    command
}

/// Every `.jpg` below `root`, by name.
fn jpeg_names(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            let path = entry.expect("a readable entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("jpg"))
            {
                found.push(path.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
    }
    found.sort();
    found
}

/// Every `.jpg` below `root`, by name, with the bytes on disk.
fn outputs(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    jpeg_names(root)
        .into_iter()
        .map(|name| {
            let bytes = fs::read(jpeg_named(root, &name)).expect("read a written file");
            (name, bytes)
        })
        .collect()
}

/// The payloads of every `Exif` APP1 segment in `bytes`.
fn exif_payloads(bytes: &[u8]) -> Vec<&[u8]> {
    let certified = jpeg::certify(bytes).expect("a written file is a JPEG");
    certified
        .segments
        .iter()
        .filter(|segment| segment.marker == 0xE1)
        .map(|segment| segment.payload(bytes))
        .filter(|payload| payload.starts_with(b"Exif\0\0"))
        .collect()
}

/// `true` when `haystack` contains `needle`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// The one `.jpg` below `root` with this name.
fn jpeg_named(root: &Path, name: &str) -> PathBuf {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            let path = entry.expect("a readable entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|file| file == name) {
                return path;
            }
        }
    }
    panic!("no {name} under {}: {:?}", root.display(), jpeg_names(root));
}

#[test]
fn scan_json_and_the_summary_name_the_layout_and_the_cache_counts() {
    let vault = modern();

    let object = scan_json(&vault);
    assert_eq!(object["layout"], "modern");
    assert_eq!(object["db_version"], "1504001");
    assert_eq!(object["crs_version"], "18.0.0");
    assert_eq!(object["superseded"], 0);
    assert_eq!(object["deleted"], 1, "M6's tombstone");
    assert_eq!(object["unrecognised"], 0);
    // One shape per file the builder wrote, and every leaf here names the long
    // edge its own frame measures.
    assert_eq!(
        object["shapes"]["level_file"],
        vault.level_files().len() as u64
    );
    assert_eq!(
        object["shapes"]["suffix_matches_sof"],
        vault.level_files().len() as u64
    );
    assert_eq!(object["shapes"]["container"], 0);

    // The text summary leads with one line naming the layout and nothing else,
    // and a cache count follows as its own row.
    let output = scan(&vault).output().expect("run scan");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.starts_with("cache layout: modern\n"),
        "the layout is the first line:\n{text}"
    );
    assert!(text.contains("deleted"), "{text}");
    assert!(!text.contains("superseded"), "zero, so omitted:\n{text}");
}

#[test]
fn scan_json_reports_an_unknown_layout_and_counts_what_it_could_not_name() {
    let vault = unrecognised();

    // A version no release produced is reported, never used as a gate, and N1's
    // run exits 3 because nothing can be selected — with one hint, on stderr, so
    // stdout stays the one object `--json` promises.
    let output = scan(&vault)
        .arg("--json")
        .output()
        .expect("run scan --json");
    assert_eq!(output.status.code(), Some(3));
    let object: Value = serde_json::from_slice(&output.stdout).expect("one object on stdout");
    assert_eq!(object["layout"], "unknown");
    assert_eq!(object["db_version"], "9900001");
    assert_eq!(object["crs_version"], "99.0.0");
    assert_eq!(object["unrecognised"], 3);
    assert_eq!(object["unmapped"], 0, "unrecognised is not an orphan");
    assert_eq!(object["shapes"]["level_file"], 0);
    assert_eq!(object["shapes"]["container"], 0);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("3 payload-like files in the cache match no layout"),
        "the layout word alone does not say why nothing could be named:
{stderr}"
    );
    assert!(
        stderr.contains("never written"),
        "and the hint says what became of them:
{stderr}"
    );
}

#[test]
fn extract_names_each_payload_in_the_csv_and_the_manifest() {
    let vault = modern();

    extract(&vault).assert().code(0);

    // `report.csv` ends with `payload` and then `payload_metadata`, after
    // `note`, and an unclaimed payload carries its own path where an output
    // would be.
    let csv = fs::read_to_string(vault.out().join("report.csv")).expect("report.csv");
    let mut lines = csv.lines();
    let header = lines.next().expect("a header");
    let rows: Vec<&str> = lines.collect();
    assert!(
        header.ends_with(",note,payload,payload_metadata"),
        "the trailing columns are the payload and its own metadata:\n{header}"
    );
    let orphan = rows
        .iter()
        .find(|row| row.starts_with("unmapped"))
        .expect("the unclaimed pyramid");
    let fields: Vec<&str> = orphan.split(',').collect();
    assert_eq!(fields.len(), 16, "{orphan}");
    assert_eq!(fields[9], "", "nothing is written for an orphan");
    assert_eq!(fields[10], "", "and it has no original path");
    assert!(
        fields[14].contains(&uuid(20)) && fields[14].ends_with("_640"),
        "the payload column names the largest level it holds: {}",
        fields[14]
    );
    assert_eq!(
        fields[15], "",
        "a cache payload of this shape carries no metadata of its own"
    );

    // M13's payload carries all three, so the row names them as one quoted
    // field and its orientation column is the value actually written — the
    // payload's own, not the catalog's `AB`. Its note carries commas too, so the
    // trailing field is read from the end rather than by counting a split.
    let carried = rows
        .iter()
        .find(|row| row.contains("IMG_0013.jpg"))
        .expect("M13's row");
    assert_eq!(
        carried.split(',').nth(7),
        Some("BC"),
        "the effective orientation is printed: {carried}"
    );
    assert!(
        carried.ends_with(",\"exif,xmp,icc\""),
        "one token list, one quoted field: {carried}"
    );

    // The manifest carries the cache's shape too, and one `payload` member per
    // item — the orphan's is the only one with no status image behind it.
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(vault.out().join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["layout"], "modern");
    assert_eq!(
        manifest["version"],
        env!("CARGO_PKG_VERSION"),
        "the manifest records the version the run was, so a bump that misses it fails"
    );
    assert_eq!(manifest["db_version"], "1504001");
    assert_eq!(manifest["crs_version"], "18.0.0");
    assert_eq!(manifest["stats"]["superseded"], 0);
    assert_eq!(manifest["stats"]["deleted"], 1);
    assert_eq!(manifest["stats"]["unrecognised"], 0);
    assert_eq!(
        manifest["shapes"]["level_file"],
        vault.level_files().len() as u64
    );
    let orphans: Vec<&Value> = manifest["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter(|item| item["status"] == "unmapped")
        .collect();
    assert_eq!(orphans.len(), 1, "one orphan, never one per file");
    assert!(
        orphans[0]["payload"]
            .as_str()
            .is_some_and(|payload| payload.contains(&uuid(20))),
        "the orphan's payload is a member, not prose: {}",
        orphans[0]
    );
    assert!(
        orphans[0].get("payload_metadata").is_none(),
        "absent stays absent: a bare payload adds no member"
    );

    // The item whose payload carries metadata says so, and its orientation is
    // the effective one, exactly as the CSV's column is.
    let carried = manifest["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|item| {
            item["output"]
                .as_str()
                .is_some_and(|output| output.ends_with("IMG_0013.jpg"))
        })
        .expect("M13's manifest item");
    assert_eq!(carried["payload_metadata"], "exif,xmp,icc");
    assert_eq!(carried["orientation"], "BC");
}

#[test]
fn extract_on_a_modern_cache_exits_zero_and_writes_every_selected_level() {
    let vault = modern();

    extract(&vault).assert().code(0);

    assert_eq!(
        jpeg_names(vault.out()),
        vec!["IMG_0001.jpg", "IMG_0013.jpg"],
        "one file per selected item, and no `.partial` left behind"
    );
    for name in jpeg_names(vault.out()) {
        let written = fs::read(jpeg_named(vault.out(), &name)).expect("read the recovered file");
        let certified = jpeg::certify(&written).expect("a complete JPEG");
        assert!(certified.complete, "{name} must end with EOI");
        assert_eq!(certified.dimensions, (1616, 1077), "{name}");
        let exif = exif_payloads(&written);
        assert_eq!(
            exif.len(),
            1,
            "{name} carries exactly one Exif APP1, ours: M13's is replaced"
        );
        assert!(
            contains(exif[0], b"lrsalvage "),
            "{name} names the tool that wrote it"
        );
    }
}

#[test]
fn extract_skips_every_output_its_own_earlier_run_wrote() {
    let vault = modern();
    extract(&vault).assert().code(0);
    let before = outputs(vault.out());
    assert_eq!(before.len(), 2, "two selected items: {before:#?}");

    extract(&vault)
        .assert()
        .code(0)
        .stderr(predicate::str::contains("0 written"))
        .stderr(predicate::str::contains("2 skipped"));

    assert_eq!(
        outputs(vault.out()),
        before,
        "the marker, completeness and the frame are what a rerun trusts"
    );
}

/// M15's second photograph carries this packet, so its absence from the file on
/// disk proves the skip never wrote the second catalog's image.
const SECOND_CATALOG_XMP: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:Rating="3"/>
 </rdf:RDF>
</x:xmpmeta>"#;

/// The `Software` value M15's foreign file carries.
///
/// It names this tool without *preceding* with it: the marker is a prefix, so
/// `contains("lrsalvage")` would wrongly keep this file and `starts_with` does
/// not.
const FOREIGN_SOFTWARE: &str = "lrsalvage-compatible importer 1.2";

#[test]
fn m15_a_second_catalog_skips_the_destination_its_predecessor_wrote() {
    // The first catalog: three images recovered into the folder, plus a file
    // another tool had already written into it at a fourth name.
    let (a1, a2, a3) = (uuid(101), uuid(102), uuid(103));
    let first = Vault::modern()
        .image(ImageRow::new(1, "IMG_0001").cache(&a1))
        .pyramid(PyramidRow::new(&a1).level(1616, 1077))
        .level_file(Level::sized(&a1, 1616, 1077))
        .image(ImageRow::new(2, "IMG_0002").cache(&a2))
        .pyramid(PyramidRow::new(&a2).level(1616, 1077))
        .level_file(Level::sized(&a2, 1616, 1077))
        .image(ImageRow::new(3, "IMG_0003").cache(&a3))
        .pyramid(PyramidRow::new(&a3).level(960, 1077))
        .level_file(Level::sized(&a3, 960, 1077))
        .preoccupied_destination(
            "originals/IMG_0004.jpg",
            &bytes::foreign_output(1616, 1077, FOREIGN_SOFTWARE),
        )
        .write();
    extract(&first).assert().code(0);
    let before = outputs(first.out());
    assert_eq!(
        before.len(),
        4,
        "three of ours and the foreign file: {before:#?}"
    );

    // The second catalog: its own library, the same destination, and every one
    // of its names already taken — a different photograph of the same size, a
    // size sharing only the width with the file on disk, one sharing only the
    // height, and the same size behind another tool's marker.
    let (b1, b2, b3, b4) = (uuid(201), uuid(202), uuid(203), uuid(204));
    let second = Vault::modern()
        .same_destination_as(&first)
        .image(ImageRow::new(1, "IMG_0001").cache(&b1))
        .pyramid(PyramidRow::new(&b1).level(1616, 1077))
        .level_file(Level::sized(&b1, 1616, 1077).raw(&bytes::jpeg_with(
            1616,
            1077,
            &[bytes::xmp_app1(SECOND_CATALOG_XMP)],
        )))
        .image(ImageRow::new(2, "IMG_0002").cache(&b2))
        .pyramid(PyramidRow::new(&b2).level(1616, 640))
        .level_file(Level::sized(&b2, 1616, 640))
        .image(ImageRow::new(3, "IMG_0003").cache(&b3))
        .pyramid(PyramidRow::new(&b3).level(640, 1077))
        .level_file(Level::sized(&b3, 640, 1077))
        .image(ImageRow::new(4, "IMG_0004").cache(&b4))
        .pyramid(PyramidRow::new(&b4).level(1616, 1077))
        .level_file(Level::sized(&b4, 1616, 1077))
        .write();

    let run = extract(&second)
        .output()
        .expect("run the second catalog's extract");
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert_eq!(
        run.status.code(),
        Some(2),
        "the three refusals make the run partial:\n{stderr}"
    );
    // One skip and three refusals: the identity is the marker prefix *and* both
    // dimensions, and each has a destination only it decides. A comparison on
    // one component alone turns its arm into a second skip, so this count is the
    // assertion that keeps each half in place.
    assert!(
        stderr.contains("4 evaluated, 0 written, 1 skipped, 3 failed"),
        "nothing is written, and exactly one destination is taken as ours:\n{stderr}"
    );

    // The collision: the file on disk is still the first catalog's photograph,
    // and the second catalog's own bytes never reached it.
    assert_eq!(
        outputs(second.out()),
        before,
        "the shared tree is byte-identical: nothing at all was written"
    );
    let collided = &before["IMG_0001.jpg"];
    assert!(
        contains(collided, b"lrsalvage "),
        "the skipped file is one of our own outputs"
    );
    assert!(
        !contains(collided, SECOND_CATALOG_XMP.as_bytes()),
        "and never the second catalog's photograph"
    );

    // The run's own record names the collision as already recovered — the
    // report this row exists to pin. The first catalog's run claimed
    // `extract.csv`, so this one is the numbered file. The status column is the
    // structured field this row pins; the note beside it is prose, and prose is
    // never asserted (`specs/fixtures.md`).
    let record = fs::read_to_string(second.out().join("extract (2).csv"))
        .expect("the second run's outcome record");
    let skipped: Vec<&str> = record
        .lines()
        .filter(|line| line.starts_with("skipped"))
        .collect();
    assert_eq!(
        skipped.len(),
        1,
        "only the same-name same-size file is skipped:\n{record}"
    );
    assert!(skipped[0].contains("IMG_0001.jpg"), "{}", skipped[0]);
    assert!(
        stderr.contains("not a verified extraction"),
        "the other three are refused, not kept:\n{stderr}"
    );
}

#[test]
fn a_metadata_free_run_never_verifies_anything() {
    let vault = modern();

    extract(&vault).arg("--no-metadata").assert().code(0);
    let payloads: Vec<Vec<u8>> = vault
        .level_files()
        .iter()
        .map(|path| fs::read(path).expect("read a payload"))
        .collect();
    let before = outputs(vault.out());
    assert_eq!(before.len(), 2);
    for (name, bytes) in &before {
        assert!(
            payloads.contains(bytes),
            "{name} is the payload's own bytes, verbatim"
        );
    }

    // A metadata run must not silently keep what a metadata-free run wrote:
    // there is no marker to prove authorship.
    extract(&vault)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("not a verified extraction"));
    assert_eq!(
        outputs(vault.out()),
        before,
        "every output is left byte-identical"
    );
}

#[test]
fn extract_on_a_damaged_cache_exits_partial_and_writes_only_the_healthy_image() {
    let vault = damaged();

    extract(&vault)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("failed:"));

    assert_eq!(
        jpeg_names(vault.out()),
        vec!["IMG_0004.jpg"],
        "nothing is written for the image whose winner is broken"
    );
    let written = fs::read(jpeg_named(vault.out(), "IMG_0004.jpg")).expect("read the file");
    let certified = jpeg::certify(&written).expect("a complete JPEG");
    assert!(certified.complete);
    assert_eq!(certified.dimensions, (1616, 1077));
}

#[test]
fn extract_all_levels_on_a_damaged_cache_writes_the_good_level_under_its_size() {
    let vault = damaged();

    extract(&vault)
        .arg("--all-levels")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("failed:"));

    // D5: the plain name belongs to the failed 1616, which writes nothing; the
    // good 808 is written with a size suffix.
    assert_eq!(
        jpeg_names(vault.out()),
        vec!["IMG_0001 - 808x539.jpg", "IMG_0004.jpg"]
    );
    let suffixed = fs::read(jpeg_named(vault.out(), "IMG_0001 - 808x539.jpg")).expect("read");
    let certified = jpeg::certify(&suffixed).expect("a complete JPEG");
    assert!(certified.complete);
    assert_eq!(certified.dimensions, (808, 539));
    assert!(
        !vault.out().join("originals").join("IMG_0001.jpg").exists(),
        "the failed level keeps the plain name and writes nothing"
    );
}

/// A pyramid whose levels are the doubling ladder §5.7 measured a 1:1 build
/// writing: 320, 960, 1920, 3840.
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
fn m16_a_cap_writes_the_largest_level_under_it() {
    let vault = capped();

    extract(&vault)
        .arg("--max-long-edge")
        .arg("1920")
        .assert()
        .code(0);

    assert_eq!(
        jpeg_names(vault.out()),
        vec!["IMG_0031.jpg"],
        "one file, under the image's plain name"
    );
    let bytes = fs::read(vault.out().join("originals").join("IMG_0031.jpg")).expect("the output");
    assert_eq!(
        jpeg::dimensions(&bytes).expect("a readable frame"),
        (1920, 1280),
        "the cap chooses the level, it does not merely hide the bigger one"
    );
}

#[test]
fn m17_a_cap_no_level_clears_exits_three_and_writes_nothing() {
    let pyramid = uuid(32);
    let vault = Vault::modern()
        .image(ImageRow::new(32, "IMG_0032").cache(&pyramid))
        .pyramid(PyramidRow::new(&pyramid).level(3840, 2560))
        .level_file(Level::sized(&pyramid, 3840, 2560))
        .write();

    extract(&vault)
        .arg("--max-long-edge")
        .arg("1920")
        .assert()
        .code(3)
        .stderr(predicate::str::contains("nothing to extract"));

    assert!(
        jpeg_names(vault.out()).is_empty(),
        "nothing was selected, so nothing may be written: {:?}",
        jpeg_names(vault.out())
    );

    // The scan says what the cap did, rather than reading as an empty cache.
    let output = scan(&vault)
        .args(["--json", "--max-long-edge", "1920"])
        .output()
        .expect("run scan --json");
    let object: Value = serde_json::from_slice(&output.stdout).expect("one JSON object");
    assert_eq!(object["above_cap"], 1, "{object}");
    assert_eq!(object["below_floor"], 0, "{object}");
    assert_eq!(object["failed"], 0, "a cap never fails an image: {object}");
}

#[test]
fn m18_all_levels_reports_the_levels_the_cap_left_behind() {
    let vault = capped();

    extract(&vault)
        .args(["--all-levels", "--max-long-edge", "1920"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("above cap (1920px)"));

    let mut written = jpeg_names(vault.out());
    written.sort();
    assert_eq!(
        written,
        vec!["IMG_0031 - 960x640.jpg", "IMG_0031.jpg"],
        "the cap's choice keeps the plain name; the smaller survivor is suffixed"
    );
}

#[test]
fn a_cap_below_the_floor_is_an_empty_window_not_a_usage_error() {
    let pyramid = uuid(33);
    let vault = Vault::modern()
        .image(ImageRow::new(33, "IMG_0033").cache(&pyramid))
        .pyramid(PyramidRow::new(&pyramid).level(960, 640))
        .level_file(Level::sized(&pyramid, 960, 640))
        .write();

    // The level clears the floor and not the cap, so the window holds nothing —
    // the same answer `--min-long-edge 99999` gets, and never exit 1.
    extract(&vault)
        .args(["--min-long-edge", "640", "--max-long-edge", "500"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("nothing to extract"));
    assert!(jpeg_names(vault.out()).is_empty());
}

#[test]
fn a_floor_above_every_level_exits_three_and_writes_nothing() {
    let vault = damaged();

    extract(&vault)
        .arg("--min-long-edge")
        .arg("99999")
        .assert()
        .code(3)
        .stderr(predicate::str::contains("nothing to extract"));

    assert!(
        jpeg_names(vault.out()).is_empty(),
        "nothing was selected, so nothing may be written: {:?}",
        jpeg_names(vault.out())
    );
}

// ---------------------------------------------------------------------------
// `unindexed` — a missing or unreadable index is a mode, not an error
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
fn u1_an_indexless_cache_exits_three_and_names_the_missing_file() {
    let vault = unindexed();

    let output = scan(&vault).arg("--json").output().expect("run scan");
    assert_eq!(output.status.code(), Some(3));
    let object: Value = serde_json::from_slice(&output.stdout).expect("one object on stdout");
    assert_eq!(object["layout"], "modern");
    assert_eq!(object["images"], 2);
    assert_eq!(object["no_cache_entry"], 2);
    assert_eq!(object["unmapped"], 2);
    assert_eq!(object["recovered"], 0);

    // ADR 0006 ¶1: one hint, naming the file that is missing, with the counts
    // the plan produced. Nothing about previews the cache does not have.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("previews.db is missing"),
        "the hint names the file:\n{stderr}"
    );
    assert!(
        stderr.contains("2 payloads cannot be matched to any of the 2 catalog images"),
        "with the counts from the plan:\n{stderr}"
    );
    assert!(
        stderr.contains("listed as unmapped and never written"),
        "and what became of them:\n{stderr}"
    );
    assert!(
        !stderr.contains("sqlite error"),
        "an absent index is not a database error:\n{stderr}"
    );
    assert!(
        !stderr.contains("no previews"),
        "nothing claims the cache is empty:\n{stderr}"
    );

    // And the human summary says the same thing in its own words.
    let text = scan(&vault)
        .output()
        .expect("run scan")
        .stdout
        .iter()
        .map(|byte| *byte as char)
        .collect::<String>();
    assert!(
        text.starts_with("cache layout: modern\n"),
        "the layout survives without rows:\n{text}"
    );
    assert!(text.contains("unmapped payloads"), "{text}");

    // The hint promises `scan --verbose` prints each path, so it does: one line
    // per orphan pyramid, naming a file the builder actually wrote.
    let verbose = scan(&vault)
        .arg("--verbose")
        .output()
        .expect("run scan --verbose")
        .stdout
        .iter()
        .map(|byte| *byte as char)
        .collect::<String>();
    let listed: Vec<&str> = verbose
        .lines()
        .filter(|line| line.starts_with("unmapped  -"))
        .collect();
    assert_eq!(listed.len(), 2, "one line per orphan pyramid:\n{verbose}");
    for line in listed {
        assert!(
            vault
                .level_files()
                .iter()
                .any(|file| line
                    .contains(file.file_name().expect("a leaf").to_string_lossy().as_ref())),
            "the line names a payload on disk: {line}"
        );
    }
}

#[test]
fn u2_an_indexless_extract_hands_over_the_orphan_list_and_writes_no_jpeg() {
    let vault = unindexed();

    extract(&vault)
        .assert()
        .code(3)
        .stderr(predicate::str::contains("nothing to extract"));

    // ADR 0006 ¶7: a run that selected nothing still writes its report when it
    // found orphan payloads — and writes nothing at all for them.
    let csv = fs::read_to_string(vault.out().join("report.csv")).expect("report.csv");
    let orphans = csv
        .lines()
        .filter(|row| row.starts_with("unmapped"))
        .count();
    assert_eq!(orphans, 2, "one row per orphan pyramid:\n{csv}");

    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(vault.out().join("manifest.json")).unwrap())
            .expect("one object in manifest.json");
    assert_eq!(manifest["layout"], "modern");
    let listed = manifest["items"]
        .as_array()
        .expect("an item list")
        .iter()
        .filter(|item| item["status"] == "unmapped")
        .count();
    assert_eq!(listed, 2, "one manifest member per orphan pyramid");

    assert!(
        jpeg_names(vault.out()).is_empty(),
        "an orphan is listed, never written: {:?}",
        jpeg_names(vault.out())
    );
    assert!(
        !vault.out().join("extract.csv").exists(),
        "nothing was evaluated, so there are no outcomes to list"
    );
}

#[test]
fn u3_a_previews_path_that_is_not_a_directory_is_a_usage_error() {
    let vault = unindexed();

    // The catalog is a real file, so this is a mistyped cache path — not an
    // empty cache (ADR 0006 ¶3). A `--report` is asked for and never written:
    // a run that exits 1 has no plan to describe, because the inputs failed
    // before one existed (`cli`).
    let report = vault.out().join("scan.json");
    let mut command = lrsalvage();
    command
        .arg("scan")
        .arg("--catalog")
        .arg(vault.catalog())
        .arg("--previews")
        .arg(vault.catalog())
        .arg("--report")
        .arg(&report);
    let output = command.output().expect("run scan");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("is not a preview cache directory"),
        "a real message, not a SQLite one:\n{stderr}"
    );
    assert!(!stderr.contains("sqlite error"), "{stderr}");
    assert!(
        !has_file_named(vault.dir(), "scan.json"),
        "a report is never written for a run with no plan"
    );

    // A path that exists nowhere is the same mistake, and reads the same way.
    let missing = vault.dir().join("no such cache");
    let mut command = lrsalvage();
    command
        .arg("scan")
        .arg("--catalog")
        .arg(vault.catalog())
        .arg("--previews")
        .arg(&missing);
    command
        .assert()
        .code(1)
        .stderr(predicate::str::contains("is not a preview cache directory"));

    // `extract` asks the same question before it touches `--out`, so a run that
    // cannot read its cache leaves no directory behind.
    let uncreated = vault.dir().join("not created");
    let mut command = lrsalvage();
    command
        .arg("extract")
        .arg("--catalog")
        .arg(vault.catalog())
        .arg("--previews")
        .arg(vault.catalog())
        .arg("--out")
        .arg(&uncreated);
    command
        .assert()
        .code(1)
        .stderr(predicate::str::contains("is not a preview cache directory"));
    assert!(
        !uncreated.exists(),
        "a refused cache path creates no output directory"
    );
}

#[test]
fn u4_a_damaged_index_degrades_exactly_like_a_missing_one() {
    let index = uuid(1);
    let vault = Vault::modern()
        .image(ImageRow::new(1, "IMG_0001"))
        .pyramid(PyramidRow::new(&index).level(1616, 1077))
        .level_file(Level::sized(&index, 1616, 1077))
        .raw_index(&bytes::not_a_jpeg())
        .write();

    let output = scan(&vault).arg("--json").output().expect("run scan");
    assert_eq!(output.status.code(), Some(3));
    let object: Value = serde_json::from_slice(&output.stdout).expect("one object on stdout");
    assert_eq!(object["unmapped"], 1, "the payload is still listed");
    assert_eq!(object["no_cache_entry"], 1);

    // The reason replaces `is missing`, and the second line says what to do
    // about it (ADR 0006 ¶2).
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not be read (sqlite error: "),
        "the hint carries the reason:\n{stderr}"
    );
    assert!(
        stderr.contains("fix or move the file to name them"),
        "and the instruction:\n{stderr}"
    );
    assert!(
        !stderr.contains("lrsalvage: sqlite error"),
        "a damaged index never aborts the run:\n{stderr}"
    );
}

#[test]
fn e2_an_empty_run_writes_no_report_at_all() {
    let vault = Vault::empty()
        .image(ImageRow::new(1, "IMG_0001"))
        .image(ImageRow::new(2, "IMG_0002"))
        .write();

    extract(&vault)
        .assert()
        .code(3)
        .stderr(predicate::str::contains("nothing to extract"));

    // Nothing selected *and* nothing unclaimed is not something to report on:
    // the contrast with U2, where the orphan list is worth writing.
    for name in ["report.csv", "manifest.json"] {
        assert!(
            !vault.out().join(name).exists(),
            "{name} must not be written by an empty run"
        );
    }
    assert!(jpeg_names(vault.out()).is_empty());
}

// ---------------------------------------------------------------------------
// The verification report, rows R1–R11
// ---------------------------------------------------------------------------

/// `modern with a suspect`, the cache R1, R2, R7, R8 and R9 run against: M1
/// recovered, M5 whose `_1616` leaf holds an 800x600 frame, M6's tombstone and
/// M10's orphan. One leaf disagrees with its own name, so `suffix_matches_sof` is
/// one lower than the level-file count by construction rather than by a copied
/// number.
///
/// It returns the uuids too: a negative row asserts the names the builder
/// produced, and M6's tombstoned pyramid leaves no file on disk to read one from.
fn modern_with_a_suspect() -> (Vault, Vec<String>) {
    let (m1, m5) = (uuid(1), uuid(4));
    let (m6, m10) = (uuid(6), uuid(20));
    let vault = Vault::modern()
        .image(ImageRow::new(1, "IMG_0001").cache(&m1))
        .pyramid(PyramidRow::new(&m1).level(1616, 1077))
        .level_file(Level::sized(&m1, 1616, 1077))
        .image(ImageRow::new(4, "IMG_0004").cache(&m5))
        .pyramid(PyramidRow::new(&m5).level(1616, 1077))
        .level_file(Level::sized(&m5, 800, 600).long_dimension(1616.0))
        .image(ImageRow::new(6, "IMG_0006").cache(&m6))
        .pyramid(PyramidRow::new(&m6).level(1616, 1077))
        .tombstone(&m6, 700_000_000.0)
        .unclaimed(&[Level::sized(&m10, 640, 427)])
        .write();
    (vault, vec![m1, m5, m6, m10])
}

/// `mixed`, built from X1, X2 and X3: both representations, one of them
/// superseded, so a report publishes both grammars.
fn mixed() -> Vault {
    let (superseded, container) = (uuid(1), uuid(2));
    let level_file = uuid(3);
    Vault::mixed()
        .image(ImageRow::new(1, "IMG_0001").cache(&superseded))
        .pyramid(PyramidRow::new(&superseded).level(1616, 1077))
        .superseded(&[Level::sized(&superseded, 1616, 1077)])
        .image(ImageRow::new(2, "IMG_0002").cache(&container))
        .pyramid(PyramidRow::new(&container).level(1616, 1077))
        .container(&[Level::sized(&container, 1616, 1077)])
        .image(ImageRow::new(3, "IMG_0003").cache(&level_file))
        .pyramid(PyramidRow::new(&level_file).level(1616, 1077))
        .level_file(Level::sized(&level_file, 1616, 1077))
        .write()
}

/// `empty`: an index that holds no rows, and two images it never claimed.
fn empty() -> Vault {
    Vault::empty()
        .image(ImageRow::new(1, "IMG_0001"))
        .image(ImageRow::new(2, "IMG_0002"))
        .write()
}

/// The verification report a run wrote, as the one object it promises.
fn verification(path: &Path) -> (String, Value) {
    let text =
        fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let object = serde_json::from_str(&text).unwrap_or_else(|error| {
        panic!(
            "{} is not one JSON object ({error}):\n{text}",
            path.display()
        )
    });
    (text, object)
}

/// One path, in both separator spellings: a leak may reach the bytes either way.
fn push_path(names: &mut Vec<String>, path: &Path) {
    let raw = path.to_string_lossy().into_owned();
    names.push(raw.replace('\\', "/"));
    names.push(raw);
}

/// Every string a verification report must never contain: the run's paths and
/// the uuids, digests, shard folders and leaf names **the builder produced**.
fn identifying(vault: &Vault, uuids: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    for path in [vault.dir(), vault.catalog(), vault.previews(), vault.out()] {
        push_path(&mut names, path);
    }
    for uuid in uuids {
        names.push(uuid.clone());
        names.push(lrsalvage_fixtures::vault::digest_of(uuid));
        // A shard folder is `uuid[0]` and then `uuid[0..4]`; the one-character
        // one is covered by the relative path below, which is where it would
        // appear. A single letter is not a string worth asserting absence of.
        names.push(uuid.chars().take(4).collect());
    }
    for file in vault.level_files().iter().chain(vault.containers()) {
        names.push(
            file.file_name()
                .expect("a payload has a leaf name")
                .to_string_lossy()
                .into_owned(),
        );
        let relative = file
            .strip_prefix(vault.previews())
            .expect("every payload is written inside the cache");
        push_path(&mut names, relative);
        for part in relative.components() {
            let text = part.as_os_str().to_string_lossy().into_owned();
            if text.len() > 1 {
                names.push(text);
            }
        }
    }
    names.retain(|name| !name.is_empty());
    names
}

/// R1. The published object's field set is a contract, not a snapshot of the
/// plan, and every `stats` member is the plan's own number.
#[test]
fn r1_the_verification_report_is_the_frozen_field_set() {
    let (vault, _) = modern_with_a_suspect();
    let path = vault.out().join("scan.json");

    scan(&vault)
        .arg("--report")
        .arg(&path)
        .assert()
        .code(0)
        .stderr(predicate::str::contains("wrote"));

    let (text, report) = verification(&path);

    // Exactly the frozen set: no `items`, no `catalog`, no `previews`.
    let mut keys: Vec<&String> = report.as_object().expect("an object").keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "crs_version",
            "db_version",
            "generated_unix",
            "layout",
            "options",
            "schema",
            "shapes",
            "snapshot_sha256",
            "source_sha256",
            "stats",
            "version",
        ],
        "the report's keys are frozen at eleven: no path-bearing member exists"
    );

    assert_eq!(report["schema"], "lrsalvage.verification");
    assert_eq!(report["version"], env!("CARGO_PKG_VERSION"));
    assert!(
        report["generated_unix"].as_u64().expect("an epoch second") > 1_600_000_000,
        "generated_unix must be a real timestamp:\n{text}"
    );
    assert_eq!(report["layout"], "modern");
    assert_eq!(report["db_version"], "1504001");
    assert_eq!(report["crs_version"], "18.0.0");

    // The options the run applied, echoed: both bounds and all three switches.
    assert_eq!(report["options"]["min_long_edge"], 640);
    assert_eq!(report["options"]["max_long_edge"], 0);
    assert_eq!(report["options"]["all_levels"], false);
    assert_eq!(report["options"]["include_present"], false);

    // A run that did not ask for the proof carries both digests absent, and the
    // echo says so: the key set does not move with a flag.
    assert_eq!(report["source_sha256"], Value::Null);
    assert_eq!(report["snapshot_sha256"], Value::Null);
    assert_eq!(report["options"]["verify_source"], false);

    // Every `stats` member is the plan's own number, under the frozen names.
    let plan = vault.plan(&lrsalvage_core::plan::Options::default());
    let stats = &report["stats"];
    let members = [
        ("images", plan.stats.images),
        ("recovered", plan.stats.recoverable),
        ("suspect", plan.stats.suspect),
        ("missing_payload", plan.stats.missing_payload),
        ("no_cache_entry", plan.stats.no_cache_entry),
        ("unmapped", plan.stats.unmapped),
        ("failed", plan.stats.failed),
        ("original_present", plan.stats.original_present),
        ("below_floor", plan.stats.below_floor),
        ("above_cap", plan.stats.above_cap),
        ("bytes", plan.stats.bytes),
        ("levels", plan.stats.levels),
        ("selected_items", plan.stats.selected_items),
        ("selected_images", plan.stats.selected_images),
        ("superseded", plan.stats.superseded),
        ("deleted", plan.stats.deleted),
        ("unrecognised", plan.stats.unrecognised),
    ];
    for (name, value) in members {
        assert_eq!(stats[name], value, "stats.{name} is the plan's number");
    }
    assert_eq!(
        stats["tiers"],
        serde_json::json!(plan.stats.tiers),
        "the tier histogram, with absent tiers absent"
    );
    let mut named: Vec<&String> = stats.as_object().expect("an object").keys().collect();
    named.sort();
    assert_eq!(named.len(), 18, "the `stats` list is frozen: {named:?}");

    // One object per class: the level files the builder wrote, one of which
    // names a long edge its own frame disagrees with, and no container at all.
    assert_eq!(
        report["shapes"]["level_file"]["count"],
        vault.level_files().len() as u64
    );
    assert_eq!(
        report["shapes"]["level_file"]["suffix_matches_sof"],
        vault.level_files().len() as u64 - 1,
        "M5's `_1616` leaf measures 800x600"
    );
    assert_eq!(
        report["shapes"]["level_file"]["template"],
        lrsalvage_core::previews_db::LEVEL_FILE_GRAMMAR
    );
    assert_eq!(report["shapes"]["container"]["count"], 0);
    assert!(
        report["shapes"]["container"].get("template").is_none(),
        "a template for a shape the cache never showed is an example nobody asked for"
    );
}

/// R2. The matrix's first negative row: one leaked string fails it. Compared
/// against names the builder produced, never a hard-coded sample.
#[test]
fn r2_the_written_bytes_name_no_cache_and_no_payload() {
    let (vault, uuids) = modern_with_a_suspect();
    let path = vault.out().join("scan.json");
    scan(&vault).arg("--report").arg(&path).assert().code(0);

    let (text, _) = verification(&path);
    assert!(
        text.len() > 200,
        "the row must not be able to pass by the file being empty:\n{text}"
    );
    for name in identifying(&vault, &uuids) {
        assert!(
            !text.contains(&name),
            "the report must not contain {name:?}:\n{text}"
        );
    }
}

/// R3. A mixed cache publishes both grammars, and `superseded` is a stat.
#[test]
fn r3_a_mixed_cache_publishes_both_grammars() {
    let vault = mixed();
    let path = vault.out().join("scan.json");

    scan(&vault).arg("--report").arg(&path).assert().code(0);

    let (_, report) = verification(&path);
    let level_file = &report["shapes"]["level_file"];
    let container = &report["shapes"]["container"];
    assert!(
        level_file["count"].as_u64().expect("a count") > 0,
        "a level-file class is present"
    );
    assert!(
        container["count"].as_u64().expect("a count") > 0,
        "a container class is present"
    );
    assert_eq!(
        level_file["template"],
        lrsalvage_core::previews_db::LEVEL_FILE_GRAMMAR,
        "each class carries its own grammar, not a cache-wide one"
    );
    assert_eq!(
        container["template"],
        lrsalvage_core::previews_db::CONTAINER_GRAMMAR
    );
    assert_eq!(report["stats"]["superseded"], 1, "X1 holds both");
}

/// R4. An exit-3 run still hands over its report, and the histogram has no
/// bucket for "no shape".
#[test]
fn r4_a_run_that_named_nothing_still_writes_its_report() {
    let vault = unrecognised();
    let path = vault.out().join("scan.json");

    let output = scan(&vault)
        .arg("--report")
        .arg(&path)
        .output()
        .expect("run scan --report");
    assert_eq!(output.status.code(), Some(3), "nothing was selected");
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("payload-like files in the cache match no layout"),
        "the one hint still prints:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let (_, report) = verification(&path);
    assert_eq!(report["layout"], "unknown");
    assert_eq!(report["stats"]["unrecognised"], 3);
    assert_eq!(report["shapes"]["level_file"]["count"], 0);
    assert_eq!(report["shapes"]["container"]["count"], 0);
    for class in ["level_file", "container"] {
        assert!(
            report["shapes"][class].get("template").is_none(),
            "{class} showed no file, so it publishes no template"
        );
    }
    assert!(
        jpeg_names(vault.out()).is_empty(),
        "the report is the only thing this run wrote"
    );
}

/// R5. The shape facts survive without an index: the count comes from the leaf
/// alone, with no row to consult.
#[test]
fn r5_an_indexless_run_still_publishes_the_cache_shape() {
    let vault = unindexed();
    let path = vault.out().join("scan.json");

    scan(&vault).arg("--report").arg(&path).assert().code(3);

    let (_, report) = verification(&path);
    assert_eq!(report["stats"]["unmapped"], 2);
    assert_eq!(report["stats"]["no_cache_entry"], 2);
    assert_eq!(report["layout"], "modern", "the layout is the tree's");
    assert_eq!(
        report["shapes"]["level_file"]["count"],
        vault.level_files().len() as u64,
        "every level file the builder wrote, derived from the leaf"
    );
}

/// R6. A requested artifact is written even when nothing was selected — the
/// contrast with E2, where `extract` writes no report at all.
#[test]
fn r6_an_empty_cache_still_writes_the_report_it_was_asked_for() {
    let vault = empty();
    let path = vault.out().join("scan.json");

    scan(&vault)
        .arg("--report")
        .arg(&path)
        .assert()
        .code(3)
        .stderr(predicate::str::contains("wrote"));

    let (_, report) = verification(&path);
    assert_eq!(report["stats"]["images"], 2);
    assert_eq!(report["stats"]["no_cache_entry"], 2);
    assert_eq!(report["stats"]["recovered"], 0);
    assert_eq!(report["layout"], "none");
    assert_eq!(report["shapes"]["level_file"]["count"], 0);
    assert_eq!(report["shapes"]["level_file"]["suffix_matches_sof"], 0);
    assert_eq!(report["shapes"]["container"]["count"], 0);
}

/// R7. A claimed name, never an overwrite.
#[test]
fn r7_an_existing_report_name_is_numbered_and_left_alone() {
    let (vault, _) = modern_with_a_suspect();
    let path = vault.out().join("scan.json");
    fs::write(&path, b"somebody else's file").expect("preoccupy the name");

    let output = scan(&vault)
        .arg("--report")
        .arg(&path)
        .output()
        .expect("run scan --report");
    assert_eq!(output.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("already exists; writing"),
        "the run says so:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let numbered = vault.out().join("scan (2).json");
    assert!(numbered.exists(), "the next free number was claimed");
    verification(&numbered);
    assert_eq!(
        fs::read(&path).expect("the old file"),
        b"somebody else's file",
        "a name on disk may belong to somebody else"
    );
}

/// R10. `--verify-source` publishes the proof: the catalog's digest and the
/// copy's, equal, the catalog's own number and the flag echoed beside them.
#[test]
fn r10_a_verified_run_publishes_the_same_digest_for_the_catalog_and_its_copy() {
    let (vault, _) = modern_with_a_suspect();
    let path = vault.out().join("scan.json");

    scan(&vault)
        .arg("--verify-source")
        .arg("--report")
        .arg(&path)
        .assert()
        .code(0);

    let (text, report) = verification(&path);
    let source = report["source_sha256"]
        .as_str()
        .unwrap_or_else(|| panic!("a digest, never null:\n{text}"));
    let snapshot = report["snapshot_sha256"]
        .as_str()
        .unwrap_or_else(|| panic!("a digest, never null:\n{text}"));

    assert_eq!(source, snapshot, "the copy is the catalog:\n{text}");
    assert_eq!(
        source,
        lrsalvage_core::snapshot::sha256_file(vault.catalog()).expect("the catalog's digest"),
        "the member is the catalog file's own sha256, never a label"
    );
    assert_eq!(report["options"]["verify_source"], true);
}

/// R11. On `extract` the flag performs the same check and publishes nothing:
/// the per-run reports are the files they were, and a passing run is silent.
#[test]
fn r11_a_verified_extract_checks_the_copy_and_leaves_its_reports_unchanged() {
    let vault = modern();

    let output = extract(&vault)
        .arg("--verify-source")
        .output()
        .expect("run extract --verify-source");
    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("sha256"),
        "a passing run says nothing about digests:\n{stderr}"
    );

    assert_eq!(
        jpeg_names(vault.out()),
        vec!["IMG_0001.jpg", "IMG_0013.jpg"],
        "the check does not change what is written"
    );
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(vault.out().join("manifest.json")).unwrap())
            .unwrap();
    for member in ["source_sha256", "snapshot_sha256", "verify_source"] {
        assert!(
            manifest.get(member).is_none(),
            "manifest.json is untouched by the flag: it carries {member}"
        );
    }
    let csv = fs::read_to_string(vault.out().join("report.csv")).expect("report.csv");
    assert!(
        !csv.contains("sha256"),
        "report.csv is untouched by the flag:\n{csv}"
    );
}

/// `true` when any file below `root` is called `name`.
fn has_file_named(root: &Path, name: &str) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().is_some_and(|leaf| leaf == name) {
            return true;
        }
        if path.is_dir() && has_file_named(&path, name) {
            return true;
        }
    }
    false
}

/// R8. Every named write obeys the destination rule, and a refused report is a
/// destination error rather than "nothing selected".
#[test]
fn r8_a_report_whose_parent_is_inside_the_cache_is_refused() {
    let (vault, _) = modern_with_a_suspect();
    let path = vault.previews().join("scan.json");

    let output = scan(&vault)
        .arg("--report")
        .arg(&path)
        .output()
        .expect("run scan --report");
    assert_eq!(output.status.code(), Some(4), "not 3, and not 0");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("refusing to write inside"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !has_file_named(vault.dir(), "scan.json"),
        "no report anywhere, the cache included"
    );
}

/// R9. The two surfaces do not mix: a file for a stranger, one object for a
/// script.
#[test]
fn r9_json_and_report_compose_without_a_collision() {
    let (vault, uuids) = modern_with_a_suspect();
    let path = vault.out().join("scan.json");

    let output = scan(&vault)
        .arg("--json")
        .arg("--verbose")
        .arg("--report")
        .arg(&path)
        .output()
        .expect("run scan --json --verbose --report");
    assert_eq!(output.status.code(), Some(0));

    // Exactly one object: `from_slice` rejects anything appended to it.
    let object: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not exactly one object ({error}):\n{}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    assert_eq!(
        object["layout"], "modern",
        "the `--json` field set survives"
    );
    assert_eq!(
        object["catalog"],
        vault.catalog().to_string_lossy().replace('\\', "/")
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("recovered"),
        "the verbose lines moved to stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let (text, report) = verification(&path);
    assert_eq!(report["stats"]["recovered"], object["recovered"]);
    for name in identifying(&vault, &uuids) {
        assert!(
            !text.contains(&name),
            "the file stays path-free with all three flags: {name:?}"
        );
    }
}
