//! Generates the four completion scripts and the man page.
//!
//! This is not an example of the library's use. It is an *example target* because
//! that is the one kind of target `cargo` compiles with the crate's
//! dev-dependencies — where `clap_complete` and `clap_mangen` live, so a consumer
//! of the published crate resolves neither — and because `cargo install` installs
//! no second command from it. `Cargo.toml` says so beside the target's name.
//!
//! ```console
//! cargo run -p lrsalvage-cli --example artifacts -- <directory>
//! ```
//!
//! `scripts/check.sh completions` runs this into a temporary directory and diffs
//! the result against the committed copies, so a flag that changed without the
//! artifacts changing fails a run rather than a user; `release.yml` runs it into
//! `dist/`, so what a release attaches is what this tree generates.
//!
//! Two rules the artifacts obey, and both come from the gate that reads them
//! back:
//!
//! - **Everything is derived from `lrsalvage_cli::cli::command()`**, the one
//!   `clap` definition `--help` is printed from. The synopsis, every flag, every
//!   default and the man page's `EXIT STATUS` rows come out of it: there is no
//!   hand-kept second copy of the flag list anywhere in this tree, which is the
//!   drift `tests/readme.rs` was written to stop for the README's tables.
//! - **Every byte is `\n`.** The committed files are compared byte for byte on
//!   three operating systems, and `.gitattributes` marks them `-text` so no
//!   checkout rewrites them; nothing here may inherit a platform's line ending.

use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};

use clap::Command as ClapCommand;
use clap_complete::Shell;
use clap_mangen::Man;
use clap_mangen::roff::{Roff, bold, roman};
use lrsalvage_cli::{cli, exit};

/// The shells a script is attached for. `clap_complete` knows a fifth, `elvish`;
/// it is not here because the artifact list is the release's, and a shell this
/// release carries no script for is not one to claim.
const SHELLS: [Shell; 4] = [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell];

/// The man page's name, printed like every script's, so a caller diffing a
/// directory needs no list of its own.
const MAN_PAGE: &str = "lrsalvage.1";

fn main() -> Result<(), Box<dyn Error>> {
    let out = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: artifacts <directory>")?;
    std::fs::create_dir_all(&out)?;

    // `generate_to` writes a shell's own conventional name — `lrsalvage.bash`,
    // `_lrsalvage`, `lrsalvage.fish`, `_lrsalvage.ps1` — which is what a user
    // drops into that shell's completion directory.
    let mut command = cli::command();
    for shell in SHELLS {
        let written = clap_complete::generate_to(shell, &mut command, "lrsalvage", &out)?;
        println!("{}", file_name(&written));
    }

    let page = man_page()?;
    // Written as given: `std::fs::write` translates no line ending, and the gate
    // compares these bytes on every platform.
    std::fs::write(out.join(MAN_PAGE), page.as_bytes())?;
    println!("{MAN_PAGE}");
    Ok(())
}

