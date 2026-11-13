//! End-to-end tests for the `lrsalvage` binary.
//!
//! Every test drives the real executable built by Cargo and speaks only
//! through its command line, exit codes and stdout/stderr. A synthetic
//! catalog, preview cache and `AgHg` payload are built inside a `TempDir`, so
//! the suite needs no fixtures on disk, no network and no real Lightroom data.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use rusqlite::{Connection, params};
use tempfile::TempDir;

/// The `(uuid, digest)` pair Adobe wrote for the one preview in the fixture.
const UUID: &str = "ABCDEF01-2345-6789-ABCD-EF0123456789";
const DIGEST: &str = "0123456789abcdef0123456789abcdef";

/// Catalog tables `catalog.rs` reads. `Adobe_AdditionalMetadata` is omitted on
/// purpose: an old catalog that predates it must still be salvageable.
const CATALOG_SCHEMA: &str = r#"
    CREATE TABLE Adobe_images (id_local INTEGER PRIMARY KEY, copyName TEXT,
      orientation TEXT, fileFormat TEXT, rootFile INTEGER);
    CREATE TABLE AgLibraryFile (id_local INTEGER PRIMARY KEY, folder INTEGER,
      baseName TEXT, extension TEXT);
    CREATE TABLE AgLibraryFolder (id_local INTEGER PRIMARY KEY, pathFromRoot TEXT,
      rootFolder INTEGER);
    CREATE TABLE AgLibraryRootFolder (id_local INTEGER PRIMARY KEY, absolutePath TEXT);
"#;

/// The `previews.db` tables and columns `previews_db.rs` reads. The numeric
/// columns that real caches store as REAL are declared REAL here, so the
/// coercion is exercised rather than assumed.
const PREVIEWS_SCHEMA: &str = r#"
    CREATE TABLE Pyramid (uuid TEXT NOT NULL, digest TEXT NOT NULL,
      colorProfile TEXT NOT NULL, quality TEXT NOT NULL,
      croppedWidth REAL NOT NULL, croppedHeight REAL NOT NULL);
    CREATE TABLE PyramidLevel (uuid TEXT NOT NULL, digest TEXT NOT NULL,
      level INTEGER NOT NULL, longDimension REAL NOT NULL, lastAccess REAL NOT NULL,
      width INTEGER NOT NULL, height INTEGER NOT NULL);
    CREATE TABLE ImageCacheEntry (imageId REAL NOT NULL, uuid TEXT NOT NULL,
      digest TEXT NOT NULL, orientation TEXT);
"#;

fn lrsalvage() -> Command {
    Command::cargo_bin("lrsalvage").expect("the integration-test binary path")
}

/// One `AgHg` chunk: magic, header length, version, kind, payload/padding
/// lengths, name, payload. Mirrors `docs/format-notes.md` §1.
fn chunk(name: &str, payload: &[u8]) -> Vec<u8> {
    let header_len = 24 + name.len();
    let mut bytes = Vec::with_capacity(header_len + payload.len());
    bytes.extend_from_slice(b"AgHg");
    bytes.extend_from_slice(&u16::try_from(header_len).unwrap().to_be_bytes());
    bytes.push(0); // version
    bytes.push(0); // kind
    bytes.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    bytes.extend_from_slice(&0u64.to_be_bytes()); // padding
    bytes.extend_from_slice(name.as_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

/// A structurally valid baseline JPEG at `width` x `height`.
///
/// Tiny, but `jpeg::dimensions` finds the SOF and `jpeg::is_complete` finds the
/// final EOI. No pixels are ever decoded.
fn jpeg(width: u16, height: u16) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8];
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

/// A catalog with one image named `synthetic.CR2`, whose original is gone,
/// plus the preview cache that holds its one 800x600 JPEG.
struct Fixture {
    dir: TempDir,
    catalog: PathBuf,
    previews: PathBuf,
    out: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let catalog_dir = dir.path().join("catalog");
        fs::create_dir_all(&catalog_dir).expect("create catalog dir");
        let out = dir.path().join("out");
        fs::create_dir_all(&out).expect("create output dir");

        let catalog = catalog_dir.join("Test.lrcat");
        write_catalog(&catalog, &catalog_dir);
        let previews = catalog_dir.join("Test Previews.lrdata");
        write_previews(&previews);

        Self {
            dir,
            catalog,
            previews,
            out,
        }
    }

    fn dir(&self) -> &Path {
        self.dir.path()
    }

    fn catalog_dir(&self) -> PathBuf {
        self.catalog
            .parent()
            .expect("catalog has a parent")
            .to_path_buf()
    }

    fn scan(&self) -> Command {
        let mut command = lrsalvage();
        command
            .arg("scan")
            .arg("--catalog")
            .arg(&self.catalog)
            .arg("--previews")
            .arg(&self.previews);
        command
    }

    fn extract(&self) -> Command {
        let mut command = lrsalvage();
        command
            .arg("extract")
            .arg("--catalog")
            .arg(&self.catalog)
            .arg("--previews")
            .arg(&self.previews)
            .arg("--out")
            .arg(&self.out);
        command
    }
}

