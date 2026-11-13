//! The only module that writes to disk.
//!
//! A destination is never clobbered on faith. An existing file is skipped only
//! when it verifies as one of our own outputs — a complete JPEG (SOI … EOI)
//! whose SOF dimensions match the item being extracted; anything else is left
//! byte-identical unless `--force` asked for a replacement.
//!
//! A write goes to a unique `.<stem>.<pid>-<n>.partial` beside the destination,
//! is flushed with `sync_all`, and is then renamed into place. There is no
//! delete-then-rename step: `rename` replaces an existing file on both Windows
//! and POSIX, so an interrupted run cannot destroy the file it was replacing.
//!
//! Nothing is written inside the catalog or the preview cache.
//! [`ensure_safe_destination_create`] refuses a destination before creating it.

use std::fs;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{Error, IoContext, Result};
use crate::jpeg;
use crate::lrprev::Lrprev;
use crate::model::Orientation;
use crate::plan::{Plan, PlanItem, Status};
use crate::{exif, report};

#[derive(Debug, Clone)]
pub struct ExtractOptions {
    pub output_root: PathBuf,
    pub dry_run: bool,
    pub write_metadata: bool,
    pub set_timestamps: bool,
    /// Write the EXIF Orientation tag. Off means "pixels only, no rotation
    /// information", for catalogs that already baked rotation in.
    pub write_orientation: bool,
    /// Replace an existing destination that does not verify as one of our own
    /// outputs. Without it, such a file is reported and left untouched.
    pub force: bool,
}

