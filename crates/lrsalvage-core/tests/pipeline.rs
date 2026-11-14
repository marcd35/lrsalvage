//! End-to-end tests: synthetic catalog + preview cache in, JPEGs out.
//!
//! The catalog, the preview cache and every payload come from
//! `lrsalvage-fixtures`, built inside a `TempDir`; a real `.lrcat` is never
//! involved. This suite therefore holds no byte layout of its own, and the
//! assertions only use the public seams: `Plan::build`, `extract` and
//! `ensure_safe_destination`.
//!
//! One fixture covers one image of each kind the planner can report:
//!
//! | image | what it is                                        | status          |
//! |-------|---------------------------------------------------|-----------------|
//! | 1     | payload present, dims agree with `previews.db`    | Recoverable     |
//! | 2     | cache row present, payload file gone              | MissingPayload  |
//! | 3     | catalog image with no cache row                   | NoCacheEntry    |
//! | 4     | payload decodes to a different size than declared | Suspect         |
//! | 5     | intact but below the 640 px floor                 | Recoverable     |
//! | 6     | virtual copy ("Copy 1")                           | Recoverable     |
//! | 7, 8  | two images over one file row (output collision)   | Recoverable     |
//!
//! plus one payload (uuid `ZZZZ…`) that no cache entry claims: Unmapped.

use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use lrsalvage_core::error::Error;
use lrsalvage_core::extract::{self, ExtractOptions, OutcomeStatus};
use lrsalvage_core::jpeg;
use lrsalvage_core::plan::{Options, Plan, PlanItem, Status};
use lrsalvage_fixtures::vault::{ImageRow, Level, PyramidRow, Vault};

/// Lightroom stores absolute paths with POSIX separators and a trailing slash.
/// The output tree is grouped by its last segment only.
const ROOT_FOLDER: &str = "F:/photog/2015/3. March/3.16 107 york street/";
const ROOT_NAME: &str = "3.16 107 york street";

const XMP: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:tiff="http://ns.adobe.com/tiff/1.0/"
    xmlns:exif="http://ns.adobe.com/exif/1.0/"
    xmp:CreateDate="2015-03-16T10:00:00"
    exif:DateTimeOriginal="2015-03-16T10:00:00"
    tiff:Make="SONY"
    tiff:Model="DSLR-A350"/>
 </rdf:RDF>
</x:xmpmeta>"#;

/// One `AgLibraryFile` row: two images may share one (`7`/`8` below).
const FILES: &[(i64, &str)] = &[
    (101, "IMG_0001"),
    (102, "IMG_0002"),
    (103, "IMG_0003"),
    (104, "IMG_0004"),
    (105, "IMG_0005"),
    (106, "IMG_0006"),
    (107, "IMG_0007"),
];

struct CacheSpec {
    uuid: &'static str,
    /// JPEG dimensions of each `level_N` chunk actually written. `None` means
    /// the cache row exists but the payload file is gone.
    levels: Option<&'static [(u32, u32)]>,
    /// Dimensions `PyramidLevel` claims, one row per entry.
    declared: &'static [(u32, u32)],
}

struct ImageSpec {
    id: i64,
    file: i64,
    copy_name: Option<&'static str>,
    /// Catalog two-letter code, copied into `ImageCacheEntry.orientation` too.
    orientation: &'static str,
    /// `None`: the image is in the catalog but no cache row exists.
    cache: Option<CacheSpec>,
}

