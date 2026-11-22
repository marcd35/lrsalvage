//! Compose the downloadable practice folders through the shared synthetic vault builder.

use std::fs;
use std::path::PathBuf;

use lrsalvage_app::vault::copy_tree;
use lrsalvage_core::plan::Control;
use lrsalvage_fixtures::vault::{ImageRow, Level, PyramidRow, UnrecognisedFile, Vault};

const NAME: &str = "Practice";
const XMP: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:Rating="3"/></rdf:RDF></x:xmpmeta>"#;

struct Photo {
    name: &'static str,
    width: u32,
    height: u32,
    jpeg: &'static [u8],
}

macro_rules! photo {
    ($name:literal, $width:literal, $height:literal) => {
        Photo {
            name: $name,
            width: $width,
            height: $height,
            jpeg: include_bytes!(concat!("../../practice/photos/", $name, ".jpg")),
        }
    };
}

const CANYON: Photo = photo!("canyon", 960, 640);
const CANYON_PORTRAIT: Photo = photo!("canyon-portrait", 640, 960);
const CANYON_SMALL: Photo = photo!("canyon-small", 320, 213);
const SNOW: Photo = photo!("snow", 960, 640);
const SNOW_SMALL: Photo = photo!("snow-small", 480, 320);
const PLATEAU: Photo = photo!("plateau", 960, 640);
const PLATEAU_PORTRAIT: Photo = photo!("plateau-portrait", 640, 960);

fn main() {
    let out = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            eprintln!("usage: vault-practice <out-dir>");
            std::process::exit(2);
        });
    fs::create_dir_all(&out).expect("create samples directory");
    let cases = [
        ("working", working()),
        ("mixed", mixed()),
        (
            "unreadable-catalog",
            base(Vault::modern()).raw_catalog(b"An intentionally unreadable practice catalog."),
        ),
        (
            "unrecognized-layout",
            base(Vault::unrecognised())
                .image(ImageRow::new(1, "canyon"))
                .unrecognised_file(UnrecognisedFile::no_suffix().raw(CANYON.jpeg)),
        ),
    ];

    for (name, vault) in cases {
        let vault = vault.write();
        let folder = out.join(name);
        fs::create_dir(&folder).expect("create a new practice case folder");
        let catalog = vault.catalog();
        let cache = vault.previews();
        // A picked folder has to show the catalog and cache as siblings.
        copy_tree(catalog.parent().expect("catalog folder"), &folder);
        copy_tree(cache, &folder.join(cache.file_name().expect("cache name")));
        let report = lrsalvage_app::scan(
            &folder.join(catalog.file_name().expect("catalog name")),
            &folder.join(cache.file_name().expect("cache name")),
            &mut |_| Control::Continue,
        );
        match name {
            "working" => {
                let report = report.expect("working sample scans");
                assert_eq!(report["stats"]["images"], 6);
                assert_eq!(report["stats"]["selected_images"], 6);
            }
            "mixed" => {
                let report = report.expect("mixed sample scans");
                assert_eq!(report["stats"]["images"], 8);
                assert_eq!(report["stats"]["selected_images"], 4);
                assert_eq!(report["stats"]["below_floor"], 2);
                assert_eq!(report["stats"]["missing_payload"], 1);
                assert_eq!(report["stats"]["no_cache_entry"], 1);
            }
            "unreadable-catalog" => {
                let failure = report.expect_err("unreadable sample fails");
                assert_eq!(failure.to_json()["stage"], "open");
            }
            "unrecognized-layout" => {
                let report = report.expect("unknown layout still scans");
                assert_eq!(report["cache"]["layout"], "unknown");
                assert_eq!(report["stats"]["unrecognised"], 1);
                assert_eq!(report["stats"]["selected_images"], 0);
            }
            _ => unreachable!(),
        }
        println!("{name}: expected scan outcome");
    }
    let folder = out.join("cache-only");
    let vault = cache_only().write();
    fs::create_dir(&folder).expect("create cache-only sample");
    copy_tree(vault.previews(), &folder);
    let cache = lrsalvage_app::scan_cache(&folder, &mut |_| Control::Continue)
        .expect("cache-only sample scans");
    assert_eq!(cache.report["scan"]["indexed"], 3);
    assert_eq!(cache.report["scan"]["recoverable"], 2);
    assert_eq!(cache.report["scan"]["missing"], 1);
    assert!(
        cache.report["photos"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["fallback"] == true)
    );
    println!("cache-only: expected scan outcome");
}

fn cache_only() -> Vault {
    let healthy = uuid(1);
    let pruned = uuid(2);
    let missing = uuid(3);
    Vault::modern()
        .pyramid(PyramidRow::new(&healthy).level(CANYON.width, CANYON.height))
        .level_file(Level::sized(&healthy, CANYON.width, CANYON.height).raw(CANYON.jpeg))
        .pyramid(
            PyramidRow::new(&pruned)
                .level(SNOW.width, SNOW.height)
                .level(SNOW_SMALL.width, SNOW_SMALL.height),
        )
        .level_file(Level::sized(&pruned, SNOW_SMALL.width, SNOW_SMALL.height).raw(SNOW_SMALL.jpeg))
        .pyramid(PyramidRow::new(&missing).level(PLATEAU.width, PLATEAU.height))
}

fn base(vault: Vault) -> Vault {
    vault
        .catalog_name(NAME)
        .root_folder("/lrsalvage-practice-originals/")
}

fn working() -> Vault {
    let mut vault = base(Vault::modern());
    for (id, group, photo) in [
        (1, "Summer/", CANYON),
        (2, "Summer/", CANYON_PORTRAIT),
        (3, "Winter/", SNOW),
        (4, "Summer/", PLATEAU),
        (5, "Summer/", PLATEAU_PORTRAIT),
    ] {
        vault = add_photo(
            vault,
            ImageRow::new(id, photo.name).path_from_root(group),
            id,
            &photo,
        );
    }
    add_photo(
        vault,
        ImageRow::new(6, "canyon")
            .path_from_root("Summer/")
            .copy("Black and white")
            .file(1),
        6,
        &CANYON_PORTRAIT,
    )
}

fn mixed() -> Vault {
    let mut vault = base(Vault::modern());
    for (id, group, photo) in [
        (1, "Summer/", CANYON),
        (2, "Summer/", CANYON_PORTRAIT),
        (3, "Winter/", SNOW),
        (4, "Summer/", PLATEAU),
        (5, "Summer/", CANYON_SMALL),
        (6, "Winter/", SNOW_SMALL),
    ] {
        vault = add_photo(
            vault,
            ImageRow::new(id, photo.name).path_from_root(group),
            id,
            &photo,
        );
    }
    let missing = uuid(7);
    vault
        .image(
            ImageRow::new(7, "river")
                .path_from_root("Summer/")
                .cache(&missing),
        )
        .pyramid(PyramidRow::new(&missing).level(960, 640))
        .image(ImageRow::new(8, "valley").path_from_root("Winter/"))
}

fn add_photo(vault: Vault, image: ImageRow, id: i64, photo: &Photo) -> Vault {
    let uuid = uuid(id);
    vault
        .image(image.cache(&uuid).xmp(XMP))
        .pyramid(PyramidRow::new(&uuid).level(photo.width, photo.height))
        .level_file(Level::sized(&uuid, photo.width, photo.height).raw(photo.jpeg))
}

fn uuid(id: i64) -> String {
    format!("A{id:07}-1111-4111-8111-{id:012}")
}