impl Default for ExtractOptions {
    fn default() -> Self {
        ExtractOptions {
            output_root: PathBuf::new(),
            dry_run: false,
            write_metadata: true,
            set_timestamps: true,
            write_orientation: true,
            force: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeStatus {
    Written,
    /// An earlier run already produced a valid file; left alone.
    Skipped,
    /// `--dry-run`: nothing was written.
    DryRun,
    Failed,
}

impl OutcomeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            OutcomeStatus::Written => "written",
            OutcomeStatus::Skipped => "skipped",
            OutcomeStatus::DryRun => "dry-run",
            OutcomeStatus::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub output: PathBuf,
    pub status: OutcomeStatus,
    pub bytes: u64,
    pub sha256: Option<String>,
    pub note: Option<String>,
}

/// Refuse destinations nested inside either the catalog folder or the cache.
///
/// Both roots must already exist; `output_root` is created by the caller first.
/// Prefer [`ensure_safe_destination_create`], which validates before creating
/// anything.
pub fn ensure_safe_destination(
    output_root: &Path,
    catalog_path: &Path,
    cache_root: &Path,
) -> Result<()> {
    reject_if_inside(
        canonical(output_root),
        output_root,
        catalog_path,
        cache_root,
    )
}

/// Validate a destination that may not exist yet, without creating anything.
///
/// The nearest existing ancestor is canonicalised and the remaining components
/// are applied lexically, so `--out catalog/../catalog/out` is refused while
/// the directory still does not exist.
pub fn ensure_safe_destination_pending(
    output_root: &Path,
    catalog_path: &Path,
    cache_root: &Path,
) -> Result<()> {
    reject_if_inside(
        resolve_pending(output_root),
        output_root,
        catalog_path,
        cache_root,
    )
}

/// Validate a destination, then create it.
///
/// Refusal happens before `create_dir_all`, so a rejected `--out` never leaves
/// a directory inside the catalog or the cache.
pub fn ensure_safe_destination_create(
    output_root: &Path,
    catalog_path: &Path,
    cache_root: &Path,
) -> Result<()> {
    ensure_safe_destination_pending(output_root, catalog_path, cache_root)?;
    create_destination(output_root)?;
    // A symlinked ancestor can only be judged once the path resolves on disk.
    ensure_safe_destination(output_root, catalog_path, cache_root)
}

/// Validate, then create, a destination that must stay outside one tree.
///
/// Used for the `.lrcat-data` mask store, which is read-only evidence too.
pub fn ensure_create_outside(output_root: &Path, forbidden_root: &Path) -> Result<()> {
    if is_inside(&resolve_pending(output_root), &canonical(forbidden_root)) {
        return Err(Error::UnsafeDestination(output_root.to_path_buf()));
    }
    create_destination(output_root)?;
    if is_inside(&canonical(output_root), &canonical(forbidden_root)) {
        return Err(Error::UnsafeDestination(output_root.to_path_buf()));
    }
    Ok(())
}

fn reject_if_inside(
    resolved: PathBuf,
    reported: &Path,
    catalog_path: &Path,
    cache_root: &Path,
) -> Result<()> {
    let cache = canonical(cache_root);
    if is_inside(&resolved, &cache) {
        return Err(Error::UnsafeDestination(reported.to_path_buf()));
    }
    if let Some(catalog_dir) = canonical(catalog_path).parent().map(Path::to_path_buf) {
        if is_inside(&resolved, &catalog_dir) {
            return Err(Error::UnsafeDestination(reported.to_path_buf()));
        }
    }
    Ok(())
}

fn is_inside(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
}

fn create_destination(output_root: &Path) -> Result<()> {
    fs::create_dir_all(output_root).map_err(|source| Error::Destination {
        path: output_root.to_path_buf(),
        source,
    })?;
    probe_writable(output_root)
}

/// Creating a directory is not the same as being allowed to write in it.
///
/// A probe file turns a read-only destination into a destination error up
/// front, instead of a run's worth of per-file failures. The probe is named
/// uniquely per process and removed immediately.
fn probe_writable(dir: &Path) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    for _ in 0..8 {
        let unique = format!(
            "{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let probe = dir.join(format!(".lrsalvage-write-probe.{unique}"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
        {
            Ok(_) => {
                let _ = fs::remove_file(&probe);
                return Ok(());
            }
            // A stale probe from a killed run with the same pid: try another.
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(Error::Destination {
                    path: dir.to_path_buf(),
                    source,
                });
            }
        }
    }
    Err(Error::Destination {
        path: dir.to_path_buf(),
        source: std::io::Error::other("could not create a unique write probe"),
    })
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Resolve `path` so it is comparable with the canonical evidence roots.
///
/// Relative paths are anchored at the working directory, the nearest existing
/// ancestor is canonicalised, and `.`/`..` in the remainder are folded in
/// lexically. No directory is created.
fn resolve_pending(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let Some(ancestor) = absolute.ancestors().find(|candidate| candidate.exists()) else {
        return canonical(&absolute);
    };
    let mut resolved = canonical(ancestor);
    let remainder = absolute.strip_prefix(ancestor).unwrap_or(&absolute);
    for component in remainder.components() {
        match component {
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(part) => resolved.push(part),
        }
    }
    resolved
}

/// Write every selected item. `progress` is called once per outcome.
pub fn extract(
    plan: &Plan,
    options: &ExtractOptions,
    progress: &mut dyn FnMut(&Outcome),
) -> Result<Vec<Outcome>> {
    let mut outcomes = Vec::new();

    for item in plan.selected() {
        let outcome = extract_one(item, options);
        progress(&outcome);
        outcomes.push(outcome);
    }

    Ok(outcomes)
}

/// Written when the destination holds something that does not verify as ours.
pub const DESTINATION_NOT_OURS: &str =
    "destination exists and is not a verified extraction; use --force to replace";

/// Is `bytes` one of our own past outputs for an item of `expected` size?
///
/// A resume must prove the whole file survived: end-of-image present, frame
/// parseable, and the frame the size the plan expects. A truncated file whose
/// SOF happens to be intact therefore fails.
fn verifies_as_ours(bytes: &[u8], expected: (u32, u32)) -> bool {
    jpeg::is_complete(bytes) && jpeg::dimensions(bytes).is_ok_and(|dims| dims == expected)
}

fn extract_one(item: &PlanItem, options: &ExtractOptions) -> Outcome {
    let (Some(payload_path), Some(output)) = (item.payload.as_ref(), item.output.as_ref()) else {
        return failed(
            item.output.clone().unwrap_or_default(),
            "plan item has no payload or no output path",
        );
    };

    let expected = (item.width.unwrap_or(0), item.height.unwrap_or(0));

    // Resume: an existing file is skipped only when it verifies as ours.
    // Anything else is somebody else's file until `--force` says otherwise.
    if output.exists() {
        if let Ok(existing) = fs::read(output) {
            if verifies_as_ours(&existing, expected) {
                return Outcome {
                    output: output.clone(),
                    status: OutcomeStatus::Skipped,
                    bytes: existing.len() as u64,
                    sha256: None,
                    note: Some("already recovered and verified".into()),
                };
            }
        }
        if !options.force {
            return failed(output.clone(), DESTINATION_NOT_OURS);
        }
    }

    let payload = match fs::read(payload_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            return failed(output.clone(), format!("unreadable payload: {e}"));
        }
    };

    let parsed = match Lrprev::parse(&payload) {
        Ok(p) => p,
        Err(e) => return failed(output.clone(), format!("not a usable .lrprev: {e}")),
    };

    let level = parsed
        .levels()
        .into_iter()
        .find(|chunk| jpeg::dimensions(chunk.data).is_ok_and(|dims| dims == expected))
        .or_else(|| parsed.best_level());

    let Some(level) = level else {
        return failed(
            output.clone(),
            "no level contains a readable JPEG".to_string(),
        );
    };

    if !jpeg::is_complete(level.data) {
        return failed(
            output.clone(),
            "level does not end with an EOI marker".to_string(),
        );
    }

    let mut bytes = level.data.to_vec();
    let mut notes: Vec<String> = Vec::new();
    if options.write_metadata {
        let xmp = item
            .image
            .as_ref()
            .and_then(|i| i.xmp.clone())
            .unwrap_or_default();
        let orientation = if options.write_orientation {
            item.orientation
        } else {
            Orientation::Normal
        };
        let meta = exif::from_catalog_with_profile(&xmp, orientation, item.color_profile.clone());
        if !meta.is_empty() {
            match exif::inject_full(&bytes, &meta) {
                Ok(injection) => {
                    bytes = injection.jpeg;
                    notes.extend(injection.notes);
                }
                Err(e) => return failed(output.clone(), format!("exif injection failed: {e}")),
            }
        }
    }

    if options.dry_run {
        return Outcome {
            output: output.clone(),
            status: OutcomeStatus::DryRun,
            bytes: bytes.len() as u64,
            sha256: None,
            note: Some(if notes.is_empty() { "dry run; nothing written".to_string() } else { format!("dry run; nothing written; {}", notes.join("; ")) }),
        };
    }

    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = fs::create_dir_all(parent) {
                return failed(output.clone(), format!("cannot create {parent:?}: {e}"));
            }
        }
    }

    // A unique temp name per write: two concurrent runs cannot rename each
    // other's file away, and a stale `.partial` from a killed run is never
    // reused. `sync_all` means a rename only ever exposes durable bytes.
    let temp = partial_sibling(output);
    let write = (|| -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()
    })();
    if let Err(e) = write {
        let _ = fs::remove_file(&temp);
        return failed(output.clone(), format!("cannot write {temp:?}: {e}"));
    }

    // `rename` replaces an existing file on Windows and POSIX; never remove
    // the old file first, or an interrupted run loses both copies.
    if let Err(e) = fs::rename(&temp, output) {
        let _ = fs::remove_file(&temp);
        return failed(output.clone(), format!("cannot finalise {output:?}: {e}"));
    }

    if options.set_timestamps {
        set_capture_time(output, item);
    }

    let digest = Sha256::digest(&bytes);
    let sha256 = digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();

    Outcome {
        output: output.clone(),
        status: OutcomeStatus::Written,
        bytes: bytes.len() as u64,
        sha256: Some(sha256),
        note: if notes.is_empty() {
            None
        } else {
            Some(notes.join("; "))
        },
    }
}

