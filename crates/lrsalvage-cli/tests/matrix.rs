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
//! `modern`; D1 and D4 for `damaged`) rather than whole. A count asserted by
//! accident would read as a fixture bug rather than a promise. The rows' full
//! shapes and their structured fields are asserted in `lrsalvage-core`'s matrix.
//!
//! What a run does assert is the cache's shape: the layout word, the two version
//! strings, the three cache counts and the `shapes` triple, in `--json` and in
//! `manifest.json`, plus the trailing `payload` column of a CSV row.
//!
//! **No exit-code row was added for the widening or the orphan sweep, because no
//! run's code moved.** An unclaimed payload is listed and never selected, so it
//! can neither make a run partial nor turn "nothing to extract" into work; a
//! row-less level file is simply the level the image is written from. The codes
//! asserted below are the same before and after, which is the claim — not an
//! omission to be filled in later.

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

fn lrsalvage() -> Command {
    Command::cargo_bin("lrsalvage").expect("the integration-test binary path")
}

/// `modern`, with the rows these runs need.
///
/// | id | row |
/// |----|-----|
/// | M1 | recovered, one level file: the one item an exit-0 run writes |
/// | M6 | a tombstone over a pyramid with no file: the `deleted` count |
/// | M10 | one unclaimed pyramid: the orphan the reports must name |
fn modern() -> Vault {
    let (m1, m6) = (uuid(1), uuid(6));
    let m10 = uuid(20);
    Vault::modern()
        .image(ImageRow::new(1, "IMG_0001").cache(&m1))
        .pyramid(PyramidRow::new(&m1).level(1616, 1077))
        .level_file(Level::sized(&m1, 1616, 1077))
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

    // A version no release produced is reported, never used as a gate.
    let object = scan_json(&vault);
    assert_eq!(object["layout"], "unknown");
    assert_eq!(object["db_version"], "9900001");
    assert_eq!(object["crs_version"], "99.0.0");
    assert_eq!(object["unrecognised"], 3);
    assert_eq!(object["unmapped"], 0, "unrecognised is not an orphan");
    assert_eq!(object["shapes"]["level_file"], 0);
    assert_eq!(object["shapes"]["container"], 0);
}

#[test]
fn extract_names_each_payload_in_the_csv_and_the_manifest() {
    let vault = modern();

    extract(&vault).assert().code(0);

    // `report.csv` ends with `payload`, after `note`, and an unclaimed payload
    // carries its own path where an output would be.
    let csv = fs::read_to_string(vault.out().join("report.csv")).expect("report.csv");
    let mut rows = csv.lines();
    let header = rows.next().expect("a header");
    assert!(
        header.ends_with(",note,payload"),
        "the trailing column is the payload:\n{header}"
    );
    let orphan = rows
        .find(|row| row.starts_with("unmapped"))
        .expect("the unclaimed pyramid");
    let fields: Vec<&str> = orphan.split(',').collect();
    assert_eq!(fields.len(), 15, "{orphan}");
    assert_eq!(fields[9], "", "nothing is written for an orphan");
    assert_eq!(fields[10], "", "and it has no original path");
    assert!(
        fields[14].contains(&uuid(20)) && fields[14].ends_with("_640"),
        "the payload column names the largest level it holds: {}",
        fields[14]
    );

    // The manifest carries the cache's shape too, and one `payload` member per
    // item — the orphan's is the only one with no status image behind it.
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(vault.out().join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["layout"], "modern");
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
}

#[test]
fn extract_on_a_modern_cache_exits_zero_and_writes_the_level_file() {
    let vault = modern();

    extract(&vault).assert().code(0);

    let output = jpeg_named(vault.out(), "IMG_0001.jpg");
    let written = fs::read(&output).expect("read the recovered file");
    let certified = jpeg::certify(&written).expect("a complete JPEG");
    assert!(certified.complete, "{} must end with EOI", output.display());
    assert_eq!(certified.dimensions, (1616, 1077));
    assert_eq!(
        jpeg_names(vault.out()),
        vec!["IMG_0001.jpg"],
        "one item, one file, and no `.partial` left behind"
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
