//! Decodable synthetic patterns and structural-only JPEGs for browser review.

use crate::bytes;
use crate::vault::{ImageRow, Level, PyramidRow, Vault};

pub const BOUNDARY: &[u8] = include_bytes!("../assets/review/bridge.jpg");

/// Ten catalog rows that distinguish selection, availability and JPEG decoding.
pub fn vault() -> Vault {
    let mut vault = Vault::modern().root_folder("X:/Synthetic review/");
    for (id, name, folder, claim, jpeg) in [
        (
            1,
            "Meadow",
            "2023/",
            (900, 600),
            include_bytes!("../assets/review/meadow.jpg").as_slice(),
        ),
        (2, "Bridge", "2023/", (640, 480), BOUNDARY),
        (
            3,
            "Portrait",
            "2024/",
            (800, 512),
            include_bytes!("../assets/review/portrait.jpg").as_slice(),
        ),
        (
            4,
            "Orchard",
            "2024/",
            (720, 500),
            include_bytes!("../assets/review/orchard.jpg").as_slice(),
        ),
        (5, "Unknown", "", (640, 480), BOUNDARY),
        (
            6,
            "Broken",
            "2023/",
            (950, 700),
            &bytes::jpeg_without_scan(950, 700),
        ),
        (
            7,
            "Creek",
            "2024/",
            (639, 426),
            include_bytes!("../assets/review/creek.jpg").as_slice(),
        ),
        (
            8,
            "Island",
            "2023/",
            (320, 220),
            include_bytes!("../assets/review/island.jpg").as_slice(),
        ),
    ] {
        let uuid = format!("AAAA{id:04}-1111-4111-8111-{id:012}");
        let mut photo = ImageRow::new(id, name).path_from_root(folder).cache(&uuid);
        if id == 3 {
            photo = photo.orientation("BC");
        }
        if id == 5 {
            photo = photo.without_file();
        }
        vault = vault
            .image(photo)
            .pyramid(PyramidRow::new(&uuid).level(claim.0, claim.1))
            .level_file(Level::sized(&uuid, claim.0, claim.1).raw(jpeg));
    }
    vault
        .image(ImageRow::new(9, "Missing index entry").path_from_root("2024/"))
        .image(
            ImageRow::new(10, "Missing file")
                .path_from_root("2023/")
                .cache("AAAA0010-1111-4111-8111-000000000010"),
        )
        .pyramid(PyramidRow::new("AAAA0010-1111-4111-8111-000000000010").level(640, 480))
}

/// The recorded Meadow original, kept separate until the app rechecks it.
pub fn originals() -> Vault {
    Vault::modern().image(
        ImageRow::new(1, "Meadow")
            .path_from_root("2023/")
            .original_on_disk(),
    )
}