fn write_catalog(catalog: &Path, catalog_dir: &Path) {
    let conn = Connection::open(catalog).expect("create catalog");
    conn.execute_batch(CATALOG_SCHEMA).expect("catalog schema");

    // Lightroom stores POSIX separators and a trailing slash.
    let originals = format!(
        "{}/",
        catalog_dir
            .join("originals")
            .to_string_lossy()
            .replace('\\', "/")
    );
    conn.execute(
        "INSERT INTO AgLibraryRootFolder (id_local, absolutePath) VALUES (1, ?1)",
        params![originals],
    )
    .expect("root folder");
    conn.execute(
        "INSERT INTO AgLibraryFolder (id_local, pathFromRoot, rootFolder) VALUES (10, '', 1)",
        [],
    )
    .expect("folder");
    conn.execute(
        "INSERT INTO AgLibraryFile (id_local, folder, baseName, extension) \
         VALUES (100, 10, 'synthetic', 'CR2')",
        [],
    )
    .expect("file row");
    conn.execute(
        "INSERT INTO Adobe_images (id_local, copyName, orientation, fileFormat, rootFile) \
         VALUES (1, NULL, 'AB', 'CR2', 100)",
        [],
    )
    .expect("image row");
}

fn write_previews(previews: &Path) {
    fs::create_dir_all(previews).expect("create preview cache");
    let conn = Connection::open(previews.join("previews.db")).expect("create previews.db");
    conn.execute_batch(PREVIEWS_SCHEMA)
        .expect("previews schema");
    conn.execute(
        "INSERT INTO ImageCacheEntry (imageId, uuid, digest, orientation) \
         VALUES (1.0, ?1, ?2, 'AB')",
        params![UUID, DIGEST],
    )
    .expect("cache entry");
    conn.execute(
        "INSERT INTO Pyramid (uuid, digest, colorProfile, quality, croppedWidth, croppedHeight) \
         VALUES (?1, ?2, 'sRGB', 'standard', 800.0, 600.0)",
        params![UUID, DIGEST],
    )
    .expect("pyramid");
    conn.execute(
        "INSERT INTO PyramidLevel \
         (uuid, digest, level, longDimension, lastAccess, width, height) \
         VALUES (?1, ?2, 1, 800.0, 0.0, 800, 600)",
        params![UUID, DIGEST],
    )
    .expect("pyramid level");
    drop(conn);

    // `<cache>/<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>.lrprev`
    let payload_dir = previews.join("A").join("ABCD");
    fs::create_dir_all(&payload_dir).expect("create payload dir");
    let mut bytes = chunk("header", b"pyramid = { }");
    bytes.extend(chunk("level_1", &jpeg(800, 600)));
    fs::write(payload_dir.join(format!("{UUID}-{DIGEST}.lrprev")), bytes).expect("write payload");
}

fn jpeg_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect_jpegs(dir, &mut found);
    found.sort();
    found
}

fn collect_jpegs(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read output dir") {
        let entry = entry.expect("directory entry");
        let path = entry.path();
        if entry.file_type().expect("file type").is_dir() {
            collect_jpegs(&path, found);
        } else if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("jpg"))
        {
            found.push(path);
        }
    }
}

fn assert_empty_output(fixture: &Fixture) {
    let entries: Vec<_> = fs::read_dir(&fixture.out)
        .expect("read output dir")
        .collect();
    assert!(
        entries.is_empty(),
        "expected no files under {}, found {:?}",
        fixture.out.display(),
        entries
            .into_iter()
            .map(|entry| entry.expect("directory entry").path())
            .collect::<Vec<_>>()
    );
}

#[test]
fn help_lists_every_subcommand() {
    lrsalvage().arg("--help").assert().code(0).stdout(
        predicate::str::contains("scan")
            .and(predicate::str::contains("extract"))
            .and(predicate::str::contains("masks")),
    );
}

#[test]
fn unknown_subcommand_and_unknown_flag_are_usage_errors() {
    // Clap's default is 2, but 2 already means "partial" here, so both must
    // be reported as a plain usage error instead.
    lrsalvage()
        .arg("frobnicate")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("unrecognized subcommand"));

    lrsalvage()
        .arg("--nope")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("unexpected argument"));
}

#[test]
fn missing_catalog_is_a_usage_error_that_names_the_path() {
    let fixture = Fixture::new();
    let missing = fixture.dir().join("missing.lrcat");

    lrsalvage()
        .arg("scan")
        .arg("--catalog")
        .arg(&missing)
        .arg("--previews")
        .arg(&fixture.previews)
        .assert()
        .code(1)
        .stderr(
            predicate::str::contains("i/o error")
                .and(predicate::str::contains(missing.display().to_string())),
        );
}

