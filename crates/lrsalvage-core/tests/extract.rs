//! `extract`'s resume verification, driven from a synthetic vault.
//!
//! The payload and the pre-existing destination come from `lrsalvage-fixtures`;
//! `specs/fixtures.md` says why a test that needs it lives here, not in
//! `src/extract.rs`.

use std::fs;

use lrsalvage_core::extract::{self, ExtractOptions, OutcomeStatus};
use lrsalvage_core::plan::Options;
use lrsalvage_fixtures::bytes;
use lrsalvage_fixtures::vault::{ImageRow, Level, PyramidRow, Vault};

const UUID: &str = "AAAA0001-1111-4111-8111-000000000001";

/// The output path a vault's one image derives: its root folder's last segment,
/// then the image's own name.
const RELATIVE_OUTPUT: &str = "originals/IMG_0001.jpg";

fn vault() -> Vault {
    let ours = bytes::our_output(1600, 1200, "0.1.0");
    Vault::modern()
        .image(ImageRow::new(1, "IMG_0001").cache(UUID))
        .pyramid(PyramidRow::new(UUID).level(1600, 1200))
        .level_file(Level::sized(UUID, 1600, 1200))
        .preoccupied_destination(RELATIVE_OUTPUT, &ours)
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
    let already_there = vault.out().join(RELATIVE_OUTPUT);
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
