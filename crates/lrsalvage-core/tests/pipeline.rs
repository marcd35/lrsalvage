//! End-to-end tests: synthetic catalog + preview cache in, JPEGs out.
//!
//! Every fixture is built from scratch inside a `TempDir`; a real `.lrcat` is
//! never involved. The byte layouts follow the modules that read them
//! (`catalog`, `previews_db`, `lrprev`), and the assertions only use the public
//! seams: `Plan::build`, `extract` and `ensure_safe_destination`.
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
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use lrsalvage_core::catalog::Catalog;
use lrsalvage_core::error::Error;
use lrsalvage_core::extract::{self, ExtractOptions, OutcomeStatus};
use lrsalvage_core::jpeg;
use lrsalvage_core::plan::{Options, Plan, PlanItem, Status};
use lrsalvage_core::previews_db::PreviewIndex;
use rusqlite::{Connection, params};

/// Lightroom stores absolute paths with POSIX separators and a trailing slash.
/// The output tree is grouped by its last segment only.
const ROOT_FOLDER: &str = "F:/photog/2015/3. March/3.16 107 york street/";
const ROOT_NAME: &str = "3.16 107 york street";

const CATALOG_SCHEMA: &str = r#"
    CREATE TABLE Adobe_images (id_local INTEGER PRIMARY KEY, copyName TEXT,
      orientation TEXT, fileFormat TEXT, rootFile INTEGER);
    CREATE TABLE AgLibraryFile (id_local INTEGER PRIMARY KEY, folder INTEGER,
      baseName TEXT, extension TEXT);
    CREATE TABLE AgLibraryFolder (id_local INTEGER PRIMARY KEY, pathFromRoot TEXT,
      rootFolder INTEGER);
    CREATE TABLE AgLibraryRootFolder (id_local INTEGER PRIMARY KEY, absolutePath TEXT);
    CREATE TABLE Adobe_AdditionalMetadata (image INTEGER, xmp BLOB);
"#;

/// Column names and order are what `previews_db::PreviewIndex::open` selects.
const PREVIEWS_SCHEMA: &str = r#"
    CREATE TABLE Pyramid (uuid NOT NULL, digest NOT NULL, colorProfile NOT NULL,
      fileTimeStamp, quality NOT NULL, croppedWidth NOT NULL, croppedHeight NOT NULL,
      pyramidFileTimeStamp, fingerprint, fromProxy);
    CREATE TABLE PyramidLevel (uuid NOT NULL, digest NOT NULL, level NOT NULL,
      longDimension NOT NULL, lastAccess NOT NULL, width NOT NULL, height NOT NULL);
    CREATE TABLE ImageCacheEntry (imageId NOT NULL, uuid NOT NULL, digest NOT NULL,
      orientation);
"#;

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
    digest: &'static str,
    /// JPEG dimensions of each `level_N` chunk actually written. `None` means
    /// the cache row exists but the payload file is gone.
    levels: Option<&'static [(u16, u16)]>,
    /// Dimensions `PyramidLevel` claims, one row per entry.
    declared: &'static [(u16, u16)],
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
            digest: "aa01",
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
            digest: "aa02",
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
            digest: "aa04",
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
            digest: "aa05",
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
            digest: "aa06",
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
            digest: "aa07",
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
            digest: "aa08",
            levels: Some(&[(1616, 1077)]),
            declared: &[(1616, 1077)],
        }),
    },
];

/// Present in the cache, claimed by no `ImageCacheEntry`.
const ORPHAN_UUID: &str = "ZZZZ0009-1111-4111-8111-000000000009";
const ORPHAN_DIGEST: &str = "0bad";