/// `.<stem>.<pid>-<counter>.partial` beside the destination.
fn partial_sibling(output: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let stem = output
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    let unique = format!(
        "{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    output.with_file_name(format!(".{stem}.{unique}.partial"))
}

/// One line describing a finished run, e.g. `78 evaluated, 0 written (dry run)`.
pub fn outcome_summary(outcomes: &[Outcome], dry_run: bool) -> String {
    let evaluated = outcomes.len();
    if dry_run {
        return format!("{evaluated} evaluated, 0 written (dry run)");
    }
    let count = |status: OutcomeStatus| outcomes.iter().filter(|o| o.status == status).count();
    let bytes: u64 = outcomes
        .iter()
        .filter(|o| o.status == OutcomeStatus::Written)
        .map(|o| o.bytes)
        .sum();
    format!(
        "{evaluated} evaluated, {} written, {} skipped, {} failed, {:.1} MiB",
        count(OutcomeStatus::Written),
        count(OutcomeStatus::Skipped),
        count(OutcomeStatus::Failed),
        bytes as f64 / 1_048_576.0
    )
}

/// Set the file's modification time to the capture time, when known.
///
/// The catalog stores local wall-clock time with no zone, so it is
/// interpreted in the machine's local zone. Failure is not fatal: a wrong
/// timestamp must never cost a recovered photo.
fn set_capture_time(output: &Path, item: &PlanItem) {
    use chrono::{Local, NaiveDateTime, TimeZone};

    let Some(raw) = item
        .image
        .as_ref()
        .and_then(|i| i.xmp.as_ref())
        .and_then(|x| x.capture_time.as_deref())
    else {
        return;
    };
    let trimmed = raw.get(..19).unwrap_or(raw);
    let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S") else {
        return;
    };
    let Some(local) = Local.from_local_datetime(&naive).single() else {
        return;
    };
    let ft = filetime::FileTime::from_unix_time(local.timestamp(), 0);
    let _ = filetime::set_file_mtime(output, ft);
}