const IMAGES: &[ImageSpec] = &[
    ImageSpec {
        id: 1,
        file: 101,
        copy_name: None,
        orientation: "AB",
        cache: Some(CacheSpec {
            uuid: "AAAA0001-1111-4111-8111-000000000001",
            levels: Some(&[(101, 68), (1616, 1077)]),
            declared: &[(1616, 1077), (640, 427)],
        }),
    },
    ImageSpec {
        id: 2,
        file: 102,
        copy_name: None,
        orientation: "AB",
        cache: Some(CacheSpec {
            uuid: "AAAA0002-1111-4111-8111-000000000002",
            levels: None,
            declared: &[(1616, 1077)],
        }),
    },
    ImageSpec {
        id: 3,
        file: 103,
        copy_name: None,
        orientation: "AB",
        cache: None,
    },
    ImageSpec {
        id: 4,
        file: 104,
        copy_name: None,
        orientation: "BC",
        cache: Some(CacheSpec {
            uuid: "AAAA0004-1111-4111-8111-000000000004",
            levels: Some(&[(800, 600)]),
            declared: &[(1616, 1077)],
        }),
    },
    ImageSpec {
        id: 5,
        file: 105,
        copy_name: None,
        orientation: "AB",
        cache: Some(CacheSpec {
            uuid: "AAAA0005-1111-4111-8111-000000000005",
            levels: Some(&[(320, 240)]),
            declared: &[(320, 240)],
        }),
    },
    ImageSpec {
        id: 6,
        file: 106,
        copy_name: Some("Copy 1"),
        orientation: "AB",
        cache: Some(CacheSpec {
            uuid: "AAAA0006-1111-4111-8111-000000000006",
            levels: Some(&[(1616, 1077)]),
            declared: &[(1616, 1077)],
        }),
    },
    ImageSpec {
        id: 7,
        file: 107,
        copy_name: None,
        orientation: "AB",
        cache: Some(CacheSpec {
            uuid: "AAAA0007-1111-4111-8111-000000000007",
            levels: Some(&[(1616, 1077)]),
            declared: &[(1616, 1077)],
        }),
    },
    ImageSpec {
        id: 8,
        file: 107,
        copy_name: None,
        orientation: "AB",
        cache: Some(CacheSpec {
            uuid: "AAAA0008-1111-4111-8111-000000000008",
            levels: Some(&[(1616, 1077)]),
            declared: &[(1616, 1077)],
        }),
    },
];

/// Present in the cache, claimed by no `ImageCacheEntry`.
const ORPHAN_UUID: &str = "ZZZZ0009-1111-4111-8111-000000000009";

/// A catalog, a preview cache and its payloads, all inside one temp dir.
///
/// Both databases and every byte come from `lrsalvage-fixtures`: this suite
/// keeps no fixture of its own beside it.
struct Fixture {
    vault: Vault,
}

impl Fixture {
    fn new() -> Self {
        Fixture { vault: build() }
    }

    fn dir(&self) -> &Path {
        self.vault.dir()
    }

    fn catalog_path(&self) -> PathBuf {
        self.vault.catalog().to_path_buf()
    }

    fn cache_root(&self) -> PathBuf {
        self.vault.previews().to_path_buf()
    }

    fn output_root(&self) -> PathBuf {
        self.vault.out().to_path_buf()
    }

    fn plan(&self) -> Plan {
        self.vault.plan(&Options::default())
    }
}

/// A `legacy` vault: every pyramid is an `.lrprev` container, which is the one
/// layout this version reads.
fn build() -> Vault {
    let mut vault = Vault::legacy()
        .catalog_name("2023")
        .root_folder(ROOT_FOLDER);

    for spec in IMAGES {
        let base_name = FILES
            .iter()
            .find(|(id, _)| *id == spec.file)
            .map(|(_, base_name)| *base_name)
            .expect("a file row for every image");

        let mut image = ImageRow::new(spec.id, base_name)
            .file(spec.file)
            .orientation(spec.orientation)
            // Every image carries XMP, so the EXIF writer always has something
            // to say and the orientation tag is never dropped as a no-op.
            .xmp(XMP);
        if let Some(copy_name) = spec.copy_name {
            image = image.copy(copy_name);
        }

        if let Some(cache) = &spec.cache {
            image = image.cache(cache.uuid);

            let mut pyramid = PyramidRow::new(cache.uuid).profile("ACR").quality("HIGH");
            for (width, height) in cache.declared {
                pyramid = pyramid.level(*width, *height);
            }
            vault = vault.pyramid(pyramid);

            // Only the levels whose files exist: a declared level with no
            // entry here is a stale row, which this cache tolerates.
            for (width, height) in cache.levels.unwrap_or(&[]) {
                vault = vault.level_file(Level::sized(cache.uuid, *width, *height));
            }
        }

        vault = vault.image(image);
    }

    // An orphan payload: on disk, claimed by nobody.
    vault
        .unclaimed(&[Level::sized(ORPHAN_UUID, 1024, 683)])
        .write()
}