/// A catalog, a preview cache and its payloads, all inside one temp dir.
struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Fixture {
            dir: tempfile::tempdir().expect("temp dir"),
        };
        fixture.write_catalog();
        fixture.write_previews();
        fixture
    }

    fn dir(&self) -> &Path {
        self.dir.path()
    }

    fn catalog_path(&self) -> PathBuf {
        self.dir().join("catalog").join("2023.lrcat")
    }

    fn cache_root(&self) -> PathBuf {
        self.dir().join("2023 Previews.lrdata")
    }

    fn output_root(&self) -> PathBuf {
        self.dir().join("recovered")
    }

    fn write_catalog(&self) {
        let dir = self.dir().join("catalog");
        fs::create_dir_all(&dir).expect("create catalog dir");
        let conn = Connection::open(self.catalog_path()).expect("create catalog");
        conn.execute_batch(CATALOG_SCHEMA).expect("catalog schema");
        conn.execute(
            "INSERT INTO AgLibraryRootFolder (id_local, absolutePath) VALUES (1, ?1)",
            [ROOT_FOLDER],
        )
        .expect("root folder");
        conn.execute(
            "INSERT INTO AgLibraryFolder (id_local, pathFromRoot, rootFolder) \
             VALUES (10, '', 1)",
            [],
        )
        .expect("folder");
        for (id, base_name) in FILES {
            conn.execute(
                "INSERT INTO AgLibraryFile (id_local, folder, baseName, extension) \
                 VALUES (?1, 10, ?2, 'CR2')",
                params![id, base_name],
            )
            .expect("file row");
        }
        for image in IMAGES {
            conn.execute(
                "INSERT INTO Adobe_images \
                 (id_local, copyName, orientation, fileFormat, rootFile) \
                 VALUES (?1, ?2, ?3, 'CR2', ?4)",
                params![image.id, image.copy_name, image.orientation, image.file],
            )
            .expect("image row");
            // Every image carries XMP, so the EXIF writer always has something
            // to say and the orientation tag is never dropped as a no-op.
            conn.execute(
                "INSERT INTO Adobe_AdditionalMetadata (image, xmp) VALUES (?1, ?2)",
                params![image.id, xmp_blob(XMP)],
            )
            .expect("xmp row");
        }
    }

    fn write_previews(&self) {
        let cache = self.cache_root();
        fs::create_dir_all(&cache).expect("create cache dir");
        let conn = Connection::open(cache.join("previews.db")).expect("create previews.db");
        conn.execute_batch(PREVIEWS_SCHEMA)
            .expect("previews schema");

        for image in IMAGES {
            let Some(spec) = &image.cache else { continue };
            // Numeric columns are REAL in real caches (`4062.0`, `1616.0`), so
            // the fixture stores them that way everywhere.
            conn.execute(
                "INSERT INTO ImageCacheEntry (imageId, uuid, digest, orientation) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![image.id as f64, spec.uuid, spec.digest, image.orientation],
            )
            .expect("cache entry");
            let (cropped_width, cropped_height) = spec.declared[0];
            conn.execute(
                "INSERT INTO Pyramid \
                 (uuid, digest, colorProfile, quality, croppedWidth, croppedHeight) \
                 VALUES (?1, ?2, 'ACR', 'HIGH', ?3, ?4)",
                params![
                    spec.uuid,
                    spec.digest,
                    f64::from(cropped_width),
                    f64::from(cropped_height)
                ],
            )
            .expect("pyramid");
            for (level, (width, height)) in spec.declared.iter().enumerate() {
                conn.execute(
                    "INSERT INTO PyramidLevel \
                     (uuid, digest, level, longDimension, lastAccess, width, height) \
                     VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)",
                    params![
                        spec.uuid,
                        spec.digest,
                        level as f64,
                        f64::from(*width.max(height)),
                        f64::from(*width),
                        f64::from(*height)
                    ],
                )
                .expect("pyramid level");
            }
        }
        drop(conn);

        for image in IMAGES {
            let Some(spec) = &image.cache else { continue };
            let Some(levels) = spec.levels else { continue };
            write_payload(&cache, spec.uuid, spec.digest, levels);
        }
        // An orphan payload: on disk, claimed by nobody.
        write_payload(&cache, ORPHAN_UUID, ORPHAN_DIGEST, &[(1024, 683)]);
    }

    fn plan(&self) -> Plan {
        let (_snapshot, catalog) =
            Catalog::open(&self.catalog_path()).expect("open fixture catalog");
        let index = PreviewIndex::open(&self.cache_root().join("previews.db"))
            .expect("open fixture previews.db");
        Plan::build(
            &catalog,
            &index,
            &self.cache_root(),
            &self.output_root(),
            &Options::default(),
        )
        .expect("build plan")
    }
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

