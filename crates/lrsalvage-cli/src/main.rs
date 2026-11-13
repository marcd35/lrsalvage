//! `lrsalvage` — command line front end.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use lrsalvage_core::catalog::Catalog;
use lrsalvage_core::extract::{self, ExtractOptions, OutcomeStatus};
use lrsalvage_core::plan::{Options, Plan, Status, Tier};
use lrsalvage_core::previews_db::PreviewIndex;
use lrsalvage_core::{Error, Result, masks, report};

const EXIT_OK: u8 = 0;
const EXIT_USAGE: u8 = 1;
const EXIT_PARTIAL: u8 = 2;
const EXIT_NOTHING: u8 = 3;
const EXIT_UNSAFE: u8 = 4;

#[derive(Debug, Parser)]
#[command(
    name = "lrsalvage",
    version,
    about = "Recover JPEG previews from Adobe Lightroom Classic catalogs",
    long_about = "Reads a Lightroom catalog and its preview cache, and writes out the largest \
                  cached JPEG for every image whose original file is gone.\n\n\
                  Everything is opened read-only. Output never lands inside the catalog or \
                  the preview cache."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Report what is recoverable. Writes nothing except an explicitly
    /// requested contact sheet.
    Scan(ScanArgs),
    /// Write the recoverable JPEGs, plus a report and a JSON manifest.
    Extract(ExtractArgs),
    /// Decode `.lrcat-data` AI mask layers to PNG.
    Masks(MasksArgs),
}

#[derive(Debug, clap::Args)]
struct ScanArgs {
    /// Catalog file (`*.lrcat`).
    #[arg(long)]
    catalog: PathBuf,
    /// Preview cache directory (`* Previews.lrdata`).
    #[arg(long)]
    previews: PathBuf,
    /// Emit a machine-readable summary instead of a table.
    #[arg(long)]
    json: bool,
    /// Write a self-contained HTML contact sheet of the best previews.
    #[arg(long)]
    contact_sheet: Option<PathBuf>,
    /// Print one line per plan item instead of only totals.
    #[arg(long)]
    verbose: bool,
    /// Ignore previews below this long edge.
    #[arg(long, default_value_t = 640)]
    min_long_edge: u32,
    /// Inspect every level, not only the best one.
    #[arg(long)]
    all_levels: bool,
    /// Also include images whose original file is still present.
    #[arg(long)]
    include_present: bool,
}

#[derive(Debug, clap::Args)]
struct ExtractArgs {
    #[arg(long)]
    catalog: PathBuf,
    #[arg(long)]
    previews: PathBuf,
    /// Destination directory. Must not be inside the catalog or cache.
    #[arg(long)]
    out: PathBuf,
    /// Do everything except write files.
    #[arg(long)]
    dry_run: bool,
    #[arg(long, default_value_t = 640)]
    min_long_edge: u32,
    #[arg(long)]
    all_levels: bool,
    /// Also include images whose original file is still present.
    #[arg(long)]
    include_present: bool,
    /// Do not add EXIF, XMP rating or the ICC profile to the recovered JPEGs.
    #[arg(long)]
    no_metadata: bool,
    /// Do not set file modification times to the capture time.
    #[arg(long)]
    no_timestamps: bool,
    /// Replace an existing destination file that is not a verified extraction.
    #[arg(long)]
    force: bool,
}

#[derive(Debug, clap::Args)]
struct MasksArgs {
    /// A `<catalog>.lrcat-data` directory.
    #[arg(long)]
    data: PathBuf,
    #[arg(long)]
    out: PathBuf,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("lrsalvage: {e}");
            ExitCode::from(exit_code_for(&e))
        }
    }
}

/// Only an unsafe or unusable destination is code 4; everything else that
/// stops a run before it finishes is a usage or input error.
fn exit_code_for(error: &Error) -> u8 {
    match error {
        Error::UnsafeDestination(_) | Error::Destination { .. } => EXIT_UNSAFE,
        _ => EXIT_USAGE,
    }
}

fn run() -> Result<u8> {
    // clap's own parse failure is a usage error, and code 2 already means
    // "partial", so it must never escape here.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            // `--help` and `--version` are successful requests, not errors.
            let code = if error.use_stderr() {
                EXIT_USAGE
            } else {
                EXIT_OK
            };
            let _ = error.print();
            return Ok(code);
        }
    };
    match cli.command {
        Command::Scan(args) => scan(args),
        Command::Extract(args) => extract(args),
        Command::Masks(args) => masks(args),
    }
}

