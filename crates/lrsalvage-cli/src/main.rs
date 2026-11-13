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
    /// Report what is recoverable. Writes nothing to the destination.
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
    /// Ignore previews below this long edge.
    #[arg(long, default_value_t = 640)]
    min_long_edge: u32,
    /// Inspect every level, not only the best one.
    #[arg(long)]
    all_levels: bool,
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
    /// Do not add EXIF metadata to the recovered JPEGs.
    #[arg(long)]
    no_metadata: bool,
    /// Do not set file modification times to the capture time.
    #[arg(long)]
    no_timestamps: bool,
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
            let code = match e {
                Error::UnsafeDestination(_) => EXIT_UNSAFE,
                Error::NotACatalog { .. } => EXIT_USAGE,
                _ => EXIT_USAGE,
            };
            ExitCode::from(code)
        }
    }
}

fn run() -> Result<u8> {
    let cli = Cli::parse();
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
    };
    let (_snapshot, plan) = open_plan(&args.catalog, &args.previews, &options, Path::new("."))?;

    if args.json {
        println!("{}", scan_json(&plan, &args.catalog, &args.previews));
    } else {
        print!("{}", report::summary(&plan));
    }

    if let Some(path) = args.contact_sheet {
        report::write_contact_sheet(&plan, &path, &args.previews)?;
        eprintln!("contact sheet: {}", path.display());
    }

    if plan.stats.recoverable == 0 {
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
        "below_floor": stats.below_floor,
        "tiers": tiers,
    })
    .to_string()
}

fn extract(args: ExtractArgs) -> Result<u8> {
    let options = Options {
        min_long_edge: args.min_long_edge,
        all_levels: args.all_levels,
    };

    std::fs::create_dir_all(&args.out).map_err(|source| Error::Io {
        path: args.out.clone(),
        source,
    })?;
    extract::ensure_safe_destination(&args.out, &args.catalog, &args.previews)?;

    let (_snapshot, plan) = open_plan(&args.catalog, &args.previews, &options, &args.out)?;
    print!("{}", report::summary(&plan));

    let selected = plan.selected().count();
    if selected == 0 {
        eprintln!("nothing to extract");
        return Ok(EXIT_NOTHING);
    }

    let extract_options = ExtractOptions {
        output_root: args.out.clone(),
        dry_run: args.dry_run,
        write_metadata: !args.no_metadata,
        set_timestamps: !args.no_timestamps,
        write_orientation: true,
    };

    let bar = progress_bar(selected as u64, args.dry_run);
    let mut outcomes = Vec::with_capacity(selected);
    let mut result = extract::extract(&plan, &extract_options, &mut |outcome| {
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

    if let Err(e) = result {
        eprintln!("extraction aborted: {e}");
        result = Err(e);
    }

    let outcomes = outcomes;
    let written = outcomes
        .iter()
        .filter(|o| o.status == OutcomeStatus::Written)
        .count();
    let skipped = outcomes
        .iter()
        .filter(|o| o.status == OutcomeStatus::Skipped)
        .count();
    let failed = outcomes
        .iter()
        .filter(|o| o.status == OutcomeStatus::Failed)
        .count();
    let bytes: u64 = outcomes
        .iter()
        .filter(|o| o.status == OutcomeStatus::Written)
        .map(|o| o.bytes)
        .sum();

    if !args.dry_run {
        let csv = args.out.join("report.csv");
        report::write_csv(&plan, &csv)?;
        let manifest = args.out.join("manifest.json");
        report::write_json(&plan, &manifest, env!("CARGO_PKG_VERSION"))?;
        let run_csv = args.out.join("extract.csv");
        extract::write_outcomes_csv(&outcomes, &run_csv)?;
        eprintln!("wrote {}", csv.display());
        eprintln!("wrote {}", manifest.display());
        eprintln!("wrote {}", run_csv.display());
    }

    eprintln!(
        "{} written, {} skipped, {} failed, {:.1} MB",
        written,
        skipped,
        failed,
        bytes as f64 / 1_048_576.0
    );

    result?;
    if failed > 0 {
        return Ok(EXIT_PARTIAL);
    }
    Ok(EXIT_OK)
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
    std::fs::create_dir_all(&args.out).map_err(|source| Error::Io {
        path: args.out.clone(),
        source,
    })?;

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
