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
    // empty cache (ADR 0006 ¶3).
    let mut command = lrsalvage();
    command
        .arg("scan")
        .arg("--catalog")
        .arg(vault.catalog())
        .arg("--previews")
        .arg(vault.catalog());
    let output = command.output().expect("run scan");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not a directory"),
        "a real message, not a SQLite one:\n{stderr}"
    );
    assert!(!stderr.contains("sqlite error"), "{stderr}");

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
        .stderr(predicate::str::contains("not a directory"));
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