/// Write one payload where Lightroom keeps it:
/// `<cache>/<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>.lrprev`.
fn write_payload(cache_root: &Path, uuid: &str, digest: &str, levels: &[(u16, u16)]) -> PathBuf {
    let first: String = uuid.chars().take(1).collect();
    let first_four: String = uuid.chars().take(4).collect();
    let dir = cache_root.join(first).join(first_four);
    fs::create_dir_all(&dir).expect("create payload dir");

    let path = dir.join(format!("{uuid}-{digest}.lrprev"));
    let mut bytes = chunk("header", header_text(levels).as_bytes(), 0);
    for (index, (width, height)) in levels.iter().enumerate() {
        let padding = if index + 1 == levels.len() { 0 } else { 9 };
        bytes.extend(chunk(
            &format!("level_{}", index + 1),
            &jpeg(*width, *height),
            padding,
        ));
    }
    fs::write(&path, bytes).expect("write payload");
    path
}

/// The Lua-like `header` chunk: a list of level dimensions, nothing reads it
/// on this path, but a real payload always carries one.
fn header_text(levels: &[(u16, u16)]) -> String {
    let mut text = String::from("pyramid = {\n\tlevels = {\n");
    for (width, height) in levels {
        text.push_str(&format!("\t\t{{ height = {height}, width = {width}, }},\n"));
    }
    text.push_str("\t},\n}\n");
    text
}

/// One `AgHg` chunk: magic, u16 header length, version, kind, u64 payload
/// length, u64 padding length, name, payload, padding.
fn chunk(name: &str, payload: &[u8], padding: usize) -> Vec<u8> {
    let header_len = 24 + name.len();
    let mut bytes = Vec::with_capacity(header_len + payload.len() + padding);
    bytes.extend_from_slice(b"AgHg");
    bytes.extend_from_slice(
        &u16::try_from(header_len)
            .expect("chunk header fits a u16")
            .to_be_bytes(),
    );
    bytes.push(0); // version
    bytes.push(0); // kind
    bytes.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    bytes.extend_from_slice(&(padding as u64).to_be_bytes());
    bytes.extend_from_slice(name.as_bytes());
    bytes.extend_from_slice(payload);
    // Padding bytes are deliberately not zero: they must be skipped, not read.
    bytes.resize(bytes.len() + padding, 0xEE);
    bytes
}

/// A structurally valid JPEG at `width` x `height`: SOI, APP0, SOF0, SOS, one
/// entropy byte, EOI. Nothing ever decodes pixels, so the tables stay tiny.
fn jpeg(width: u16, height: u16) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8];
    bytes.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10]);
    bytes.extend_from_slice(b"JFIF\0");
    bytes.extend_from_slice(&[0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
    bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&[0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
    bytes.extend_from_slice(&[
        0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00,
    ]);
    bytes.extend_from_slice(&[0x00, 0xFF, 0xD9]);
    bytes
}

/// zlib-compress `xml` behind the 4-byte prefix Lightroom uses for XMP blobs.
fn xmp_blob(xml: &str) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(xml.as_bytes()).expect("compress xmp");
    let compressed = encoder.finish().expect("finish xmp");
    let mut blob = vec![0u8; 4];
    blob.extend_from_slice(&compressed);
    blob
}
