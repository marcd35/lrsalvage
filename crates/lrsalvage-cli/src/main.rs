//! `lrsalvage` — command line front end.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use lrsalvage_cli::cli::{Cli, Command, ExtractArgs, MasksArgs, ScanArgs};
use lrsalvage_cli::exit::{
    EXIT_EVIDENCE, EXIT_NOTHING, EXIT_OK, EXIT_PARTIAL, EXIT_UNSAFE, EXIT_USAGE,
};
use lrsalvage_core::catalog::Catalog;
use lrsalvage_core::extract::{self, ExtractOptions, OutcomeStatus};
use lrsalvage_core::plan::{Options, Plan, Status, Tier};
use lrsalvage_core::previews_db::{IndexProblem, Layout};
use lrsalvage_core::{Error, Result, masks, previews_db, report};

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
/// stops a run before it finishes is a usage or input error, except a copy that
/// is not the catalog, which is code 5.
///
/// The `EXIT_USAGE` arm names every remaining variant one by one instead of a
/// `_` default: a new error then cannot compile until this one place decides what
/// it means, which is the whole edit a new exit code needs.
fn exit_code_for(error: &Error) -> u8 {
    match error {
        Error::UnsafeDestination(_) | Error::Destination { .. } => EXIT_UNSAFE,
        Error::SourceChanged { .. } => EXIT_EVIDENCE,
        Error::Io { .. }
        | Error::Sqlite(_)
        | Error::NotACatalog { .. }
        | Error::NotAPreviewCache { .. }
        | Error::RequiredPreviewIndex { .. }
        | Error::PayloadMissing { .. }
        | Error::PreviewUnavailable
        | Error::PreviewLevelMissing { .. }
        | Error::PreviewDimensionsChanged { .. }
        | Error::LrprevTruncated { .. }
        | Error::LrprevHeader(_)
        | Error::PyramidHeader(_)
        | Error::JpegMalformed
        | Error::JpegMarkersMissing
        | Error::IccTooLarge(_)
        | Error::Xmp(_)
        | Error::Png(_) => EXIT_USAGE,
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
    verify_source: bool,
) -> Result<(
    lrsalvage_core::snapshot::CatalogSnapshot,
    Plan,
    Option<IndexProblem>,
)> {
    require_cache_directory(previews)?;

    // The one open of the user's catalog. Asking for the proof costs one more
    // read of the catalog, and one of our own copy, which is why
    // `--verify-source` is opt-in rather than paid on every run
    // (`adr-0010-what-0-2-promises`).
    let (snapshot, catalog) = if verify_source {
        Catalog::open_verified(catalog_path)?
    } else {
        Catalog::open(catalog_path)?
    };
    // A missing or unreadable index is a mode, never a fatal error: the plan is
    // built from an empty index and the reason is carried out for the hint.
    let opened = previews_db::open(&previews.join(previews_db::INDEX_FILE));
    let plan = Plan::build(&catalog, &opened.index, previews, output_root, options)?;
    Ok((snapshot, plan, opened.problem))
}

/// A `--previews` path that is not a directory has nothing to salvage, which is a
/// different fact from an empty cache: a typo must never read as one
/// (`adr-0006-degrade-without-guessing` ¶3).
///
/// `extract` asks this before it touches `--out`, so a mistyped cache path can
/// never leave a directory behind or be outranked by a destination refusal.
fn require_cache_directory(previews: &Path) -> Result<()> {
    if previews.is_dir() {
        return Ok(());
    }
    Err(Error::NotAPreviewCache {
        path: previews.to_path_buf(),
    })
}

fn scan(args: ScanArgs) -> Result<u8> {
    let options = Options {
        min_long_edge: args.min_long_edge,
        max_long_edge: args.max_long_edge,
        all_levels: args.all_levels,
        include_present: args.include_present,
    };
    let (snapshot, plan, index_problem) = open_plan(
        &args.catalog,
        &args.previews,
        &options,
        Path::new("."),
        args.verify_source,
    )?;

    if args.json {
        println!("{}", scan_json(&plan, &args.catalog, &args.previews));
    } else {
        print!("{}", report::summary(&plan, &options));
    }
    index_hint(&plan, index_problem.as_ref(), &args.previews);
    layout_hint(&plan);

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

    // Both artifacts a scan can be asked for are written here: on exit 0 and on
    // exit 3 alike, before the "nothing selected" decision, because a run that
    // recovered nothing is the run most worth handing over. Neither decides the
    // code (`adr-0007-publish-a-verification-report`).
    if let Some(path) = args.report {
        write_verification_report(
            &plan,
            &options,
            snapshot.digests(),
            &path,
            &args.catalog,
            &args.previews,
        )?;
    }

    if let Some(path) = args.contact_sheet {
        // The sheet is a named artifact like any other, so it gets the same
        // destination rule as `--out`: never inside the evidence.
        extract::ensure_safe_destination_pending(
            destination_parent(&path),
            &args.catalog,
            &args.previews,
        )?;
        at_destination(
            &path,
            report::write_contact_sheet(&plan, &path, &args.previews),
        )?;
        eprintln!("contact sheet: {}", path.display());
    }

    // A mismatch already failed the run inside the acquisition: there is no
    // plan, so nothing here can publish the evidence either
    // (`adr-0007-publish-a-verification-report`).

    // Exit 3 means "nothing selected", the same set `extract` would write —
    // not the raw recoverable count, which includes items below the floor.
    if plan.selected_images() == 0 {
        return Ok(EXIT_NOTHING);
    }
    Ok(EXIT_OK)
}

/// ADR 0006 ¶1–¶2's one block for a cache whose index is missing or unreadable.
///
/// It names the file and the counts the plan produced, and it never claims the
/// cache is empty: no index means no *mapping*, not no payloads.
fn index_hint(plan: &Plan, problem: Option<&IndexProblem>, previews: &Path) {
    let Some(problem) = problem else {
        return;
    };
    let file = previews
        .join(previews_db::INDEX_FILE)
        .to_string_lossy()
        .replace('\\', "/");
    let stats = &plan.stats;
    match problem {
        IndexProblem::Missing => {
            eprintln!(
                "hint: {file} is missing, so {} payloads cannot be matched to any of the {} catalog images.",
                stats.unmapped, stats.images
            );
            eprintln!(
                "      They are listed as unmapped and never written; scan --verbose prints each path, extract records them in report.csv."
            );
        }
        IndexProblem::Unreadable(reason) => {
            eprintln!(
                "hint: {file} could not be read ({reason}), so {} payloads cannot be matched to any of the {} catalog images.",
                stats.unmapped, stats.images
            );
            eprintln!(
                "      They are listed as unmapped and never written; fix or move the file to name them."
            );
        }
    }
}

/// ADR 0004's one hint for a cache whose payloads are all of a shape this reader
/// does not know: the layout word alone tells a user the cache is unreadable but
/// not why, and there is no path to name because nothing was ever a payload as
/// far as this tool is concerned.
///
/// Nothing here refuses the run: an `unknown` layout selects no image, so it
/// exits 3 the same way an empty cache does.
fn layout_hint(plan: &Plan) {
    if plan.cache.layout != Layout::Unknown {
        return;
    }
    eprintln!(
        "hint: {} payload-like files in the cache match no layout this tool knows, so no preview could be named.",
        plan.stats.unrecognised
    );
    eprintln!(
        "      They are counted as unrecognised and never written; the cache's layout is unknown."
    );
}

fn scan_json(plan: &Plan, catalog: &Path, previews: &Path) -> String {
    let stats = &plan.stats;
    serde_json::json!({
        "catalog": catalog.to_string_lossy().replace('\\', "/"),
        "previews": previews.to_string_lossy().replace('\\', "/"),
        // The cache's shape, observed rather than interpreted: one layout word
        // and two version strings, never a gate (`adr-0004-detect-layout-not-version`).
        "layout": plan.cache.layout.as_str(),
        "db_version": plan.cache.db_version,
        "crs_version": plan.cache.crs_version,
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
        "above_cap": stats.above_cap,
        "superseded": stats.superseded,
        "deleted": stats.deleted,
        "unrecognised": stats.unrecognised,
        "thumbnail_pyramids": plan.cache.thumbnail_pyramids,
        "tiers": report::tiers_json(stats),
        "shapes": report::shapes_json(&plan.cache.shapes),
    })
    .to_string()
}

fn extract(args: ExtractArgs) -> Result<u8> {
    let options = Options {
        min_long_edge: args.min_long_edge,
        max_long_edge: args.max_long_edge,
        all_levels: args.all_levels,
        include_present: args.include_present,
    };

    // The inputs are checked before the destination: `--out` is only created
    // for a run that can read its cache.
    require_cache_directory(&args.previews)?;

    // Validate before creating: a refused destination must leave no directory
    // behind inside the catalog or the cache. A dry run creates nothing.
    if args.dry_run {
        extract::ensure_safe_destination_pending(&args.out, &args.catalog, &args.previews)?;
    } else {
        extract::ensure_safe_destination_create(&args.out, &args.catalog, &args.previews)?;
    }

    let (_snapshot, plan, index_problem) = open_plan(
        &args.catalog,
        &args.previews,
        &options,
        &args.out,
        args.verify_source,
    )?;
    print!("{}", report::summary(&plan, &options));
    index_hint(&plan, index_problem.as_ref(), &args.previews);
    layout_hint(&plan);

    let selected = plan.selected().count();
    if plan.selected_images() == 0 {
        // ADR 0006 ¶7: a run that named nothing still hands over what it found
        // when it found orphan payloads. A run with neither writes no file at
        // all — an empty run is not something to report on.
        if !args.dry_run && plan.stats.unmapped > 0 {
            write_plan_reports(&plan, &args.out)?;
        }
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

    // #64: the destination's own answer, before the first file is written. A
    // dry run asks the same question and reports the same verdict — it is where
    // a careful user looks first, and it writes nothing either way.
    //
    // The number is what this run would still write, not the plan's whole total:
    // a destination that already holds an output is skipped or failed, never
    // written, so a resume is measured by the bytes it has left. The bounds the
    // run applied (`--min-long-edge`, `--max-long-edge`, `--all-levels`) move the
    // plan's total, and so move this.
    let needed = extract::pending_bytes(&plan, &extract_options);
    match extract::ensure_room(&args.out, needed)? {
        extract::Preflight::Fits(room) => {
            if let Some(note) = room.nearly_full_note(&args.out) {
                eprintln!("{note}");
            }
        }
        extract::Preflight::Unmeasured(reason) => eprintln!(
            "the destination's free space could not be measured, so the pre-flight was \
             skipped: {reason}"
        ),
    }

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
        write_plan_reports(&plan, &args.out)?;
        let run_csv = claim(args.out.join("extract.csv"));
        at_destination(&run_csv, extract::write_outcomes_csv(&outcomes, &run_csv))?;
        eprintln!("wrote {}", run_csv.display());
    }

    eprintln!("{}", extract::outcome_summary(&outcomes, args.dry_run));

    result?;
    if failed > 0 {
        return Ok(EXIT_PARTIAL);
    }
    Ok(EXIT_OK)
}

/// The plan's own record: `report.csv` and `manifest.json`, written whenever a
/// run has something to report on and never on a dry run (`adr-0006` ¶7).
///
/// Each name is claimed, never assumed: a name on disk may belong to somebody
/// else, so an existing one takes the next free number.
fn write_plan_reports(plan: &Plan, out: &Path) -> Result<()> {
    let csv = claim(out.join("report.csv"));
    at_destination(&csv, report::write_csv(plan, &csv))?;
    let manifest = claim(out.join("manifest.json"));
    at_destination(
        &manifest,
        report::write_json(plan, &manifest, env!("CARGO_PKG_VERSION")),
    )?;
    eprintln!("wrote {}", csv.display());
    eprintln!("wrote {}", manifest.display());
    Ok(())
}

/// The directory a named artifact lands in, `.` when only a name was given.
fn destination_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

/// `--report`: the path-free verification report, written for the plan the run
/// made and moved by no exit code (`adr-0007-publish-a-verification-report`).
///
/// The parent obeys the `--out` destination rule, and the name is claimed: a
/// name already on disk may belong to somebody else. What the file says is
/// `report`'s; the path, the claim and the code are this module's.
fn write_verification_report(
    plan: &Plan,
    options: &Options,
    digests: Option<&lrsalvage_core::snapshot::SourceDigests>,
    path: &Path,
    catalog: &Path,
    previews: &Path,
) -> Result<()> {
    extract::ensure_safe_destination_pending(destination_parent(path), catalog, previews)?;
    let path = claim(path.to_path_buf());
    at_destination(
        &path,
        report::write_verification(plan, options, digests, &path, env!("CARGO_PKG_VERSION")),
    )?;
    eprintln!("wrote {}", path.display());
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(byte: char) -> String {
        byte.to_string().repeat(64)
    }

    /// The published numbers, and one case per code, are the contract this
    /// crate can assert. Which code an existing variant takes is the `match`'
    /// own prose: nothing compares a variant to a number except the arm beside
    /// it. What the build does hold is the set — `exit_code_for` names every
    /// variant rather than defaulting, so a new error cannot compile until the
    /// mapping decides which code it takes.
    #[test]
    fn the_published_exit_numbers_are_pinned_and_each_code_has_a_case() {
        assert_eq!(
            [
                EXIT_OK,
                EXIT_USAGE,
                EXIT_PARTIAL,
                EXIT_NOTHING,
                EXIT_UNSAFE,
                EXIT_EVIDENCE
            ],
            [0, 1, 2, 3, 4, 5],
            "the numbers are the contract; the README's table is their prose"
        );

        // The man page's `EXIT STATUS` section is `exit::PUBLISHED`, whose numbers
        // are the constants above. A code that reached one and not the other is
        // this line's failure — which is what makes the generated man page's exit
        // section derived rather than a second table.
        assert_eq!(
            lrsalvage_cli::exit::PUBLISHED.map(|(code, _)| code),
            [
                EXIT_OK,
                EXIT_USAGE,
                EXIT_PARTIAL,
                EXIT_NOTHING,
                EXIT_UNSAFE,
                EXIT_EVIDENCE
            ],
            "every published code needs a row in the man page's EXIT STATUS section"
        );

        let path = PathBuf::from("C:/evidence/2023.lrcat");
        let cases = [
            (
                Error::UnsafeDestination(path.clone()),
                EXIT_UNSAFE,
                "a destination inside the evidence",
            ),
            (
                Error::Destination {
                    path: path.clone(),
                    source: std::io::Error::other("denied"),
                },
                EXIT_UNSAFE,
                "a name that cannot be written",
            ),
            (
                Error::SourceChanged {
                    path: path.clone(),
                    source_sha256: digest('a'),
                    snapshot_sha256: digest('b'),
                },
                EXIT_EVIDENCE,
                "a copy that is not the catalog",
            ),
            (
                Error::NotACatalog { path: path.clone() },
                EXIT_USAGE,
                "an input that is not a catalog",
            ),
            (
                Error::Io {
                    path: path.clone(),
                    source: std::io::Error::other("missing"),
                },
                EXIT_USAGE,
                "a catalog that cannot be read",
            ),
        ];

        for (error, code, what) in cases {
            assert_eq!(exit_code_for(&error), code, "{what}: {error}");
        }
    }
}