fn item_for(plan: &Plan, image_id: i64) -> &PlanItem {
    plan.items
        .iter()
        .find(|item| {
            item.image
                .as_ref()
                .is_some_and(|image| image.id_local == image_id)
        })
        .unwrap_or_else(|| panic!("no plan item for image {image_id}"))
}

#[test]
fn plan_classifies_every_kind_of_image_and_still_selects_suspects() {
    let fixture = Fixture::new();
    let plan = fixture.plan();

    assert_eq!(plan.stats.images, 8, "one item per catalog image");
    assert_eq!(item_for(&plan, 1).status, Status::Recoverable);
    assert_eq!(item_for(&plan, 2).status, Status::MissingPayload);
    assert_eq!(item_for(&plan, 3).status, Status::NoCacheEntry);
    assert_eq!(item_for(&plan, 4).status, Status::Suspect);
    assert_eq!(
        item_for(&plan, 5).status,
        Status::Recoverable,
        "small but intact is still recoverable"
    );

    // Premise: image 2's payload really is absent from the cache, not merely
    // reported as such.
    let missing = item_for(&plan, 2);
    let missing_payload = missing.payload.as_ref().expect("payload path is known");
    assert!(
        !missing_payload.exists(),
        "fixture must not write {}",
        missing_payload.display()
    );
    assert!(!missing.selected);

    // A suspect payload is a real photo: flagged, and still written.
    let suspect = item_for(&plan, 4);
    assert!(suspect.selected, "a suspect payload is still recovered");
    assert_eq!((suspect.width, suspect.height), (Some(800), Some(600)));
    assert_eq!(suspect.declared, Some((1616, 1077)));
    assert!(
        suspect
            .note
            .as_deref()
            .is_some_and(|note| note.contains("800x600") && note.contains("1616x1077")),
        "note should name both sizes, got {:?}",
        suspect.note
    );

    // Floor: 320 px on the long edge is below the default 640, so the item is
    // planned but not selected, and counted exactly once.
    let small = item_for(&plan, 5);
    assert_eq!(small.long_edge(), 320);
    assert!(!small.selected, "below the floor must not be extracted");
    assert_eq!(plan.stats.below_floor, 1);

    // Orphan payload: present in the cache, mappable to no image.
    let unmapped: Vec<&PlanItem> = plan
        .items
        .iter()
        .filter(|item| item.status == Status::Unmapped)
        .collect();
    assert_eq!(unmapped.len(), 1, "exactly the ZZZZ payload is unclaimed");
    assert!(unmapped[0].image.is_none());
    assert!(!unmapped[0].selected);
    assert!(
        unmapped[0]
            .payload
            .as_ref()
            .is_some_and(|path| path.is_file()),
        "the unmapped payload exists, it just has no catalog image"
    );

    assert_eq!(plan.stats.recoverable, 5, "images 1, 5, 6, 7, 8");
    assert_eq!(plan.stats.suspect, 1);
    assert_eq!(plan.stats.missing_payload, 1);
    assert_eq!(plan.stats.no_cache_entry, 1);
    assert_eq!(plan.stats.unmapped, 1);
    assert_eq!(plan.stats.failed, 0);
    assert_eq!(plan.selected().count(), 5, "suspect included, orphan not");
}

