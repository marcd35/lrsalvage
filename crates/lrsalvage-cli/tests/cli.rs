//! End-to-end tests for the `lrsalvage` binary.
//!
//! Every test drives the real executable built by Cargo and speaks only
//! through its command line, exit codes and stdout/stderr. The catalog, the
//! preview cache and its `AgHg` payload come from `lrsalvage-fixtures`, built
//! inside a `TempDir`, so the suite needs no fixtures on disk, no network and
//! no real Lightroom data.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use lrsalvage_core::plan::Options;
use lrsalvage_fixtures::vault::{ImageRow, Level, PyramidRow, Vault};
use predicates::prelude::*;

/// The `(uuid, digest)` pair Adobe wrote for the one preview in the fixture.
/// The digest itself is derived by the builder, so rows and file always agree.
const UUID: &str = "ABCDEF01-2345-6789-ABCD-EF0123456789";

fn lrsalvage() -> Command {
    Command::cargo_bin("lrsalvage").expect("the integration-test binary path")
}

/// A catalog with one image named `synthetic.CR2`, whose original is gone,
/// plus the preview cache that holds its one 800x600 JPEG.
///
/// Both databases and the payload come from `lrsalvage-fixtures`; this suite
/// keeps no fixture of its own and describes no bytes.
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

    fn catalog(&self) -> &Path {
        self.vault.catalog()
    }

    fn catalog_dir(&self) -> &Path {
        self.catalog().parent().expect("the catalog has a parent")
    }

    fn previews(&self) -> &Path {
        self.vault.previews()
    }

    fn out(&self) -> &Path {
        self.vault.out()
    }

    fn scan(&self) -> Command {
        let mut command = lrsalvage();
        command
            .arg("scan")
            .arg("--catalog")
            .arg(self.catalog())
            .arg("--previews")
            .arg(self.previews());
        command
    }

    fn extract(&self) -> Command {
        let mut command = lrsalvage();
        command
            .arg("extract")
            .arg("--catalog")
            .arg(self.catalog())
            .arg("--previews")
            .arg(self.previews())
            .arg("--out")
            .arg(self.out());
        command
    }
}