/// The man page, assembled from `clap_mangen`'s sections plus the ones it has no
/// opinion about.
///
/// The order is a man page's, not `clap_mangen`'s own: the root's sections, then
/// `COMMANDS`, then one section per subcommand carrying that command's own
/// derived synopsis and options, then `EXIT STATUS`, then `VERSION`. What it
/// deliberately does not do is take `Man::render`, whose `SUBCOMMANDS` section
/// names `lrsalvage-scan(1)` and its siblings: this release ships **one** page,
/// so a cross-reference to the three it does not ship is a reader's dead end.
fn man_page() -> Result<String, Box<dyn Error>> {
    let command = cli::command();
    let man = Man::new(command.clone());

    // The `.TH` line, and the only copy of roff's apostrophe preamble: the
    // `source` field is the name and version `clap_mangen` reads off the
    // command, so the page cannot disagree with `lrsalvage --version`.
    let mut page = rendered(|writer| Man::render_title(&man, writer))?;

    for section in [
        Man::render_name_section,
        Man::render_synopsis_section,
        Man::render_description_section,
        Man::render_options_section,
    ] {
        page.push_str(block(&rendered(|writer| section(&man, writer))?)?);
    }

    page.push_str(block(&commands_section(&command).render())?);

    for subcommand in command.get_subcommands() {
        // `bin_name` and not merely the subcommand's own name: `clap_mangen`
        // renders the synopsis from it, and `lrsalvage scan` is what `--help`
        // prints in its usage line and what a reader can type.
        let sub_man = Man::new(
            subcommand
                .clone()
                .bin_name(format!("lrsalvage {}", subcommand.get_name())),
        );
        page.push_str(&format!(
            ".SH {}\n",
            subcommand.get_name().to_ascii_uppercase()
        ));
        // A command's synopsis and options are `clap_mangen`'s own, one heading
        // level down: the page already has one `SYNOPSIS` and one `OPTIONS`, and
        // a reader of `lrsalvage scan` wants them under `SCAN`.
        for section in [Man::render_synopsis_section, Man::render_options_section] {
            let body = rendered(|writer| section(&sub_man, writer))?;
            page.push_str(&demote(block(&body)?)?);
        }
    }

    page.push_str(block(&exit_status_section().render())?);
    page.push_str(block(&rendered(|writer| {
        Man::render_version_section(&man, writer)
    })?)?);
    Ok(page)
}

/// `COMMANDS`, derived from the definition: one `.TP` per subcommand, its name in
/// bold and the `about` string `--help` prints.
fn commands_section(command: &ClapCommand) -> Roff {
    let mut roff = Roff::default();
    roff.control("SH", ["COMMANDS"]);
    for subcommand in command.get_subcommands() {
        roff.control("TP", []);
        roff.text([bold(subcommand.get_name())]);
        if let Some(about) = subcommand.get_about() {
            for line in about.to_string().lines() {
                roff.text([roman(line)]);
            }
        }
    }
    roff
}

/// `EXIT STATUS`, and its **one source**: [`exit::PUBLISHED`], whose numbers are
/// the constants `main.rs`'s unit test
/// `the_published_exit_numbers_are_pinned_and_each_code_has_a_case` pins — the
/// same test that pins `exit_code_for`, the mapping an error to a code. Nothing
/// here retypes a number or invents a row: add a code and that test fails until
/// the table has a row for it, which is how this section stays derived.
fn exit_status_section() -> Roff {
    let mut roff = Roff::default();
    roff.control("SH", ["EXIT STATUS"]);
    for (code, meaning) in exit::PUBLISHED {
        roff.control("TP", []);
        roff.text([bold(code.to_string())]);
        roff.text([roman(meaning)]);
    }
    roff
}

/// One of `clap_mangen`'s sections, rendered.
///
/// The section renderers all take `&mut dyn Write`, which is what lets each be
/// collected into a `String` before the page decides where it belongs.
fn rendered(
    section: impl FnOnce(&mut dyn io::Write) -> io::Result<()>,
) -> Result<String, Box<dyn Error>> {
    let mut buffer = Vec::new();
    section(&mut buffer)?;
    Ok(String::from_utf8(buffer).expect("roff output is UTF-8"))
}

/// A rendered block — the heading control and the body under it — without the
/// two-line apostrophe preamble `Roff::render` writes ahead of every block. The
/// page carries that preamble once, at its title, so a block inside it
/// contributes only from its first heading.
fn block(rendered: &str) -> Result<&str, Box<dyn Error>> {
    let mut offset = 0;
    for line in rendered.split_inclusive('\n') {
        if line.starts_with(".SH ") || line.starts_with(".SS ") {
            return Ok(&rendered[offset..]);
        }
        offset += line.len();
    }
    Err("no heading in a rendered section: clap_mangen's layout moved".into())
}

/// A block one heading level down, for a subcommand's own sections.
fn demote(block: &str) -> Result<String, Box<dyn Error>> {
    let rest = block
        .strip_prefix(".SH ")
        .ok_or("a command's section no longer opens with a level-one heading")?;
    Ok(format!(".SS {rest}"))
}

/// A file's name, as the generator prints it: a caller diffing a directory
/// therefore needs no list of its own.
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}
