//! The fixture matrix, rows whose medium is a run.
//!
//! Every test drives the real binary through the command line and asserts what
//! only a run can say: the exit code a row names, and the bytes of the tree it
//! left behind. The catalog, the cache and every payload come from
//! `lrsalvage-fixtures`; no fixture lives in this file.
//!
//! **No total and no file count is asserted.** A run's `images`/`recovered`
//! counts are the vault's pinned totals, which this slice cannot produce: the
//! `modern` vault's include `unmapped 2`, which needs the orphan sweep, and
//! `mixed`'s include `superseded 1`, a stat that does not exist yet. Each vault
//! here is built from the rows these runs need (M1 for `modern`; D1 and D4 for
//! `damaged`) rather than whole, so that a count asserted by accident would read
//! as a fixture bug rather than a promise. The rows' full shapes and their
//! structured fields are asserted in `lrsalvage-core`'s matrix.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use lrsalvage_core::jpeg;
use lrsalvage_fixtures::bytes;
use lrsalvage_fixtures::vault::{ImageRow, Level, PyramidRow, Vault};
use predicates::prelude::*;

/// One distinct uuid per row, so no two pyramids share an address.
fn uuid(row: u32) -> String {
    format!("AAAA{row:04}-1111-4111-8111-{row:012}")
}

fn lrsalvage() -> Command {
    Command::cargo_bin("lrsalvage").expect("the integration-test binary path")
}

/// `modern`, with the one row an exit-0 run needs (M1).
fn modern() -> Vault {
    let m1 = uuid(1);
    Vault::modern()
        .image(ImageRow::new(1, "IMG_0001").cache(&m1))
        .pyramid(PyramidRow::new(&m1).level(1616, 1077))
        .level_file(Level::sized(&m1, 1616, 1077))
        .write()
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
