//! The command surface: three subcommands, their flags, and the one `clap`
//! definition every artifact of this crate comes from.
//!
//! `--help` is printed from it, `main.rs` parses with it, and the four completion
//! scripts and the man page are generated from it, so a flag cannot reach one and
//! miss the others. It holds no format knowledge and reads no file: the arguments
//! it describes are handed to `main.rs`, which drives `lrsalvage-core`.

use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};

/// The one wording both commands' `--verify-source` help uses: two copies of a
/// promise drift, and clap's own `--help` is where a user meets it.
const VERIFY_SOURCE_HELP: &str = "Prove the catalog was only read: hash it, copy it, hash the copy, and fail if the two disagree.";

/// The definition, as `clap`'s `Command` rather than as parsed arguments.
///
/// A generator holds the same value the binary parses: `clap_complete` and
/// `clap_mangen` walk it to write their output, and `main.rs` calls
/// [`clap::Parser::try_parse`] on the type it is derived for. There is no second
/// list of flags anywhere in this tree.
pub fn command() -> clap::Command {
    Cli::command()
}

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
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Report what is recoverable. Writes nothing except the files you name.
    Scan(ScanArgs),
    /// Write the recoverable JPEGs, plus a report and a JSON manifest.
    Extract(ExtractArgs),
    /// Decode `.lrcat-data` AI mask layers to PNG.
    Masks(MasksArgs),
}

#[derive(Debug, clap::Args)]
pub struct ScanArgs {
    /// Catalog file (`*.lrcat`).
    #[arg(long)]
    pub catalog: PathBuf,
    /// Preview cache directory (`* Previews.lrdata`).
    #[arg(long)]
    pub previews: PathBuf,
    /// Emit a machine-readable summary instead of a table.
    #[arg(long)]
    pub json: bool,
    /// Write a self-contained HTML contact sheet of the best previews.
    #[arg(long)]
    pub contact_sheet: Option<PathBuf>,
    /// Write the path-free verification report to this file.
    #[arg(long)]
    pub report: Option<PathBuf>,
    /// Print one line per plan item instead of only totals.
    #[arg(long)]
    pub verbose: bool,
    /// Ignore previews below this long edge.
    #[arg(long, default_value_t = 640)]
    pub min_long_edge: u32,
    /// Write the largest preview at or below this long edge, not the largest
    /// present. `0` disables the cap.
    #[arg(long, default_value_t = 0)]
    pub max_long_edge: u32,
    /// Inspect every level, not only the best one.
    #[arg(long)]
    pub all_levels: bool,
    /// Also include images whose original file is still present.
    #[arg(long)]
    pub include_present: bool,
    #[arg(long, help = VERIFY_SOURCE_HELP)]
    pub verify_source: bool,
}

#[derive(Debug, clap::Args)]
pub struct ExtractArgs {
    #[arg(long)]
    pub catalog: PathBuf,
    #[arg(long)]
    pub previews: PathBuf,
    /// Destination directory. Must not be inside the catalog or cache.
    #[arg(long)]
    pub out: PathBuf,
    /// Do everything except write files.
    #[arg(long)]
    pub dry_run: bool,
    /// Ignore previews below this long edge.
    #[arg(long, default_value_t = 640)]
    pub min_long_edge: u32,
    /// Write the largest preview at or below this long edge, not the largest
    /// present. `0` disables the cap.
    #[arg(long, default_value_t = 0)]
    pub max_long_edge: u32,
    #[arg(long)]
    pub all_levels: bool,
    /// Also include images whose original file is still present.
    #[arg(long)]
    pub include_present: bool,
    #[arg(long, help = VERIFY_SOURCE_HELP)]
    pub verify_source: bool,
    /// Do not add EXIF, XMP rating or the ICC profile to the recovered JPEGs.
    ///
    /// Such output carries no marker, so a later run cannot prove it wrote the
    /// file: every destination is reported as not a verified extraction and is
    /// left untouched unless --force is given.
    #[arg(long)]
    pub no_metadata: bool,
    /// Do not set file modification times to the capture time.
    #[arg(long)]
    pub no_timestamps: bool,
    /// Replace an existing destination file that is not a verified extraction.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, clap::Args)]
pub struct MasksArgs {
    /// A `<catalog>.lrcat-data` directory.
    #[arg(long)]
    pub data: PathBuf,
    #[arg(long)]
    pub out: PathBuf,
}