fn open_plan(
    catalog_path: &Path,
    previews: &Path,
    options: &Options,
    output_root: &Path,
) -> Result<(lrsalvage_core::snapshot::CatalogSnapshot, Plan)> {
    let previews_db = previews.join("previews.db");
    if !previews_db.exists() {
        eprintln!(
            "warning: {} has no previews.db; this cache holds no previews",
            previews.display()
        );
    }
    let (snapshot, catalog) = Catalog::open(catalog_path)?;
    let index = PreviewIndex::open(&previews_db)?;
    let plan = Plan::build(&catalog, &index, previews, output_root, options)?;
    Ok((snapshot, plan))
}

fn scan(args: ScanArgs) -> Result<u8> {
    let options = Options {
        min_long_edge: args.min_long_edge,
        all_levels: args.all_levels,
        include_present: args.include_present,
    };
    let (_snapshot, plan) = open_plan(&args.catalog, &args.previews, &options, Path::new("."))?;

    if args.json {
        println!("{}", scan_json(&plan, &args.catalog, &args.previews));
    } else {
        print!("{}", report::summary_with_floor(&plan, args.min_long_edge));
    }

    if args.verbose {
        for line in report::item_lines(&plan) {
            // Keep stdout parseable when --json was asked for.
            if args.json {
                eprintln!("{line}");
            } else {
                println!("{line}");
            }
        }
    }

    if let Some(path) = args.contact_sheet {
        // The sheet is the one thing `scan` writes, so it gets the same
        // destination rule as `--out`: never inside the evidence.
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        extract::ensure_safe_destination_pending(parent, &args.catalog, &args.previews)?;
        at_destination(
            &path,
            report::write_contact_sheet(&plan, &path, &args.previews),
        )?;
        eprintln!("contact sheet: {}", path.display());
    }

    // Exit 3 means "nothing selected", the same set `extract` would write —
    // not the raw recoverable count, which includes items below the floor.
    if plan.selected_images() == 0 {
        return Ok(EXIT_NOTHING);
    }
    Ok(EXIT_OK)
}

fn scan_json(plan: &Plan, catalog: &Path, previews: &Path) -> String {
    let stats = &plan.stats;
    let tiers: serde_json::Map<String, serde_json::Value> = stats
        .tiers
        .iter()
        .map(|(k, v)| ((*k).to_string(), serde_json::json!(v)))
        .collect();
    serde_json::json!({
        "catalog": catalog.to_string_lossy().replace('\\', "/"),
        "previews": previews.to_string_lossy().replace('\\', "/"),
        "images": stats.images,
        "recovered": stats.recoverable,
        "bytes": stats.bytes,
        "suspect": stats.suspect,
        "missing_payload": stats.missing_payload,
        "no_cache_entry": stats.no_cache_entry,
        "unmapped": stats.unmapped,
        "failed": stats.failed,
        "original_present": stats.original_present,
        "below_floor": stats.below_floor,
        "tiers": tiers,
    })
    .to_string()
}