#[test]
fn plan_mirrors_the_catalog_folder_tree_into_output_paths() {
    let fixture = Fixture::new();
    let plan = fixture.plan();

    let normal = item_for(&plan, 1);
    let image = normal.image.as_ref().expect("image data");
    assert_eq!(image.root_display_name(), ROOT_NAME);
    assert_eq!(
        image.relative_output_path(),
        PathBuf::from(ROOT_NAME).join("IMG_0001.jpg")
    );
    assert_eq!(
        normal.output.as_deref(),
        Some(
            fixture
                .output_root()
                .join(ROOT_NAME)
                .join("IMG_0001.jpg")
                .as_path()
        )
    );

    // A virtual copy is its own image with a suffixed stem.
    let copy = item_for(&plan, 6);
    assert_eq!(
        copy.image
            .as_ref()
            .expect("image data")
            .relative_output_path(),
        PathBuf::from(ROOT_NAME).join("IMG_0006 - Copy 1.jpg")
    );
    assert_eq!(copy.status, Status::Recoverable);

    // Two catalog images over one file row resolve to one path, so the second
    // gets a numeric suffix instead of overwriting the first.
    let first = item_for(&plan, 7).output.as_ref().expect("output");
    let second = item_for(&plan, 8).output.as_ref().expect("output");
    assert_eq!(first.file_name().unwrap(), "IMG_0007.jpg");
    assert_eq!(second.file_name().unwrap(), "IMG_0007 (2).jpg");
    assert_eq!(first.parent(), second.parent());
}

#[test]
fn ensure_safe_destination_refuses_the_catalog_and_the_cache() {
    let fixture = Fixture::new();
    let catalog_path = fixture.catalog_path();
    let cache_root = fixture.cache_root();
    let dir = fixture.dir().to_path_buf();

    let in_catalog = dir.join("catalog").join("recovered");
    fs::create_dir_all(&in_catalog).expect("create");
    assert_unsafe(&in_catalog, &catalog_path, &cache_root);

    // The catalog's own folder, not just a child of it.
    assert_unsafe(&dir.join("catalog"), &catalog_path, &cache_root);

    let in_cache = cache_root.join("recovered");
    fs::create_dir_all(&in_cache).expect("create");
    assert_unsafe(&in_cache, &catalog_path, &cache_root);

    // A sibling of both trees is exactly what an output root should be.
    let sibling = dir.join("recovered");
    fs::create_dir_all(&sibling).expect("create");
    extract::ensure_safe_destination(&sibling, &catalog_path, &cache_root)
        .expect("a sibling directory is a safe destination");
}

fn assert_unsafe(output: &Path, catalog_path: &Path, cache_root: &Path) {
    match extract::ensure_safe_destination(output, catalog_path, cache_root) {
        Err(Error::UnsafeDestination(path)) => assert_eq!(path, output),
        other => panic!(
            "expected UnsafeDestination for {}, got {other:?}",
            output.display()
        ),
    }
}

#[test]
fn extract_writes_the_selected_previews_and_skips_them_on_a_rerun() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
    let output_root = fixture.output_root();
    fs::create_dir_all(&output_root).expect("create output root");
    extract::ensure_safe_destination(&output_root, &fixture.catalog_path(), &fixture.cache_root())
        .expect("output root is outside the evidence");

    let options = ExtractOptions {
        output_root: output_root.clone(),
        ..ExtractOptions::default()
    };

    let mut progress = 0usize;
    let outcomes = extract::extract(&plan, &options, &mut |_| progress += 1).expect("extract");
    assert_eq!(progress, outcomes.len(), "progress fires once per outcome");
    assert_eq!(
        outcomes.len(),
        5,
        "the selected items only, suspect included"
    );
    assert!(
        outcomes
            .iter()
            .all(|outcome| outcome.status == OutcomeStatus::Written),
        "{outcomes:#?}"
    );

    // (image, file name, EXIF orientation, decoded frame)
    let expected = [
        (1, "IMG_0001.jpg", 1u32, (1616u32, 1077u32)),
        (4, "IMG_0004.jpg", 6, (800, 600)),
        (6, "IMG_0006 - Copy 1.jpg", 1, (1616, 1077)),
        (7, "IMG_0007.jpg", 1, (1616, 1077)),
        (8, "IMG_0007 (2).jpg", 1, (1616, 1077)),
    ];
    for (id, name, orientation, dims) in expected {
        let item = item_for(&plan, id);
        let output = item.output.as_ref().expect("output path");
        assert_eq!(output.file_name().unwrap(), name, "image {id}");

        let bytes = fs::read(output).unwrap_or_else(|e| panic!("{}: {e}", output.display()));
        assert!(
            bytes.starts_with(&[0xFF, 0xD8]),
            "{name} must start with SOI"
        );
        assert!(bytes.ends_with(&[0xFF, 0xD9]), "{name} must end with EOI");
        assert_eq!(
            jpeg::dimensions(&bytes).expect("readable frame"),
            dims,
            "{name} keeps the payload's own frame"
        );
        assert_eq!(
            exif_app1(&bytes).len(),
            1,
            "{name} carries exactly one Exif APP1"
        );
        assert_eq!(
            orientation_of(&bytes),
            orientation,
            "{name} EXIF orientation (catalog code → tag)"
        );
    }

    // A second run must trust what is already there and write nothing.
    let before = tree(&output_root);
    assert_eq!(before.len(), 5);
    let second = extract::extract(&plan, &options, &mut |_| {}).expect("second extract");
    assert_eq!(second.len(), outcomes.len());
    assert!(
        second
            .iter()
            .all(|outcome| outcome.status == OutcomeStatus::Skipped),
        "a finished tree is skipped, not rewritten: {second:#?}"
    );
    assert!(
        second.iter().all(|outcome| outcome.sha256.is_none()),
        "a skip has no digest: nothing was hashed into a file"
    );
    assert_eq!(
        second.iter().map(|outcome| outcome.bytes).sum::<u64>(),
        before.values().map(|bytes| bytes.len() as u64).sum::<u64>(),
        "skipped files are still reported at their on-disk size"
    );
    assert_eq!(tree(&output_root), before, "not one byte was rewritten");

    // Every write goes through `<name>.jpg.partial` and is renamed into place;
    // none of them may survive.
    assert!(
        partials(&output_root).is_empty(),
        "partial files left behind: {:?}",
        partials(&output_root)
    );
}

