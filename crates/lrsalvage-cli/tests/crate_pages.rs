//! The two crate pages, against the rules the release checklist states in prose.
//!
//! `crates/*/README.md` is what a crates.io reader meets before the API docs, and
//! no gate read them: `readme.rs` holds the repository README to the binary this
//! crate builds, and nothing held the crate pages to anything at all. They are
//! copied, not referenced — Cargo cannot point at a file outside the package — so
//! they are two claim surfaces with two chances to drift from ADR 0010.
//!
//! What is checkable is checked here: the promise is linked rather than restated,
//! the install line is present, and the page carries no table and no number of
//! its own. What is not — the truth of a sentence, whether the page is worth
//! reading — stays a reviewer's job.

use std::path::{Path, PathBuf};

/// The contract every page must point at instead of paraphrasing.
const CONTRACT: &str =
    "https://github.com/marcd35/lrsalvage/blob/main/docs/adr/0010-what-0-2-promises.md";

/// Digits that are part of a *name* rather than a claim: the contract's identity
/// as a pointer names it, and the licence is the SPDX label the manifest declares.
/// A page may say both; neither is a measurement.
const NAMES: [&str; 2] = ["ADR 0010", "Apache License, Version 2.0"];

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The two crate roots that carry a page, as `(crate, its root)`.
fn crates() -> [(&'static str, PathBuf); 2] {
    let root = crate_root();
    [
        ("lrsalvage-core", root.join("../lrsalvage-core")),
        ("lrsalvage-cli", root.clone()),
    ]
}

fn page(crate_name: &str, dir: &Path) -> String {
    let path = dir.join("README.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{crate_name}: read {}: {error}", path.display()))
}

#[test]
fn each_page_links_the_compatibility_contract() {
    for (crate_name, dir) in crates() {
        let text = page(crate_name, &dir);
        assert!(
            text.contains(CONTRACT),
            "{crate_name}: the page must link ADR 0010 ({CONTRACT}) rather than \
             restate the promise, which is how two copies of it start to disagree"
        );
    }
}

#[test]
fn each_page_gives_its_own_install_line() {
    // The line a reader acts on first, and the one the two packages differ in.
    for (crate_name, dir) in crates() {
        let text = page(crate_name, &dir);
        let install = if crate_name == "lrsalvage-cli" {
            "cargo install lrsalvage-cli"
        } else {
            "cargo add lrsalvage-core"
        };
        assert!(
            text.contains(install),
            "{crate_name}: the page must carry `{install}`"
        );
    }
}

#[test]
fn no_page_carries_a_table_or_a_row_of_its_own() {
    // A compatibility table, a field table or a flag table on the page is a second
    // home for something ADR 0010 or the repository README owns, and neither gate
    // reads this file.
    for (crate_name, dir) in crates() {
        let text = page(crate_name, &dir);
        for line in text.lines() {
            let trimmed = line.trim_start();
            assert!(
                !trimmed.starts_with('|'),
                "{crate_name}: the page carries a table row ({trimmed:?}); point at \
                 the ADR or the repository README instead of copying it"
            );
        }
    }
}

#[test]
fn no_page_carries_a_number_of_its_own() {
    // "No count, and no number copied from `docs/format-notes.md`" is the release
    // checklist's rule, and a number is the cheapest thing in this repository to
    // leave behind: a preview size, a layout count, a release. URLs are not
    // claims — the ADR's own `0010` is part of a path — so they come out first.
    for (crate_name, dir) in crates() {
        let text = page(crate_name, &dir);
        let mut without_names = strip_urls(&text);
        for name in NAMES {
            without_names = without_names.replace(name, "name");
        }
        if let Some(digit) = without_names
            .char_indices()
            .find(|(_, c)| c.is_ascii_digit())
            .map(|(i, _)| i)
        {
            let start = digit.saturating_sub(40);
            let end = (digit + 40).min(without_names.len());
            panic!(
                "{crate_name}: the page carries a number ({:?}); a number here is a \
                 claim with no gate behind it — point at the section that measured it",
                &without_names[start..end]
            );
        }
    }
}

/// The text with every `http(s)://…` run removed, since a URL's own digits are
/// part of a path rather than a claim.
fn strip_urls(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = find_url(rest) {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        let end = after
            .find(|c: char| c.is_whitespace() || matches!(c, '>' | ')' | ']'))
            .unwrap_or(after.len());
        out.push(' ');
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

fn find_url(text: &str) -> Option<usize> {
    [text.find("https://"), text.find("http://")]
        .into_iter()
        .flatten()
        .min()
}