fn extract(args: ExtractArgs) -> Result<u8> {
    let options = Options {
        min_long_edge: args.min_long_edge,
        all_levels: args.all_levels,
        include_present: args.include_present,
    };

    // Validate before creating: a refused destination must leave no directory
    // behind inside the catalog or the cache. A dry run creates nothing.
    if args.dry_run {
        extract::ensure_safe_destination_pending(&args.out, &args.catalog, &args.previews)?;
    } else {
        extract::ensure_safe_destination_create(&args.out, &args.catalog, &args.previews)?;
    }

    let (_snapshot, plan) = open_plan(&args.catalog, &args.previews, &options, &args.out)?;
    print!("{}", report::summary_with_floor(&plan, args.min_long_edge));

    let selected = plan.selected().count();
    if plan.selected_images() == 0 {
        eprintln!("nothing to extract");
        return Ok(EXIT_NOTHING);
    }

    let extract_options = ExtractOptions {
        output_root: args.out.clone(),
        dry_run: args.dry_run,
        write_metadata: !args.no_metadata,
        set_timestamps: !args.no_timestamps,
        write_orientation: true,
        force: args.force,
    };

    let bar = progress_bar(selected as u64, args.dry_run);
    let mut outcomes = Vec::with_capacity(selected);
    let result = extract::extract(&plan, &extract_options, &mut |outcome| {
        bar.inc(1);
        if outcome.status == OutcomeStatus::Failed {
            bar.suspend(|| {
                eprintln!(
                    "failed: {} — {}",
                    outcome.output.display(),
                    outcome.note.as_deref().unwrap_or("")
                )
            });
        }
        outcomes.push(outcome.clone());
    });
    bar.finish_and_clear();

    if let Err(e) = &result {
        eprintln!("extraction aborted: {e}");
    }

    let outcomes = outcomes;
    let failed = outcomes
        .iter()
        .filter(|o| o.status == OutcomeStatus::Failed)
        .count();

    if !args.dry_run {
        // These three files are the tool's own report, but a name on disk may
        // belong to somebody else: take the next free number, never clobber.
        let csv = claim(args.out.join("report.csv"));
        at_destination(&csv, report::write_csv(&plan, &csv))?;
        let manifest = claim(args.out.join("manifest.json"));
        at_destination(
            &manifest,
            report::write_json(&plan, &manifest, env!("CARGO_PKG_VERSION")),
        )?;
        let run_csv = claim(args.out.join("extract.csv"));
        at_destination(&run_csv, extract::write_outcomes_csv(&outcomes, &run_csv))?;
        eprintln!("wrote {}", csv.display());
        eprintln!("wrote {}", manifest.display());
        eprintln!("wrote {}", run_csv.display());
    }

    eprintln!("{}", extract::outcome_summary(&outcomes, args.dry_run));

    result?;
    if failed > 0 {
        return Ok(EXIT_PARTIAL);
    }
    Ok(EXIT_OK)
}

/// A failure to use `path` as a destination is exit 4, not "bad arguments".
fn at_destination(path: &Path, result: Result<()>) -> Result<()> {
    result.map_err(|error| match error {
        Error::Io { source, .. } => Error::Destination {
            path: path.to_path_buf(),
            source,
        },
        other => other,
    })
}

/// The requested path when it is free, otherwise the next free numbered name.
///
/// A run must never silently overwrite a file it cannot prove it wrote.
fn claim(path: PathBuf) -> PathBuf {
    let chosen = unused_path(&path);
    if chosen != path {
        eprintln!(
            "{} already exists; writing {} instead",
            path.display(),
            chosen.display()
        );
    }
    chosen
}

fn unused_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().into_owned());
    let mut number = 2u32;
    loop {
        let name = match &extension {
            Some(extension) => format!("{stem} ({number}).{extension}"),
            None => format!("{stem} ({number})"),
        };
        let candidate = path.with_file_name(name);
        if !candidate.exists() {
            return candidate;
        }
        number += 1;
    }
}

fn progress_bar(total: u64, hidden: bool) -> ProgressBar {
    if hidden {
        return ProgressBar::hidden();
    }
    let bar = ProgressBar::new(total);
    bar.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} {msg}",
        )
        .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );
    bar
}

fn masks(args: MasksArgs) -> Result<u8> {
    // `.lrcat-data` is read-only evidence too: refuse an `--out` inside it,
    // and do so before creating a single directory.
    extract::ensure_create_outside(&args.out, &args.data)?;

    let files = masks::list(&args.data)?;
    if files.is_empty() {
        eprintln!("no .dat files under {}", args.data.display());
        return Ok(EXIT_NOTHING);
    }

    let bar = ProgressBar::new(files.len() as u64);
    let mut written = 0usize;
    for path in files {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("skip {}: {e}", path.display());
                bar.inc(1);
                continue;
            }
        };
        match masks::decode_dat(&bytes).and_then(|mask| masks::to_png(&mask).map(|png| (mask, png)))
        {
            Ok((mask, png)) => {
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let out = args.out.join(format!("{name}.png"));
                if let Err(e) = std::fs::write(&out, png) {
                    eprintln!("skip {}: {e}", path.display());
                } else {
                    written += 1;
                    let _ = mask;
                }
            }
            Err(e) => eprintln!("skip {}: {e}", path.display()),
        }
        bar.inc(1);
    }
    bar.finish_and_clear();
    eprintln!("{written} mask(s) written to {}", args.out.display());
    if written == 0 {
        return Ok(EXIT_NOTHING);
    }
    Ok(EXIT_OK)
}

/// Keep the tier import meaningful even when no tier is present.
#[allow(dead_code)]
fn tier_label(tier: Option<Tier>) -> &'static str {
    tier.map(Tier::as_str).unwrap_or("")
}

/// Keep the status import meaningful even when no status is present.
#[allow(dead_code)]
fn status_label(status: Status) -> &'static str {
    status.as_str()
}
