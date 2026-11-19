//! The only module that writes to disk.
//!
//! A destination is never clobbered on faith. An existing file is skipped only
//! when it verifies as one of our own outputs: its `Exif` `Software` marker says
//! `lrsalvage <version>`, it is a complete JPEG (SOI … EOI), and its SOF
//! dimensions match the item being extracted. Anything else is left
//! byte-identical unless `--force` asked for a replacement, so a same-size
//! foreign JPEG is never mistaken for ours.
//!
//! A write is staged and renamed into place by [`crate::fs::write_atomic`]: a
//! unique sibling, flushed to the device, then renamed over the destination.
//! There is no delete-then-rename step: `rename` replaces an existing file on
//! both Windows and POSIX, so an interrupted run cannot destroy the file it was
//! replacing.
//!
//! Nothing is written inside the catalog or the preview cache.
//! [`ensure_safe_destination_create`] refuses a destination before creating it.

use std::path::{Component, Path, PathBuf};

use jiff::tz::TimeZone;
use sha2::{Digest, Sha256};

use crate::error::{Error, IoContext, Result};
use crate::jpeg;
use crate::lrprev;
use crate::model::Orientation;
use crate::plan::{Payload, Plan, PlanItem, Status};
use crate::{exif, fs, report};

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
    if let Some(catalog_dir) = canonical(catalog_path).parent().map(Path::to_path_buf)
        && is_inside(&resolved, &catalog_dir)
    {
        return Err(Error::UnsafeDestination(reported.to_path_buf()));
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
        match fs::create_new(&probe) {
            Ok(()) => {
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
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
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
        fs::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let Some(ancestor) = absolute
        .ancestors()
        .find(|candidate| fs::exists(candidate).unwrap_or(false))
    else {
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

/// The headroom a run that fits must still leave behind it before the
/// destination is called *nearly* full: one gibibyte.
///
/// A statement, not a margin. The comparison below it is exact — a destination
/// holding precisely the plan's total proceeds — so this can never refuse a run
/// the ticket's decision 5 lets through. The size is the room left for the run's
/// own `report.csv` and `manifest.json`, and for whatever else writes there; one
/// constant, one home.
const NEARLY_FULL_HEADROOM: u64 = 1 << 30;

/// What a destination has room for, as the pre-flight measured it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Room {
    /// The payload bytes this run would write: `plan.stats.bytes`.
    pub needed: u64,
    /// What the destination's filesystem reports as free right now.
    pub available: u64,
}

impl Room {
    /// The bytes this run would leave free.
    pub fn left(&self) -> u64 {
        self.available.saturating_sub(self.needed)
    }

    /// Whether the run fits, and leaves under [`NEARLY_FULL_HEADROOM`] behind it.
    pub fn is_nearly_full(&self) -> bool {
        self.left() < NEARLY_FULL_HEADROOM
    }

    /// What a run says about a destination that fits but barely, `None` when it
    /// leaves room behind.
    ///
    /// Said rather than computed twice: the verdict and the sentence about it
    /// are the same measurement, so the sentence prints in a dry run and in a
    /// real one alike.
    pub fn nearly_full_note(&self, destination: &Path) -> Option<String> {
        self.is_nearly_full().then(|| {
            format!(
                "the destination {} would be nearly full after this run: {} of previews needed, \
                 {} free now, {} left",
                destination.display(),
                report::mebibytes(self.needed),
                report::mebibytes(self.available),
                report::mebibytes(self.left())
            )
        })
    }
}

/// What the pre-flight could say about a destination.
#[derive(Debug, Clone)]
pub enum Preflight {
    /// The destination answered, and the run fits.
    Fits(Room),
    /// Nothing could be measured, so nothing was compared. The run proceeds with
    /// a note: this check is a guard, not the run's premise, and a filesystem
    /// that cannot answer must not make a destination unextractable.
    Unmeasured(String),
}

/// Refuse a destination the plan would not fit into, before the first write.
///
/// `needed` is the plan's own total — `plan.stats.bytes`, the payload bytes this
/// run would write at these options — so the bounds a caller applied move it and
/// there is no second measurement to disagree with the plan.
///
/// The destination may not exist yet: a dry run creates nothing, so the question
/// is asked of the nearest existing ancestor, which is the filesystem the
/// directory would land on when it is created.
///
/// The comparison is exact, with no margin: `available == needed` proceeds. It is
/// the payload total rather than the finished file sizes — the metadata segments
/// this module adds are not counted — so a destination with exactly this much
/// room still lets a later file fail on space, as it already does (exit `2`).
pub fn ensure_room(destination: &Path, needed: u64) -> Result<Preflight> {
    let Some(measured) = existing_ancestor(destination) else {
        return Ok(Preflight::Unmeasured(
            "no directory on the way to it exists".to_string(),
        ));
    };
    let available = match fs::available_space(&measured) {
        Ok(available) => available,
        Err(error) => return Ok(Preflight::Unmeasured(error.to_string())),
    };
    if available < needed {
        let shortfall = needed - available;
        // `Error::Destination` is the crate's existing "this destination cannot
        // be used" refusal, so the shortfall reuses exit `4` and `error` gains
        // no presentation-only variant (ADR 0010's rule: no sixth code).
        return Err(Error::Destination {
            path: destination.to_path_buf(),
            source: std::io::Error::other(format!(
                "this run's previews need {} ({}) and the destination has {} ({}) free — \
                 {} ({}) short. Nothing was written.",
                byte_count(needed),
                report::mebibytes(needed),
                byte_count(available),
                report::mebibytes(available),
                byte_count(shortfall),
                report::mebibytes(shortfall),
            )),
        });
    }
    Ok(Preflight::Fits(Room { needed, available }))
}

/// The nearest ancestor of `path` that exists, made absolute so a relative
/// destination has one too.
fn existing_ancestor(path: &Path) -> Option<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        fs::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
    };
    absolute
        .ancestors()
        .find(|candidate| fs::exists(candidate).unwrap_or(false))
        .map(Path::to_path_buf)
}

/// `1 byte`, `12 bytes`: the refusal is read by a person, and a bare count would
/// read `1 bytes`.
fn byte_count(count: u64) -> String {
    if count == 1 {
        "1 byte".to_string()
    } else {
        format!("{count} bytes")
    }
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
/// A resume must prove authorship, not plausibility: the `Software` marker
/// `exif` writes is present, the whole file survived (`FF D9` at EOF), and the
/// frame is the size the plan expects. A same-size foreign JPEG fails, and so
/// does an output written before the marker shipped (§5, `adr-0003`).
fn verifies_as_ours(bytes: &[u8], expected: (u32, u32)) -> bool {
    let Ok(certified) = jpeg::certify(bytes) else {
        return false;
    };
    certified.complete
        && certified.dimensions == expected
        && exif::software(bytes, &certified.segments)
            .is_some_and(|software| software.starts_with(exif::SOFTWARE_PREFIX))
}

fn extract_one(item: &PlanItem, options: &ExtractOptions) -> Outcome {
    let (Some(payload), Some(output)) = (item.payload.as_ref(), item.output.as_ref()) else {
        return failed(
            item.output.clone().unwrap_or_default(),
            "plan item has no payload or no output path",
        );
    };

    let expected = (item.width.unwrap_or(0), item.height.unwrap_or(0));

    // Resume: an existing file is skipped only when it verifies as ours.
    // Anything else is somebody else's file until `--force` says otherwise.
    // A metadata-free run verifies nothing at all: its own output has nowhere to
    // record identity, and a metadata-bearing file must not be silently kept by
    // a run that promised the payload's bytes (`adr-0003` §6).
    if fs::exists(output).unwrap_or(false) {
        if options.write_metadata
            && let Ok(existing) = fs::read(output)
            && verifies_as_ours(&existing, expected)
        {
            return Outcome {
                output: output.clone(),
                status: OutcomeStatus::Skipped,
                bytes: existing.len() as u64,
                sha256: None,
                note: Some("already recovered and verified".into()),
            };
        }
        if !options.force {
            return failed(output.clone(), DESTINATION_NOT_OURS);
        }
    }

    let file = match fs::read(payload.path()) {
        Ok(bytes) => bytes,
        Err(e) => {
            return failed(output.clone(), format!("unreadable payload: {e}"));
        }
    };

    // The plan's choice is the choice: a level file *is* the level, and a
    // container contributes the one chunk that was certified — never "the chunk
    // whose dimensions happen to match", and never the largest as a fallback.
    let level = match payload {
        Payload::LevelFile { .. } => file.as_slice(),
        Payload::Container {
            level: Some(name), ..
        } => match lrprev::chunk_data(&file, name) {
            Ok(Some(data)) => data,
            Ok(None) => {
                return failed(
                    output.clone(),
                    format!("the payload no longer carries the certified `{name}` chunk"),
                );
            }
            Err(e) => return failed(output.clone(), format!("not a usable .lrprev: {e}")),
        },
        Payload::Container { level: None, .. } => {
            return failed(
                output.clone(),
                "plan item names no certified level to write".to_string(),
            );
        }
    };

    // Re-certify the bytes actually read against the item's dimensions, so a
    // cache edited between `scan` and `extract` fails loudly instead of writing
    // something no plan ever approved.
    match jpeg::certify(level) {
        Ok(certified) if certified.complete && certified.dimensions == expected => {}
        Ok(certified) if !certified.complete => {
            return failed(
                output.clone(),
                "level does not end with an EOI marker".to_string(),
            );
        }
        Ok(certified) => {
            let (width, height) = certified.dimensions;
            let (expected_width, expected_height) = expected;
            return failed(
                output.clone(),
                format!(
                    "payload is {width}x{height} but the plan certified \
                     {expected_width}x{expected_height}"
                ),
            );
        }
        Err(e) => return failed(output.clone(), format!("not a JPEG preview: {e}")),
    }

    let mut bytes = level.to_vec();
    let mut notes: Vec<String> = Vec::new();
    if options.write_metadata {
        let xmp = item
            .image
            .as_ref()
            .and_then(|i| i.xmp.clone())
            .unwrap_or_default();
        let orientation = if options.write_orientation {
            item.effective_orientation
        } else {
            Orientation::Normal
        };
        // The workspace version: the same string `manifest.json` records, so a
        // file's marker and the run that wrote it agree. The marker is what makes
        // this input never empty, so metadata is written whenever it is asked
        // for — a segment always exists, if only to say who wrote the file.
        let meta = exif::from_catalog_with_profile(&xmp, orientation, item.color_profile.clone())
            .with_marker(env!("CARGO_PKG_VERSION"));
        match exif::inject_full(&bytes, &meta) {
            Ok(injection) => {
                bytes = injection.jpeg;
                notes.extend(injection.notes);
            }
            Err(e) => return failed(output.clone(), format!("exif injection failed: {e}")),
        }
    }

    if options.dry_run {
        return Outcome {
            output: output.clone(),
            status: OutcomeStatus::DryRun,
            bytes: bytes.len() as u64,
            sha256: None,
            note: Some(if notes.is_empty() {
                "dry run; nothing written".to_string()
            } else {
                format!("dry run; nothing written; {}", notes.join("; "))
            }),
        };
    }

    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = fs::create_dir_all(parent)
    {
        return failed(output.clone(), format!("cannot create {parent:?}: {e}"));
    }

    // Staged under a unique name, flushed, then renamed into place, so a reader
    // sees the file that was there or the whole new one and never a mixture.
    if let Err(e) = fs::write_atomic(output, &bytes) {
        return failed(output.clone(), format!("cannot write {output:?}: {e}"));
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

/// The modification time a capture-time string asks for, or `None` when it asks
/// for nothing safe to write.
///
/// The catalog stores local wall-clock time with no zone, so `zone` is what
/// turns it into an instant. `None` is the whole refusal: a value that does not
/// parse, or one that `zone` makes ambiguous — a DST fold reads the same wall
/// clock twice, and a spring-forward gap reads it never — because picking one of
/// the two instants is a guess (`adr-0006-degrade-without-guessing`). The
/// decision is separate from the write below so a test can hold a fold to it
/// without depending on the zone this machine happens to be in.
fn capture_time_mtime(raw: &str, zone: &TimeZone) -> Option<i64> {
    // `catalog` has already reduced the value to `YYYY-MM-DDTHH:MM:SS`
    // (`catalog::normalise_timestamp`, `specs/modules/catalog.md`); the slice is
    // defence against a caller handing over more, not a shape expected here.
    let trimmed = raw.get(..19).unwrap_or(raw);
    // Spelled out rather than the loose ISO parser, which reads a bare date as
    // midnight: a time of day the catalog did not record must not be invented.
    let civil = jiff::civil::DateTime::strptime("%Y-%m-%dT%H:%M:%S", trimmed).ok()?;
    let zoned = zone.to_ambiguous_zoned(civil).unambiguous().ok()?;
    Some(zoned.timestamp().as_second())
}

/// Set the file's modification time to the capture time, when known.
///
/// The catalog's wall clock is interpreted in the machine's local zone. Failure
/// is not fatal: a wrong timestamp must never cost a recovered photo.
fn set_capture_time(output: &Path, item: &PlanItem) {
    let Some(raw) = item
        .image
        .as_ref()
        .and_then(|i| i.xmp.as_ref())
        .and_then(|x| x.capture_time.as_deref())
    else {
        return;
    };
    apply_capture_time(output, raw, &TimeZone::system());
}

/// Write `raw` as `output`'s mtime in `zone`, or leave the mtime alone.
///
/// Every refusal — an unparseable value, an ambiguous local time — comes back as
/// a no-op rather than an error, and the zone is a parameter so a test can put a
/// fold in front of it without this machine's zone.
fn apply_capture_time(output: &Path, raw: &str, zone: &TimeZone) {
    let Some(mtime) = capture_time_mtime(raw, zone) else {
        return;
    };
    let _ = fs::set_mtime(output, mtime);
}

/// `report.csv` next to the extracted files, so a run is auditable.
pub fn write_outcomes_csv(outcomes: &[Outcome], path: &Path) -> Result<()> {
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
    fs::write(path, text.as_bytes()).at(path)?;
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
    // Observe the disk directly, as a test always has: checking the seam's work
    // with the seam would prove nothing.
    use std::fs;

    use crate::model::{CatalogImage, XmpMetadata};
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
                    payload: Some(Payload::Container {
                        path: payload,
                        level: Some("level_1".to_string()),
                    }),
                    output: Some(output.clone()),
                    width: Some(1600),
                    height: Some(1200),
                    color_profile: None,
                    quality: None,
                    orientation: Orientation::Normal,
                    effective_orientation: Orientation::Normal,
                    payload_metadata: exif::PayloadMetadata::default(),
                    tier: None,
                    declared: None,
                    smallest_level: None,
                    selected: true,
                    note: None,
                }],
                stats: Stats::default(),
                ..Plan::default()
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
    fn a_same_size_foreign_jpeg_is_not_ours() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.output.parent().unwrap()).expect("create output dir");
        // Complete, right size, and written by somebody else: without the marker
        // the old rule kept it, which is the bug the marker closes.
        let foreign = jpeg(1600, 1200);
        fs::write(&fixture.output, &foreign).expect("a foreign picture");

        let outcomes = fixture.run(false);

        assert_eq!(outcomes[0].status, OutcomeStatus::Failed);
        assert_eq!(outcomes[0].note.as_deref(), Some(DESTINATION_NOT_OURS));
        assert_eq!(
            fs::read(&fixture.output).unwrap(),
            foreign,
            "somebody else's same-size file is left byte-identical"
        );
    }

    #[test]
    fn a_metadata_free_run_never_verifies_anything() {
        let fixture = Fixture::new();
        let mut options = fixture.options(false);
        options.write_metadata = false;

        let first = extract(&fixture.plan, &options, &mut |_| {}).expect("first extract");
        assert_eq!(first[0].status, OutcomeStatus::Written);
        let written = fs::read(&fixture.output).expect("read the output");
        assert_eq!(
            written,
            jpeg(1600, 1200),
            "`--no-metadata` writes the payload's own bytes"
        );

        // A metadata run must not silently keep it: there is no marker to prove
        // authorship, so the destination is reported and left alone.
        let second = fixture.run(false);
        assert_eq!(second[0].status, OutcomeStatus::Failed);
        assert_eq!(second[0].note.as_deref(), Some(DESTINATION_NOT_OURS));
        assert_eq!(fs::read(&fixture.output).unwrap(), written);
    }

    #[test]
    fn a_written_output_carries_the_marker_and_the_effective_orientation() {
        let mut fixture = Fixture::new();
        // The catalog claims an upright image; the payload states otherwise.
        let item = &mut fixture.plan.items[0];
        item.orientation = Orientation::Normal;
        item.effective_orientation = Orientation::Rotate90;

        let outcomes = fixture.run(false);
        assert_eq!(outcomes[0].status, OutcomeStatus::Written);

        let written = fs::read(&fixture.output).expect("read the output");
        let certified = jpeg::certify(&written).expect("a complete JPEG");
        assert!(certified.complete);
        assert!(
            exif::software(&written, &certified.segments)
                .is_some_and(|software| software.starts_with(exif::SOFTWARE_PREFIX)),
            "every output names itself"
        );
        assert_eq!(
            exif::orientation(&written, &certified.segments),
            Some(Orientation::Rotate90),
            "the value written is the effective orientation, not the catalog's"
        );
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
    fn an_ambiguous_local_capture_time_drops_the_mtime_instead_of_guessing() {
        let zone = est5edt();

        // The fold: 01:30 on 3 November reads twice, at -04:00 and at -05:00.
        // Either instant is a guess about which one the photographer meant, and
        // a wrong mtime is worth less than no mtime.
        assert_eq!(capture_time_mtime("2024-11-03T01:30:00", &zone), None);
        // The gap is the same refusal the other way round: 02:30 never happened.
        assert_eq!(capture_time_mtime("2024-03-10T02:30:00", &zone), None);

        // A nearby wall clock that is *not* ambiguous is resolved, so what the
        // two refusals above rest on is the ambiguity and not the parse.
        assert_eq!(
            capture_time_mtime("2024-11-03T03:30:00", &zone),
            Some(1_730_622_600)
        );
        assert_eq!(
            capture_time_mtime("2024-03-10T01:30:00", &zone),
            Some(1_710_052_200)
        );

        // A value with no time of day is a refusal, never midnight of the local
        // zone; and one carrying a fraction or an offset stays whole seconds, as
        // the slice promises — though `catalog` normalises both away before a run
        // reaches here, so this half is the defence rather than a real shape.
        assert_eq!(
            capture_time_mtime("2024-03-10T01:30:00.500+01:00", &zone),
            Some(1_710_052_200)
        );
        assert_eq!(capture_time_mtime("2009-08-06", &zone), None);
    }

    #[test]
    fn a_folded_capture_time_leaves_the_files_own_mtime_alone() {
        let zone = est5edt();
        let dir = tempfile::tempdir().expect("temp dir");
        let output = dir.path().join("recovered.jpg");
        fs::write(&output, b"whatever the payload was").expect("write");
        let written = filetime::FileTime::from_unix_time(1_600_000_000, 0);
        filetime::set_file_mtime(&output, written).expect("set mtime");

        // The fold reaches the file as nothing at all: the mtime a recovery
        // arrived with is what it leaves with.
        apply_capture_time(&output, "2024-11-03T01:30:00", &zone);
        assert_eq!(mtime(&output), written);

        // The same file, a wall clock that is unambiguous: the mtime moves. So
        // the assertion above rests on the refusal, not on a missing write path.
        apply_capture_time(&output, "2024-11-03T03:30:00", &zone);
        assert_eq!(
            mtime(&output),
            filetime::FileTime::from_unix_time(1_730_622_600, 0)
        );
    }

    fn mtime(path: &Path) -> filetime::FileTime {
        let metadata = fs::metadata(path).expect("metadata");
        filetime::FileTime::from_last_modification_time(&metadata)
    }

    /// The US eastern transition rule as a POSIX string: no tzdb, and 2024 has a
    /// fold on 3 November and a gap on 10 March, so the ambiguity tests do not
    /// depend on the zone the machine holding them is in.
    fn est5edt() -> TimeZone {
        TimeZone::posix("EST5EDT,M3.2.0,M11.1.0").expect("a POSIX zone")
    }

    #[test]
    fn the_machine_s_own_zone_resolves_a_capture_time_rather_than_going_utc() {
        // The production path is the one line no other test reaches, and it is the
        // one that fails silently: when the platform's own zone cannot be found,
        // jiff's `TimeZone::system()` answers `Etc/Unknown` — a whole UTC offset
        // of wrong mtime, with no error anywhere. `try_system` is the same lookup
        // reporting that failure instead of absorbing it.
        let zone = TimeZone::try_system().expect("this machine's zone resolves to a real one");

        let dir = tempfile::tempdir().expect("temp dir");
        let output = dir.path().join("recovered.jpg");
        fs::write(&output, b"whatever the payload was").expect("write");
        let written = filetime::FileTime::from_unix_time(1_600_000_000, 0);
        filetime::set_file_mtime(&output, written).expect("set mtime");

        set_capture_time(&output, &item_with_capture_time("2024-07-01T12:00:00"));

        // Noon local is noon minus the zone's own offset at that instant, so the
        // mtime a UTC answer would write is a different number on every machine
        // that is not at UTC.
        let instant = zone
            .to_zoned(
                "2024-07-01T12:00:00"
                    .parse::<jiff::civil::DateTime>()
                    .expect("a civil datetime"),
            )
            .expect("an unambiguous instant");
        assert_eq!(
            mtime(&output),
            filetime::FileTime::from_unix_time(instant.timestamp().as_second(), 0)
        );
    }

    /// An item whose only interesting field is the catalog's capture time.
    fn item_with_capture_time(raw: &str) -> PlanItem {
        PlanItem {
            status: Status::Recoverable,
            image: Some(CatalogImage {
                id_local: 1,
                copy_name: None,
                orientation: Orientation::Normal,
                file_format: "JPEG".to_string(),
                original_path: PathBuf::from("photo.jpg"),
                root_folder: String::new(),
                path_from_root: String::new(),
                base_name: "photo".to_string(),
                extension: "JPG".to_string(),
                xmp: Some(XmpMetadata {
                    capture_time: Some(raw.to_string()),
                    ..XmpMetadata::default()
                }),
            }),
            payload: None,
            output: None,
            width: None,
            height: None,
            color_profile: None,
            quality: None,
            orientation: Orientation::Normal,
            effective_orientation: Orientation::Normal,
            payload_metadata: exif::PayloadMetadata::default(),
            tier: None,
            declared: None,
            smallest_level: None,
            selected: true,
            note: None,
        }
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