/// A `legacy` vault: one image, one `.lrprev` container, and no
/// `Adobe_AdditionalMetadata` — an old catalog that predates it must still be
/// salvageable.
fn build() -> Vault {
    Vault::legacy()
        .catalog_name("Test")
        .without_metadata_table()
        .image(ImageRow::new(1, "synthetic").file(100).cache(UUID))
        .pyramid(
            PyramidRow::new(UUID)
                .profile("sRGB")
                .quality("standard")
                .level(800, 600),
        )
        .level_file(Level::sized(UUID, 800, 600))
        .write()
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
    let entries: Vec<_> = fs::read_dir(fixture.out())
        .expect("read output dir")
        .collect();
    assert!(
        entries.is_empty(),
        "expected no files under {}, found {:?}",
        fixture.out().display(),
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
fn version_flag_reports_this_workspace_version() {
    // Clap's `version` attribute is this crate's `Cargo.toml` version, which the
    // workspace sets once for all three members. Holding it to
    // `CARGO_PKG_VERSION` is what makes a bump that reaches one surface and not
    // another fail `cargo test` instead of a release.
    let expected = format!("lrsalvage {}\n", env!("CARGO_PKG_VERSION"));
    for flag in ["--version", "-V"] {
        lrsalvage()
            .arg(flag)
            .assert()
            .code(0)
            .stdout(predicate::str::diff(expected.clone()));
    }
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
        .arg(fixture.previews())
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
        .arg(fixture.previews())
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
        .arg(fixture.catalog())
        .arg("--previews")
        .arg(fixture.previews())
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
    let inside = fixture.previews().join("refused");

    lrsalvage()
        .arg("extract")
        .arg("--catalog")
        .arg(fixture.catalog())
        .arg("--previews")
        .arg(fixture.previews())
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

/// The payload total the pre-flight compares: the plan's own number, built the
/// way a run builds it, so the tests below can set the destination's room to the
/// exact boundary that check uses.
fn plan_bytes(fixture: &Fixture) -> u64 {
    fixture.vault.plan(&Options::default()).stats.bytes
}

/// The name the seam's native backend answers in a debug build instead of asking
/// the platform (`specs/modules/fs.md`).
const FREE_SPACE: &str = "LRSALVAGE_FREE_SPACE_BYTES";

/// Whether that substitution exists in the build under test.
///
/// It is compiled into `debug_assertions` builds only, which is what `cargo test`
/// builds; a `cargo test --release` run would otherwise "fail" every test below
/// by measuring this machine's real, roomy disk. A skip says so rather than
/// pretending the refusal is broken.
fn free_space_substitution() -> bool {
    cfg!(debug_assertions)
}

/// The line a skipped test prints, so a release-profile run is legible.
const NO_SUBSTITUTION: &str =
    "skipped: LRSALVAGE_FREE_SPACE_BYTES is compiled into debug builds only";

/// #64: the refusal, proved against the real binary.
///
/// No portable unprivileged machine can be made nearly full on demand, so the
/// seam answers [`FREE_SPACE`] in a debug build — which is what `cargo test`
/// compiles. Everything else is the shipped path: the real plan's total, the
/// real destination, the real exit code.
#[test]
fn extract_refuses_a_destination_without_room_and_writes_nothing() {
    if !free_space_substitution() {
        eprintln!("{NO_SUBSTITUTION}");
        return;
    }
    let fixture = Fixture::new();
    let needed = plan_bytes(&fixture);
    let shortfall = needed / 2;

    fixture
        .extract()
        .env(FREE_SPACE, (needed - shortfall).to_string())
        .assert()
        .code(4)
        .stderr(predicate::str::contains(format!("{shortfall} bytes")))
        .stderr(predicate::str::contains("MiB"))
        .stderr(predicate::str::contains("short"))
        .stderr(predicate::str::contains("Nothing was written"));

    assert_empty_output(&fixture);
}

/// The comparison has no margin: one byte under the plan's total is refused and
/// the plan's total itself proceeds.
#[test]
fn one_byte_short_is_refused_and_the_plans_own_total_proceeds() {
    if !free_space_substitution() {
        eprintln!("{NO_SUBSTITUTION}");
        return;
    }
    let fixture = Fixture::new();
    let needed = plan_bytes(&fixture);

    fixture
        .extract()
        .env(FREE_SPACE, (needed - 1).to_string())
        .assert()
        .code(4)
        .stderr(predicate::str::contains("1 byte"))
        .stderr(predicate::str::contains("short"));
    assert_empty_output(&fixture);

    fixture
        .extract()
        .env(FREE_SPACE, needed.to_string())
        .assert()
        .code(0)
        // It fits, and leaves nothing behind it — which is said rather than
        // silently called a fit (decision 5).
        .stderr(predicate::str::contains("nearly full"));
    assert!(
        !jpeg_files(fixture.out()).is_empty(),
        "a destination with exactly enough room writes the previews"
    );
}

/// `--dry-run` asks the same question and reports the same verdict, and writes
/// nothing either way.
#[test]
fn a_dry_run_makes_the_same_room_check_and_writes_nothing() {
    if !free_space_substitution() {
        eprintln!("{NO_SUBSTITUTION}");
        return;
    }
    let fixture = Fixture::new();
    let needed = plan_bytes(&fixture);

    fixture
        .extract()
        .arg("--dry-run")
        .env(FREE_SPACE, (needed - 1).to_string())
        .assert()
        .code(4)
        .stderr(predicate::str::contains("short"));
    assert_empty_output(&fixture);

    fixture
        .extract()
        .arg("--dry-run")
        .env(FREE_SPACE, needed.to_string())
        .assert()
        .code(0)
        .stderr(predicate::str::contains("nearly full"))
        .stderr(predicate::str::contains("0 written (dry run)"));
    assert_empty_output(&fixture);
}

/// A resume is measured by what it has **left** to write, not by the plan's whole
/// total.
///
/// The first pass filling the destination is the case a resume exists for; if the
/// pre-flight counted the bytes already written, every later resume would be
/// refused on the drive it had just filled (#64, review round).
#[test]
fn a_resume_is_measured_by_the_bytes_it_has_left_to_write() {
    if !free_space_substitution() {
        eprintln!("{NO_SUBSTITUTION}");
        return;
    }
    let fixture = Fixture::new();
    let needed = plan_bytes(&fixture);
    assert!(needed > 0, "this fixture's plan writes something: {needed}");

    // The first pass writes everything, with room to spare.
    fixture.extract().assert().code(0);
    assert!(
        !jpeg_files(fixture.out()).is_empty(),
        "the first pass wrote"
    );

    // Every output is now there, so this run would write nothing — and a
    // destination with *no* free space therefore still fits it.
    fixture
        .extract()
        .env(FREE_SPACE, "0")
        .assert()
        .code(0)
        .stderr(predicate::str::contains("skipped"))
        // Nothing left to write is nothing to call nearly full.
        .stderr(predicate::str::contains("nearly full").not());
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

    let jpegs = jpeg_files(fixture.out());
    assert_eq!(
        jpegs.len(),
        1,
        "expected exactly one recovered JPEG, found {jpegs:?}"
    );
    assert_eq!(
        jpegs[0].strip_prefix(fixture.out()).unwrap(),
        Path::new("originals").join("synthetic.jpg"),
        "the output tree must mirror the catalog's root folder"
    );
    let bytes = fs::read(&jpegs[0]).expect("read recovered JPEG");
    assert!(
        bytes.ends_with(&[0xFF, 0xD9]),
        "a recovered JPEG must end with an EOI marker"
    );

    for name in ["report.csv", "manifest.json", "extract.csv"] {
        let path = fixture.out().join(name);
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
        jpeg_files(fixture.out()).len(),
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
