//! The vault the parity run is about, and the copy the page fetches.
//!
//! Native only: a `Vault` is a directory tree and `lrsalvage-fixtures` is not a
//! `wasm32` dependency. The browser reads this vault's *bytes* — served over HTTP
//! and put into origin-private storage — so the composition is built once, here,
//! and both targets see the same files.

use lrsalvage_fixtures::vault::{ImageRow, Level, PyramidRow, Vault};
// The tree copy lives with the app crate, which needs the same one for its own
// browser run (`#149`): two walkers would be two answers to "which files does the
// page see".
pub use lrsalvage_app::vault::copy_tree;

/// One vault's rows: every status a plan can reach, and a payload for each that
/// has one.
///
/// Six images, and the split `with_wal` makes is the point of the grouping: the
/// four with a real payload come first only when the vault has no `-wal`, so the
/// WAL-carrying vault loses a *recovered* JPEG if the sidecar is ignored, not
/// just a count.
pub fn composition(wal: bool) -> Vault {
    let (one, two, copy) = (uuid(1), uuid(2), uuid(3));
    let (four, gone, small) = (uuid(4), uuid(5), uuid(6));

    let vault = Vault::modern()
        .root_folder("/photos")
        // 1: one level, recovered.
        .image(
            ImageRow::new(1, "IMG_0001")
                .path_from_root("2023/")
                .cache(&one),
        )
        .pyramid(PyramidRow::new(&one).level(1616, 1077))
        .level_file(Level::sized(&one, 1616, 1077))
        // 2: four levels, the largest wins, portrait.
        .image(
            ImageRow::new(2, "IMG_0002")
                .path_from_root("2023/")
                .cache(&two),
        )
        .pyramid(
            PyramidRow::new(&two)
                .level(202, 135)
                .level(404, 269)
                .level(808, 539)
                .level(1616, 2155),
        )
        .level_file(Level::sized(&two, 202, 135))
        .level_file(Level::sized(&two, 404, 269))
        .level_file(Level::sized(&two, 808, 539))
        .level_file(Level::sized(&two, 1616, 2155))
        // 3: a virtual copy over image 2's file row — two images, one original.
        .image(
            ImageRow::new(3, "IMG_0002")
                .path_from_root("2023/")
                .file(2)
                .copy("Copy 1")
                .cache(&copy),
        )
        .pyramid(PyramidRow::new(&copy).level(1616, 2155))
        .level_file(Level::sized(&copy, 1616, 2155))
        // 4: recovered, and one level. With `-wal` this row is in the sidecar, so
        // a run that ignores it loses a JPEG rather than a number.
        .image(
            ImageRow::new(4, "IMG_0004")
                .path_from_root("2023/")
                .cache(&four),
        )
        .pyramid(PyramidRow::new(&four).level(1616, 1077))
        .level_file(Level::sized(&four, 1616, 1077))
        // 5: an index row and no file anywhere.
        .image(
            ImageRow::new(5, "IMG_0005")
                .path_from_root("2023/")
                .cache(&gone),
        )
        .pyramid(PyramidRow::new(&gone).level(1616, 1077))
        .row_without_file(Level::sized(&gone, 1616, 1077))
        // 6: present, and below the default 640 px floor.
        .image(
            ImageRow::new(6, "IMG_0006")
                .path_from_root("2023/")
                .cache(&small),
        )
        .pyramid(PyramidRow::new(&small).level(320, 213))
        .level_file(Level::sized(&small, 320, 213));

    if wal { vault.with_wal() } else { vault }
}

/// The uuid shape the cache's shard rule expects: a GUID whose digits identify it.
fn uuid(row: u32) -> String {
    format!("AAAA{row:04}-1111-4111-8111-{row:012}")
}
