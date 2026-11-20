//! The vaults the app's browser run is about, and the copy a page fetches.
//!
//! Native only: a `Vault` is a directory tree and `lrsalvage-fixtures` is not a
//! `wasm32` dependency. The page reads these bytes — served over HTTP and put
//! into origin-private storage — so the composition is built once, here, and
//! both targets see the same files.
//!
//! Each vault is a *condition* rather than a composition for its own sake: #117's
//! user stories 4 and 5 are about telling the ways a scan can come back empty
//! apart, and a page that says "nothing found" for all of them has failed them.

use std::fs;
use std::path::Path;

use lrsalvage_fixtures::vault::{ImageRow, Level, PyramidRow, Vault};
use serde_json::Value;

/// Every file in `from`, copied to `to` under the same relative path.
///
/// The page fetches the vault over HTTP from this copy, so what the browser reads
/// and what the native run read are the same bytes rather than two buildings of
/// the same composition.
pub fn copy_tree(from: &Path, to: &Path) -> Vec<Value> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(from) {
        let entry = entry.expect("walk the vault");
        let relative = entry
            .path()
            .strip_prefix(from)
            .expect("a path inside the vault");
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = to.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).expect("create a directory");
            continue;
        }
        fs::create_dir_all(target.parent().expect("a parent")).expect("create a directory");
        fs::copy(entry.path(), &target).expect("copy a vault file");
        files.push(serde_json::json!(
            relative.to_string_lossy().replace('\\', "/")
        ));
    }
    files.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    files
}

/// How the page's report for one vault is held to the native one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compare {
    /// Everything both targets can agree on: the counts, the cache facts, the
    /// index's condition, the recorded roots, and the two digests.
    Full,
    /// [`Compare::Full`] *after* the recorded originals folder is mounted. Before
    /// that the two targets must disagree, and the disagreement is the point: the
    /// desktop can see the drive and a page has to be handed it (`#149`, DEC-S7).
    AfterMount,
    /// Both refuse, and each says which stage refused.
    Failure,
}

impl Compare {
    /// The word the page reads.
    pub fn as_str(self) -> &'static str {
        match self {
            Compare::Full => "full",
            Compare::AfterMount => "after-mount",
            Compare::Failure => "failure",
        }
    }
}

/// One case for the page: its name, how its report is compared, and the vault.
pub type Case = (&'static str, Compare, fn() -> Vault);

/// The seven vaults, in the order the page runs them.
pub fn vaults() -> Vec<Case> {
    vec![
        ("working", Compare::Full, working),
        ("originals", Compare::AfterMount, originals),
        ("no-index", Compare::Full, no_index),
        ("unreadable-index", Compare::Full, unreadable_index),
        ("no-payloads", Compare::Full, no_payloads),
        ("unknown-layout", Compare::Full, unknown_layout),
        ("not-a-catalog", Compare::Failure, not_a_catalog),
    ]
}

const ONE: &str = "AAAA0001-1111-4111-8111-000000000001";
const TWO: &str = "AAAA0002-1111-4111-8111-000000000002";
const THREE: &str = "AAAA0003-1111-4111-8111-000000000003";

/// Three images with a preview each, and neither original on disk.
pub fn working() -> Vault {
    Vault::modern()
        .image(
            ImageRow::new(1, "IMG_0001")
                .path_from_root("2023/")
                .cache(ONE),
        )
        .image(
            ImageRow::new(2, "IMG_0002")
                .path_from_root("2023/")
                .cache(TWO),
        )
        .image(
            ImageRow::new(3, "IMG_0003")
                .path_from_root("2024/")
                .cache(THREE),
        )
        .pyramid(PyramidRow::new(ONE).level(1616, 1077))
        .pyramid(PyramidRow::new(TWO).level(1440, 960))
        .pyramid(PyramidRow::new(THREE).level(1616, 1077))
        .level_file(Level::sized(ONE, 1616, 1077))
        .level_file(Level::sized(TWO, 1440, 960))
        .level_file(Level::sized(THREE, 1616, 1077))
}

/// One image whose original is still where the catalog says, and one whose is
/// not: the recheck mounts the folder and the count moves by exactly one.
pub fn originals() -> Vault {
    let vault = Vault::modern()
        .image(
            ImageRow::new(1, "IMG_0001")
                .path_from_root("2023/")
                .original_on_disk()
                .cache(ONE),
        )
        .image(
            ImageRow::new(2, "IMG_0002")
                .path_from_root("2023/")
                .cache(TWO),
        );
    vault
        .pyramid(PyramidRow::new(ONE).level(1616, 1077))
        .pyramid(PyramidRow::new(TWO).level(1616, 1077))
        .level_file(Level::sized(ONE, 1616, 1077))
        .level_file(Level::sized(TWO, 1616, 1077))
}

/// Payloads and no `previews.db` at all: every image is `no-cache-entry` and
/// every payload is `unmapped`.
pub fn no_index() -> Vault {
    Vault::unindexed()
        .image(ImageRow::new(1, "IMG_0001").path_from_root("2023/"))
        .pyramid(PyramidRow::new(ONE).level(1616, 1077))
        .level_file(Level::sized(ONE, 1616, 1077))
}

/// A `previews.db` that is there and that SQLite refuses — the same empty answer
/// with a different reason, which is what the page has to say.
pub fn unreadable_index() -> Vault {
    Vault::modern()
        .image(
            ImageRow::new(1, "IMG_0001")
                .path_from_root("2023/")
                .cache(ONE),
        )
        .pyramid(PyramidRow::new(ONE).level(1616, 1077))
        .level_file(Level::sized(ONE, 1616, 1077))
        .raw_index(b"this is not a database")
}

/// An index and one row whose payload file is gone; no payload file at all, so
/// the layout is `none` and the image is `missing-payload`.
pub fn no_payloads() -> Vault {
    Vault::empty()
        .image(
            ImageRow::new(1, "IMG_0001")
                .path_from_root("2023/")
                .cache(ONE),
        )
        .pyramid(PyramidRow::new(ONE).level(1616, 1077))
}

/// Payload-like files that match no shape this reader knows: the layout is
/// `unknown`, which is not the same as an empty cache.
pub fn unknown_layout() -> Vault {
    Vault::unrecognised()
        .image(ImageRow::new(1, "IMG_0001").path_from_root("2023/"))
        .unrecognised_file(lrsalvage_fixtures::vault::UnrecognisedFile::no_suffix())
}

/// A file a person picked believing it was a catalog. The copy is carried and
/// the open is refused, which is the one failure the page can name exactly.
pub fn not_a_catalog() -> Vault {
    Vault::modern()
        .image(ImageRow::new(1, "IMG_0001").path_from_root("2023/"))
        .raw_catalog(b"this is not a Lightroom catalog")
}