/// `report.csv` next to the extracted files, so a run is auditable.
pub fn write_outcomes_csv(outcomes: &[Outcome], path: &Path) -> Result<()> {
    use std::io::Write;

    let mut text = String::with_capacity(outcomes.len() * 96);
    text.push_str("status,bytes,sha256,output,note\r\n");
    for o in outcomes {
        text.push_str(o.status.as_str());
        text.push(',');
        text.push_str(&o.bytes.to_string());
        text.push(',');
        text.push_str(o.sha256.as_deref().unwrap_or(""));
        text.push(',');
        text.push_str(&report::escape_csv(
            &o.output.to_string_lossy().replace('\\', "/"),
        ));
        text.push(',');
        text.push_str(&report::escape_csv(o.note.as_deref().unwrap_or("")));
        text.push_str("\r\n");
    }
    let mut file = fs::File::create(path).at(path)?;
    file.write_all(text.as_bytes()).at(path)?;
    file.flush().at(path)?;
    Ok(())
}

fn failed(output: PathBuf, note: impl Into<String>) -> Outcome {
    Outcome {
        output,
        status: OutcomeStatus::Failed,
        bytes: 0,
        sha256: None,
        note: Some(note.into()),
    }
}

/// Items a plan considers recoverable but that the floor excluded.
pub fn below_floor(plan: &Plan) -> usize {
    plan.items
        .iter()
        .filter(|i| i.status == Status::Recoverable && !i.selected)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Stats;

    /// One `AgHg` chunk, as the container writer would emit it.
    fn chunk(name: &str, payload: &[u8]) -> Vec<u8> {
        let header_len = 24 + name.len();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"AgHg");
        bytes.extend_from_slice(&u16::try_from(header_len).unwrap().to_be_bytes());
        bytes.push(0); // version
        bytes.push(0); // kind
        bytes.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        bytes.extend_from_slice(&0u64.to_be_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    /// A structurally valid JPEG at `width` x `height`.
    fn jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8];
        bytes.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00]);
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

    /// A payload container holding one level at the given size.
    fn payload_bytes(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = chunk("header", b"pyramid = { }");
        bytes.extend(chunk("level_1", &jpeg(width, height)));
        bytes
    }

    /// A temp directory, one payload, and a plan that would extract it.
    struct Fixture {
        dir: tempfile::TempDir,
        plan: Plan,
        output: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("temp dir");
            let payload = dir.path().join("payload.lrprev");
            fs::write(&payload, payload_bytes(1600, 1200)).expect("write payload");
            let output = dir.path().join("out").join("recovered.jpg");
            let plan = Plan {
                items: vec![PlanItem {
                    status: Status::Recoverable,
                    image: None,
                    payload: Some(payload),
                    output: Some(output.clone()),
                    width: Some(1600),
                    height: Some(1200),
                    color_profile: None,
                    quality: None,
                    orientation: Orientation::Normal,
                    tier: None,
                    declared: None,
                    selected: true,
                    note: None,
                }],
                stats: Stats::default(),
            };
            Fixture { dir, plan, output }
        }

        fn options(&self, force: bool) -> ExtractOptions {
            ExtractOptions {
                output_root: self.dir.path().join("out"),
                dry_run: false,
                write_metadata: true,
                set_timestamps: false,
                write_orientation: true,
                force,
            }
        }

        fn run(&self, force: bool) -> Vec<Outcome> {
            extract(&self.plan, &self.options(force), &mut |_| {}).expect("extract")
        }
    }

    #[test]
    fn a_preexisting_unverified_file_is_reported_failed_and_left_untouched() {
        let fixture = Fixture::new();
        let foreign = b"a file this tool did not write";
        fs::create_dir_all(fixture.output.parent().unwrap()).expect("create output dir");
        fs::write(&fixture.output, foreign).expect("pre-existing file");

        let outcomes = fixture.run(false);

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].status, OutcomeStatus::Failed);
        assert_eq!(
            outcomes[0].note.as_deref(),
            Some("destination exists and is not a verified extraction; use --force to replace")
        );
        assert_eq!(
            fs::read(&fixture.output).expect("read the foreign file"),
            foreign,
            "a file we cannot prove we wrote must be left byte-identical"
        );
    }

    #[test]
    fn force_replaces_an_unverified_file_and_leaves_no_partial_behind() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.output.parent().unwrap()).expect("create output dir");
        fs::write(&fixture.output, b"stale").expect("pre-existing file");

        let outcomes = fixture.run(true);

        assert_eq!(outcomes[0].status, OutcomeStatus::Written);
        let written = fs::read(&fixture.output).expect("read the replacement");
        assert!(
            jpeg::is_complete(&written),
            "output must be a complete JPEG"
        );
        assert_eq!(jpeg::dimensions(&written).unwrap(), (1600, 1200));
        assert!(
            partials(fixture.dir.path()).is_empty(),
            "{:?}",
            partials(fixture.dir.path())
        );
    }

    #[test]
    fn a_preexisting_verified_file_is_skipped_and_its_bytes_untouched() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.output.parent().unwrap()).expect("create output dir");
        // Same dimensions, different bytes: verification is about the file
        // being one of ours, not about matching a payload byte for byte.
        let ours = jpeg(1600, 1200);
        fs::write(&fixture.output, &ours).expect("pre-existing extraction");

        let outcomes = fixture.run(true);

        assert_eq!(outcomes[0].status, OutcomeStatus::Skipped);
        assert!(outcomes[0].sha256.is_none(), "a skip has no digest");
        assert_eq!(fs::read(&fixture.output).unwrap(), ours);
    }

    #[test]
    fn a_truncated_file_whose_sof_survives_is_not_accepted() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.output.parent().unwrap()).expect("create output dir");
        let complete = jpeg(1600, 1200);
        let truncated = &complete[..complete.len() - 4];
        // Premise: this is exactly the resume bug — the frame is readable but
        // the image is cut off before its end-of-image marker.
        assert_eq!(jpeg::dimensions(truncated).unwrap(), (1600, 1200));
        assert!(!jpeg::is_complete(truncated));
        fs::write(&fixture.output, truncated).expect("pre-existing truncation");

        let outcomes = fixture.run(false);
        assert_eq!(outcomes[0].status, OutcomeStatus::Failed);
        assert_eq!(outcomes[0].note.as_deref(), Some(DESTINATION_NOT_OURS));
        assert_eq!(
            fs::read(&fixture.output).unwrap(),
            truncated,
            "a truncated file is not ours to delete"
        );

        // With --force the file is re-extracted, and the temp part is gone.
        let forced = fixture.run(true);
        assert_eq!(forced[0].status, OutcomeStatus::Written);
        let written = fs::read(&fixture.output).unwrap();
        assert!(jpeg::is_complete(&written));
        assert_eq!(jpeg::dimensions(&written).unwrap(), (1600, 1200));
        assert!(
            partials(fixture.dir.path()).is_empty(),
            "{:?}",
            partials(fixture.dir.path())
        );
    }

    #[test]
    fn a_complete_jpeg_of_the_wrong_size_is_not_accepted() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.output.parent().unwrap()).expect("create output dir");
        let other_photo = jpeg(800, 600);
        fs::write(&fixture.output, &other_photo).expect("a different picture");

        let outcomes = fixture.run(false);

        assert_eq!(outcomes[0].status, OutcomeStatus::Failed);
        assert_eq!(outcomes[0].note.as_deref(), Some(DESTINATION_NOT_OURS));
        assert_eq!(fs::read(&fixture.output).unwrap(), other_photo);
    }

    #[test]
    fn a_successful_write_leaves_no_partial_behind() {
        let fixture = Fixture::new();

        let outcomes = fixture.run(false);

        assert_eq!(outcomes[0].status, OutcomeStatus::Written);
        assert!(fixture.output.exists());
        assert!(
            partials(fixture.dir.path()).is_empty(),
            "{:?}",
            partials(fixture.dir.path())
        );
        // The reported digest is the digest of the bytes on disk, not of some
        // intermediate buffer that the writer might have altered.
        let on_disk = fs::read(&fixture.output).unwrap();
        let expected = Sha256::digest(&on_disk)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(outcomes[0].sha256.as_deref(), Some(expected.as_str()));
        assert_eq!(outcomes[0].bytes, on_disk.len() as u64);
    }

    #[test]
    fn dry_run_reports_evaluations_and_leaves_the_directory_empty() {
        let fixture = Fixture::new();
        let output_root = fixture.dir.path().join("out");
        fs::create_dir_all(&output_root).expect("create output dir");
        let mut options = fixture.options(false);
        options.dry_run = true;

        let outcomes = extract(&fixture.plan, &options, &mut |_| {}).expect("dry run");

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].status, OutcomeStatus::DryRun);
        assert_eq!(
            outcome_summary(&outcomes, true),
            "1 evaluated, 0 written (dry run)"
        );
        assert!(
            std::fs::read_dir(&output_root).unwrap().next().is_none(),
            "a dry run must not create a single entry"
        );
    }

    #[test]
    fn the_summary_counts_evaluations_separately_from_writes() {
        // The worked example from the review: 78 items evaluated, none written.
        let dry: Vec<Outcome> = (0..78)
            .map(|index| Outcome {
                output: PathBuf::from(format!("out/{index}.jpg")),
                status: OutcomeStatus::DryRun,
                bytes: 1234,
                sha256: None,
                note: None,
            })
            .collect();
        assert_eq!(
            outcome_summary(&dry, true),
            "78 evaluated, 0 written (dry run)"
        );

        let real = vec![
            Outcome {
                output: PathBuf::from("out/a.jpg"),
                status: OutcomeStatus::Written,
                bytes: 1_048_576,
                sha256: Some("ab".repeat(32)),
                note: None,
            },
            Outcome {
                output: PathBuf::from("out/b.jpg"),
                status: OutcomeStatus::Skipped,
                bytes: 0,
                sha256: None,
                note: None,
            },
            Outcome {
                output: PathBuf::from("out/c.jpg"),
                status: OutcomeStatus::Failed,
                bytes: 0,
                sha256: None,
                note: Some(DESTINATION_NOT_OURS.into()),
            },
        ];
        let text = outcome_summary(&real, false);
        assert!(
            text.starts_with("3 evaluated, 1 written, 1 skipped, 1 failed"),
            "{text}"
        );
        assert!(text.contains("1.0 MiB"), "{text}");
    }

    #[test]
    fn creating_a_destination_inside_the_evidence_refuses_before_creating_anything() {
        let dir = tempfile::tempdir().expect("temp dir");
        let catalog_dir = dir.path().join("catalog");
        fs::create_dir_all(&catalog_dir).expect("catalog dir");
        let catalog = catalog_dir.join("2023.lrcat");
        fs::write(&catalog, b"fixture").expect("catalog file");
        let cache = dir.path().join("2023 Previews.lrdata");
        fs::create_dir_all(&cache).expect("cache dir");

        let in_catalog = catalog_dir.join("rescued").join("nested");
        match ensure_safe_destination_create(&in_catalog, &catalog, &cache) {
            Err(Error::UnsafeDestination(path)) => assert_eq!(path, in_catalog),
            other => panic!("inside the catalog must be refused, got {other:?}"),
        }
        assert!(!in_catalog.exists(), "the destination must not be created");
        assert!(
            !catalog_dir.join("rescued").exists(),
            "no ancestor of the destination may be created either"
        );

        let in_cache = cache.join("rescued");
        assert!(matches!(
            ensure_safe_destination_create(&in_cache, &catalog, &cache),
            Err(Error::UnsafeDestination(_))
        ));
        assert!(!in_cache.exists());

        // A sibling of both trees is what an output root should be.
        let sibling = dir.path().join("rescued");
        ensure_safe_destination_create(&sibling, &catalog, &cache).expect("safe destination");
        assert!(sibling.is_dir());
    }

    #[test]
    fn a_parent_component_cannot_slip_a_destination_into_the_catalog() {
        let dir = tempfile::tempdir().expect("temp dir");
        let catalog_dir = dir.path().join("catalog");
        let elsewhere = dir.path().join("elsewhere");
        fs::create_dir_all(&catalog_dir).expect("catalog dir");
        fs::create_dir_all(&elsewhere).expect("sibling dir");
        let catalog = catalog_dir.join("2023.lrcat");
        fs::write(&catalog, b"fixture").expect("catalog file");
        let cache = dir.path().join("2023 Previews.lrdata");
        fs::create_dir_all(&cache).expect("cache dir");

        // Looks friendly, resolves inside the catalog.
        let out = elsewhere.join("..").join("catalog").join("rescued");
        assert!(matches!(
            ensure_safe_destination_create(&out, &catalog, &cache),
            Err(Error::UnsafeDestination(_))
        ));
        assert!(!catalog_dir.join("rescued").exists());
    }

    #[test]
    fn a_destination_that_cannot_be_created_is_a_destination_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let catalog_dir = dir.path().join("catalog");
        fs::create_dir_all(&catalog_dir).expect("catalog dir");
        let catalog = catalog_dir.join("2023.lrcat");
        fs::write(&catalog, b"fixture").expect("catalog file");
        let cache = dir.path().join("2023 Previews.lrdata");
        fs::create_dir_all(&cache).expect("cache dir");

        // A file where an ancestor directory would have to be.
        let blocker = dir.path().join("blocker");
        fs::write(&blocker, b"not a directory").expect("blocking file");
        let out = blocker.join("rescued");
        assert!(
            matches!(
                ensure_safe_destination_create(&out, &catalog, &cache),
                Err(Error::Destination { .. })
            ),
            "a destination that cannot be created must be a destination error, not a panic"
        );
        assert!(!out.exists());
    }

    /// A read-only destination is exit 4 before a single item is evaluated.
    #[cfg(unix)]
    #[test]
    fn a_read_only_destination_is_a_destination_error() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().expect("temp dir");
        let catalog_dir = dir.path().join("catalog");
        fs::create_dir_all(&catalog_dir).expect("catalog dir");
        let catalog = catalog_dir.join("2023.lrcat");
        fs::write(&catalog, b"fixture").expect("catalog file");
        let cache = dir.path().join("2023 Previews.lrdata");
        fs::create_dir_all(&cache).expect("cache dir");

        let out = dir.path().join("rescued");
        fs::create_dir_all(&out).expect("output dir");
        fs::set_permissions(&out, fs::Permissions::from_mode(0o555)).expect("chmod read-only");

        let result = ensure_safe_destination_create(&out, &catalog, &cache);
        fs::set_permissions(&out, fs::Permissions::from_mode(0o755)).expect("restore chmod");
        assert!(
            matches!(result, Err(Error::Destination { .. })),
            "an unwritable destination must be refused up front: {result:?}"
        );
    }

    /// Every regular file below `root` whose name marks it as a temp part.
    fn partials(root: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries {
                let path = entry.expect("readable entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else if path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(".partial"))
                {
                    found.push(path);
                }
            }
        }
        found
    }
}