#[test]
fn a_text_file_is_rejected_as_a_catalog() {
    let fixture = Fixture::new();
    let text = fixture.dir().join("notes.lrcat");
    fs::write(&text, b"These are notes, not a SQLite database.\n").expect("write text file");

    lrsalvage()
        .arg("scan")
        .arg("--catalog")
        .arg(&text)
        .arg("--previews")
        .arg(&fixture.previews)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("not a database"));
}

#[test]
fn extract_refuses_an_output_inside_the_catalog_directory() {
    let fixture = Fixture::new();
    let inside = fixture.catalog_dir().join("refused");

    lrsalvage()
        .arg("extract")
        .arg("--catalog")
        .arg(&fixture.catalog)
        .arg("--previews")
        .arg(&fixture.previews)
        .arg("--out")
        .arg(&inside)
        .assert()
        .code(4)
        .stderr(predicate::str::contains("refusing to write inside"));

    assert!(
        !inside.exists(),
        "a refused --out must not be created: {}",
        inside.display()
    );
}

#[test]
fn extract_refuses_an_output_inside_the_preview_cache() {
    let fixture = Fixture::new();
    let inside = fixture.previews.join("refused");

    lrsalvage()
        .arg("extract")
        .arg("--catalog")
        .arg(&fixture.catalog)
        .arg("--previews")
        .arg(&fixture.previews)
        .arg("--out")
        .arg(&inside)
        .assert()
        .code(4)
        .stderr(predicate::str::contains("refusing to write inside"));

    assert!(
        !inside.exists(),
        "a refused --out must not be created: {}",
        inside.display()
    );
}

#[test]
fn scan_and_extract_recover_one_synthetic_preview() {
    let fixture = Fixture::new();

    fixture
        .scan()
        .assert()
        .code(0)
        .stdout(predicate::str::is_match(r"(?m)^recovered\s+1(\s|$)").unwrap());

    fixture.extract().assert().code(0);

    let jpegs = jpeg_files(&fixture.out);
    assert_eq!(
        jpegs.len(),
        1,
        "expected exactly one recovered JPEG, found {jpegs:?}"
    );
    assert_eq!(
        jpegs[0].strip_prefix(&fixture.out).unwrap(),
        Path::new("originals").join("synthetic.jpg"),
        "the output tree must mirror the catalog's root folder"
    );
    let bytes = fs::read(&jpegs[0]).expect("read recovered JPEG");
    assert!(
        bytes.ends_with(&[0xFF, 0xD9]),
        "a recovered JPEG must end with an EOI marker"
    );

    for name in ["report.csv", "manifest.json", "extract.csv"] {
        let path = fixture.out.join(name);
        assert!(path.is_file(), "expected {} to be written", path.display());
    }
}

#[test]
fn a_second_extract_is_idempotent() {
    let fixture = Fixture::new();
    fixture.extract().assert().code(0);

    fixture
        .extract()
        .assert()
        .code(0)
        .stderr(predicate::str::contains("0 written").and(predicate::str::contains("1 skipped")));

    assert_eq!(
        jpeg_files(&fixture.out).len(),
        1,
        "resuming must not write a second copy of the same preview"
    );
}

#[test]
fn dry_run_writes_nothing() {
    let fixture = Fixture::new();

    fixture
        .extract()
        .arg("--dry-run")
        .assert()
        .code(0)
        .stderr(predicate::str::contains("0 written (dry run)"));

    assert_empty_output(&fixture);
}

#[test]
fn a_floor_above_every_level_exits_three_without_writing() {
    let fixture = Fixture::new();

    fixture
        .extract()
        .arg("--min-long-edge")
        .arg("99999")
        .assert()
        .code(3)
        .stderr(predicate::str::contains("nothing to extract"));

    assert_empty_output(&fixture);
}

#[test]
fn json_scan_keeps_stdout_parseable_with_verbose() {
    let fixture = Fixture::new();

    let assert = fixture
        .scan()
        .arg("--json")
        .arg("--verbose")
        .assert()
        .code(0);
    let output = assert.get_output();

    let stdout = String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8");
    assert_eq!(
        stdout.trim().lines().count(),
        1,
        "--json must emit exactly one line on stdout, got:\n{stdout}"
    );
    let json: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("stdout must be exactly one JSON object");

    assert_eq!(json["images"].as_u64(), Some(1), "unexpected JSON: {json}");
    assert_eq!(
        json["recovered"].as_u64(),
        Some(1),
        "unexpected JSON: {json}"
    );
    assert!(
        json.get("original_present").is_some(),
        "the summary must carry original_present: {json}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.trim().is_empty(),
        "the per-image --verbose lines must go to stderr when --json is asked for"
    );
}

#[test]
fn verbose_scan_prints_one_line_per_image() {
    let fixture = Fixture::new();

    fixture.scan().arg("--verbose").assert().code(0).stdout(
        predicate::str::is_match(
            r"(?m)^recovered\s+small\s+800x600\s+sRGB\s+originals/synthetic\.jpg$",
        )
        .unwrap(),
    );
}
