//! `extract`'s resume verification, driven from a synthetic vault.
//!
//! The payload and the pre-existing destination come from `lrsalvage-fixtures`;
//! `specs/fixtures.md` says why a test that needs it lives here, not in
//! `src/extract.rs`.

use std::fs;
use std::path::{Path, PathBuf};

use lrsalvage_core::extract::{self, ExtractOptions, OutcomeStatus};
use lrsalvage_core::plan::Options;
use lrsalvage_fixtures::bytes;
use lrsalvage_fixtures::vault::{ImageRow, Level, PyramidRow, Vault};

const UUID: &str = "AAAA0001-1111-4111-8111-000000000001";

/// The output path a vault's one image derives: its root folder's last segment,
/// then the image's own name. Built the way the fixture convention asks — with
/// `Path::join`, so no row depends on a separator spelling.
fn relative_output() -> PathBuf {
    Path::new("originals").join("IMG_0001.jpg")
}

fn vault() -> Vault {
    let ours = bytes::our_output(1600, 1200, "0.1.0");
    Vault::modern()
        .image(ImageRow::new(1, "IMG_0001").cache(UUID))
        .pyramid(PyramidRow::new(UUID).level(1600, 1200))
        .level_file(Level::sized(UUID, 1600, 1200))
        .preoccupied_destination(relative_output(), &ours)
        .write()
}

fn options(vault: &Vault) -> ExtractOptions {
    ExtractOptions {
        output_root: vault.out().to_path_buf(),
        dry_run: false,
        write_metadata: true,
        set_timestamps: false,
        write_orientation: true,
        force: true,
    }
}

#[test]
fn a_preexisting_verified_file_is_skipped_and_its_bytes_untouched() {
    let vault = vault();
    let already_there = vault.out().join(relative_output());
    let before = fs::read(&already_there).expect("the pre-existing destination");

    let plan = vault.plan(&Options::default());
    let mut progress = |_: &extract::Outcome| {};
    let outcomes = extract::extract(&plan, &options(&vault), &mut progress).expect("extract");

    // Same dimensions, different bytes: verification is about the file being one
    // of ours — the marker, completeness and the frame — not about matching a
    // payload byte for byte. The fixture writes the marker in the other byte
    // order, which is exactly the case a reader must accept.
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].status, OutcomeStatus::Skipped);
    assert!(outcomes[0].sha256.is_none(), "a skip has no digest");
    assert_eq!(
        fs::read(&already_there).expect("the destination after the run"),
        before,
        "a verified destination is never rewritten"
    );
}

/// The names under one directory, so a refusal can be shown to have changed
/// nothing.
fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("a readable directory")
        .map(|entry| {
            entry
                .expect("a directory entry")
                .path()
                .strip_prefix(dir)
                .unwrap_or_else(|_| Path::new(""))
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// #64: the pre-flight's own decision, against the filesystem's real answer.
///
/// `u64::MAX` is a total no destination can hold, so the refusal is exercised
/// without filling a disk. The exact boundary — one byte short refuses, the
/// plan's own total proceeds — is `lrsalvage-cli`'s test, where a debug build
/// answers that total on the nose (`specs/modules/fs.md`).
#[test]
fn ensure_room_refuses_a_total_no_destination_can_hold() {
    let vault = vault();
    let untouched = entries(vault.out());

    let error = extract::ensure_room(vault.out(), u64::MAX).expect_err("no disk holds u64::MAX");
    assert!(
        matches!(&error, lrsalvage_core::Error::Destination { .. }),
        "the refusal is the crate's destination variant — the one `main.rs`'s \
         exit-code test pins to `4`, so the code is held by the pair: {error:?}"
    );
    let message = error.to_string();
    assert!(
        message.contains(&format!(
            "{} bytes",
            lrsalvage_core::report::with_thousands(u64::MAX)
        )),
        "the need is stated in bytes, with the separators the summary uses: {message}"
    );
    assert!(
        message.contains("MiB"),
        "and in the human unit the reports use: {message}"
    );
    assert!(message.contains("short"), "{message}");
    assert!(message.contains("Nothing was written"), "{message}");
    assert_eq!(
        entries(vault.out()),
        untouched,
        "a refused run leaves the destination exactly as it found it"
    );
}

/// The question is asked of the nearest existing ancestor, since a dry run
/// creates nothing, and the number compared is the plan's own total.
#[test]
fn ensure_room_measures_the_nearest_existing_ancestor() {
    let vault = vault();
    let plan = vault.plan(&Options::default());
    let pending = vault.out().join("not").join("created");

    let extract::Preflight::Fits(room) = extract::ensure_room(&pending, plan.stats.bytes)
        .expect("a destination under a real folder")
    else {
        panic!("this machine cannot measure free space, which is its own surprise");
    };
    assert_eq!(
        room.needed, plan.stats.bytes,
        "the plan's total is the number compared, not a second measurement"
    );
    assert!(
        room.available > 0,
        "a filesystem this test writes to reports room, got {}",
        room.available
    );
    assert!(!pending.exists(), "asking the question creates nothing");
}

/// The pre-flight's number is what the run has *left* to write.
///
/// `vault()` places a verified output at the item's own path, so this is the
/// resume case: the destination already holds the bytes and needs no room for
/// them — unless `--force` says it may replace them (#64, review round).
#[test]
fn pending_bytes_counts_only_what_the_destination_does_not_already_hold() {
    let vault = vault();
    let plan = vault.plan(&Options::default());
    let settled = ExtractOptions {
        force: false,
        ..options(&vault)
    };

    assert_eq!(
        plan.items.iter().map(|item| item.bytes).sum::<u64>(),
        plan.stats.bytes,
        "the plan publishes per item exactly the total it sums"
    );
    assert_eq!(
        extract::pending_bytes(&plan, &settled),
        0,
        "an output already on disk is skipped or failed, never rewritten"
    );
    assert_eq!(
        extract::pending_bytes(&plan, &options(&vault)),
        plan.stats.bytes,
        "`--force` may replace what is there, so none of it is discounted"
    );
}

/// A destination that holds nothing needs room for every byte the plan selected.
#[test]
fn pending_bytes_is_the_plans_total_on_a_fresh_destination() {
    let fresh = Vault::modern()
        .image(ImageRow::new(1, "IMG_0001").cache(UUID))
        .pyramid(PyramidRow::new(UUID).level(1600, 1200))
        .level_file(Level::sized(UUID, 1600, 1200))
        .write();
    let plan = fresh.plan(&Options::default());
    let settled = ExtractOptions {
        force: false,
        ..options(&fresh)
    };

    assert_eq!(
        extract::pending_bytes(&plan, &settled),
        plan.stats.bytes,
        "nothing is there, so the run writes the plan's whole total"
    );
    assert!(plan.stats.bytes > 0, "the fixture writes something");
}
