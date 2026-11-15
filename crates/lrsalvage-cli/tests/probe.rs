//! The probe's numbers are the reader's numbers.
//!
//! `scripts/probe-cache.sh` measures things the shipped reader also measures —
//! how many level files a cache holds, and on how many of them the leaf's
//! `_<longedge>` agrees with the frame's own long edge. It is a second
//! implementation of a rule `jpeg::certify` already owns, and a second
//! implementation with no test is where the first bug came from: the first
//! version compared the suffix with the frame's **width**, so every portrait
//! level read as a disagreement while the reader said otherwise, and only a real
//! cache with portrait levels showed it.
//!
//! So this row runs the probe over a synthetic vault and holds it to `scan
//! --json`, on a vault that carries a portrait level precisely because that is
//! the shape the two implementations disagreed about. A disagreement here is the
//! finding: one of the two is lying about a cache a stranger will quote.
//!
//! **Unix only.** The probe walks bytes with `od` and `awk` and reads the index
//! with the `sqlite3` CLI; the two are not guaranteed on a Windows runner, and
//! the repository's rule is that a missing tool fails rather than skips. The
//! Windows job still builds and tests everything else, and the probe itself is
//! measured by hand where it is used.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use assert_cmd::Command as Run;
use lrsalvage_fixtures::vault::{ImageRow, Level, PyramidRow, Vault};
use serde_json::Value;

fn uuid(row: u32) -> String {
    format!("AAAA{row:04}-1111-4111-8111-{row:012}")
}

/// A modern vault whose pyramids include a portrait level: 1077 × 1616, whose
/// long edge is its height. A probe that compares the leaf's suffix with the
/// frame's width reports one fewer agreement than the reader on this vault.
fn landscape_and_portrait() -> Vault {
    let landscape = uuid(1);
    let portrait = uuid(2);
    Vault::modern()
        .image(ImageRow::new(1, "IMG_0001").cache(&landscape))
        .pyramid(PyramidRow::new(&landscape).level(1616, 1077).level(101, 68))
        .level_file(Level::sized(&landscape, 1616, 1077))
        .level_file(Level::sized(&landscape, 101, 68))
        .image(ImageRow::new(2, "IMG_0002").cache(&portrait))
        .pyramid(PyramidRow::new(&portrait).level(1077, 1616))
        .level_file(Level::sized(&portrait, 1077, 1616))
        .write()
}

/// Run the probe and read its flat `key   value` lines. Section blocks (`chains`,
/// `series`) are skipped: they are read by a person, not by this row.
fn probe(vault: &Vault) -> BTreeMap<String, String> {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("scripts")
        .join("probe-cache.sh");
    assert!(script.is_file(), "the probe is at {}", script.display());

    // No tool-presence guard: the probe names the missing tool itself
    // (`missing od: coreutils`), and the assertion below prints its stderr. A
    // guard that ran `--version` on each tool was worse than nothing — macOS's
    // BSD `od` has no `--version`, so it reported a tool that is present as
    // missing and failed this row on that platform alone.
    let output = Command::new("sh")
        .arg(&script)
        .arg(vault.previews())
        .output()
        .expect("run the probe");
    assert!(
        output.status.success(),
        "probe exited {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("the probe prints UTF-8");
    let mut values = BTreeMap::new();
    for line in stdout.lines() {
        let Some(split) = line.find(char::is_whitespace) else {
            continue;
        };
        let (key, rest) = line.split_at(split);
        values.insert(key.to_string(), rest.trim().to_string());
    }
    values
}

fn scan_json(vault: &Vault) -> Value {
    let output = Run::cargo_bin("lrsalvage")
        .expect("the integration-test binary path")
        .arg("scan")
        .arg("--catalog")
        .arg(vault.catalog())
        .arg("--previews")
        .arg(vault.previews())
        .arg("--include-present")
        .arg("--json")
        .output()
        .expect("run scan --json");
    assert!(
        output.status.success(),
        "scan exited {:?}",
        output.status.code()
    );
    serde_json::from_slice(&output.stdout).expect("scan --json prints one object")
}

#[test]
fn the_probe_agrees_with_the_reader_on_every_level_file() {
    let vault = landscape_and_portrait();
    let measured = probe(&vault);
    let reported = scan_json(&vault);
    let shapes = &reported["shapes"];

    let count = |map: &BTreeMap<String, String>, key: &str| -> u64 {
        map.get(key)
            .unwrap_or_else(|| panic!("the probe printed no `{key}`:\n{map:#?}"))
            .parse()
            .unwrap_or_else(|_| panic!("`{key}` is not a number"))
    };

    let files = count(&measured, "level_files");
    assert_eq!(files, 3, "the vault holds three level files");
    assert_eq!(files, shapes["level_file"].as_u64().unwrap());
    assert_eq!(
        count(&measured, "suffix_matches_sof"),
        shapes["suffix_matches_sof"].as_u64().unwrap(),
        "the probe and the reader must agree on every level, portrait levels included"
    );

    // A file that cannot be walked is not measured: these three count only the
    // files the probe actually read to `SOS` between `SOI` and `FF D9`.
    assert_eq!(count(&measured, "soi_at_offset_0"), files);
    assert_eq!(count(&measured, "eoi_at_eof"), files);
    assert_eq!(count(&measured, "walked_to_sos"), files);

    // This vault is complete: every row has its file and every file has its row.
    assert_eq!(count(&measured, "rows_without_file"), 0);
    assert_eq!(count(&measured, "files_without_row"), 0);
    assert_eq!(count(&measured, "file_size_mismatch"), 0);
}