#[test]
fn dry_run_reports_what_it_would_write_and_writes_nothing() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
    let output_root = fixture.output_root();
    fs::create_dir_all(&output_root).expect("create output root");

    let options = ExtractOptions {
        output_root: output_root.clone(),
        dry_run: true,
        ..ExtractOptions::default()
    };
    let outcomes = extract::extract(&plan, &options, &mut |_| {}).expect("dry run");

    assert_eq!(outcomes.len(), plan.selected().count());
    assert!(
        outcomes
            .iter()
            .all(|outcome| outcome.status == OutcomeStatus::DryRun),
        "{outcomes:#?}"
    );
    assert!(
        outcomes
            .iter()
            .all(|outcome| outcome.bytes > 0 && outcome.sha256.is_none()),
        "a dry run knows the size, not a digest"
    );
    assert!(
        tree(&output_root).is_empty(),
        "dry run left files behind: {:?}",
        tree(&output_root)
    );
}

/// Every regular file below `root`, keyed by its path relative to `root`.
fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            let path = entry.expect("readable entry").path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .expect("child of the output root")
                    .to_path_buf();
                files.insert(relative, fs::read(&path).expect("readable file"));
            }
        }
    }
    files
}

fn partials(root: &Path) -> Vec<PathBuf> {
    tree(root)
        .into_keys()
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "partial")
        })
        .collect()
}

/// Payloads of every `FF E1` APP1 segment that opens with `Exif\0\0`.
fn exif_app1(bytes: &[u8]) -> Vec<&[u8]> {
    let mut segments = Vec::new();
    let mut pos = 2;
    while pos + 4 <= bytes.len() {
        assert_eq!(bytes[pos], 0xFF, "well-formed header segment");
        let marker = bytes[pos + 1];
        if marker == 0xDA {
            break;
        }
        let length = u16::from_be_bytes([bytes[pos + 2], bytes[pos + 3]]) as usize;
        let payload = &bytes[pos + 4..pos + 2 + length];
        if marker == 0xE1 && payload.starts_with(b"Exif\0\0") {
            segments.push(payload);
        }
        pos += 2 + length;
    }
    segments
}

/// Read the Orientation tag back with an independent EXIF parser.
fn orientation_of(bytes: &[u8]) -> u32 {
    let parsed = exif::Reader::new()
        .read_from_container(&mut Cursor::new(bytes))
        .expect("written EXIF must parse back");
    parsed
        .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|field| field.value.get_uint(0))
        .expect("orientation tag present")
}
