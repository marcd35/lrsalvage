//! The only module that writes to disk.
//!
//! Every file is produced atomically: bytes go to `<name>.partial`, are
//! verified, then renamed into place. Nothing is written inside the catalog or
//! the preview cache — [`ensure_safe_destination`] enforces that.

use std::fs;
use std::path::{Path, PathBuf};

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
}

impl Default for ExtractOptions {
    fn default() -> Self {
        ExtractOptions {
            output_root: PathBuf::new(),
            dry_run: false,
            write_metadata: true,
            set_timestamps: true,
            write_orientation: true,
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
pub fn ensure_safe_destination(
    output_root: &Path,
    catalog_path: &Path,
    cache_root: &Path,
) -> Result<()> {
    let out = canonical(output_root);
    let catalog_dir = canonical(catalog_path).parent().map(Path::to_path_buf);
    let cache = canonical(cache_root);

    if let Some(dir) = catalog_dir {
        if out == dir || out.starts_with(&dir) {
            return Err(Error::UnsafeDestination(output_root.to_path_buf()));
        }
    }
    if out == cache || out.starts_with(&cache) {
        return Err(Error::UnsafeDestination(output_root.to_path_buf()));
    }
    Ok(())
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
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

fn extract_one(item: &PlanItem, options: &ExtractOptions) -> Outcome {
    let (Some(payload_path), Some(output)) = (item.payload.as_ref(), item.output.as_ref()) else {
        return failed(
            item.output.clone().unwrap_or_default(),
            "plan item has no payload or no output path",
        );
    };

    let expected = (item.width.unwrap_or(0), item.height.unwrap_or(0));

    // Resume: never overwrite a file that already looks right.
    if output.exists() {
        if let Ok(existing) = fs::read(output) {
            if jpeg::dimensions(&existing).is_ok_and(|dims| dims == expected) {
                return Outcome {
                    output: output.clone(),
                    status: OutcomeStatus::Skipped,
                    bytes: existing.len() as u64,
                    sha256: None,
                    note: Some("already recovered and verified".into()),
                };
            }
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
        let meta = exif::from_catalog(&xmp, orientation);
        if !meta.is_empty() {
            match exif::inject(&bytes, &meta) {
                Ok(tagged) => bytes = tagged,
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
            note: Some("dry run; nothing written".into()),
        };
    }

    if let Some(parent) = output.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return failed(output.clone(), format!("cannot create {parent:?}: {e}"));
        }
    }

    let temp = output.with_extension("jpg.partial");
    if let Err(e) = fs::write(&temp, &bytes) {
        let _ = fs::remove_file(&temp);
        return failed(output.clone(), format!("cannot write {temp:?}: {e}"));
    }

    if output.exists() {
        let _ = fs::remove_file(output);
    }
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
        note: None,
    }
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
