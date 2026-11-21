//! The README's tables against the binary that has to honour them.
//!
//! `README.md` documents two surfaces a user relies on: the flags each command
//! takes, and the exact field set `scan --json` emits ("these fields, and only
//! these"). Both are prose, so both can drift, and one already had: this slice
//! shipped a README row promising a hint for an `unknown` layout that nothing
//! printed — and no gate noticed, because no gate reads the README.
//!
//! What is checkable is checked here. What is not — the truth of a sentence, the
//! shape of a note — stays a reviewer's job.

use std::path::PathBuf;

use assert_cmd::Command;
use lrsalvage_fixtures::vault::{ImageRow, Level, PyramidRow, Vault};
use serde_json::Value;

const UUID: &str = "AAAA0001-1111-4111-8111-000000000001";

fn readme() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../README.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// The table that follows the first paragraph containing `lead`, as rows of
/// cells. Markdown tables are separated by `|`, and the header is dropped.
fn table_after(text: &str, lead: &str) -> Vec<Vec<String>> {
    let start = text
        .find(lead)
        .unwrap_or_else(|| panic!("README has no paragraph containing {lead:?}"));
    let rest = &text[start..];
    let mut rows = Vec::new();
    let mut in_table = false;
    for line in rest.lines() {
        let trimmed = line.trim_end_matches('\r').trim();
        if trimmed.starts_with('|') {
            in_table = true;
            let cells: Vec<String> = trimmed
                .trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect();
            rows.push(cells);
        } else if in_table {
            break;
        }
    }
    assert!(!rows.is_empty(), "no table after {lead:?}");
    rows.remove(0);
    // The `|---|---|` separator.
    if rows
        .first()
        .is_some_and(|row| row.iter().all(|cell| cell.starts_with('-')))
    {
        rows.remove(0);
    }
    rows
}

/// Backticked names in one cell, which is how a field or a flag is written.
fn names(cell: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (index, part) in cell.split('`').enumerate() {
        // Odd indexes are inside backticks.
        if index % 2 == 1 {
            let name = part.split_whitespace().next().unwrap_or_default();
            if !name.is_empty() {
                found.push(name.to_string());
            }
        }
    }
    found
}

fn scan(vault: &Vault) -> Command {
    let mut command = Command::cargo_bin("lrsalvage").expect("the binary");
    command
        .arg("scan")
        .arg("--catalog")
        .arg(vault.catalog())
        .arg("--previews")
        .arg(vault.previews());
    command
}

fn vault() -> Vault {
    Vault::modern()
        .image(ImageRow::new(1, "IMG_0001").cache(UUID))
        .pyramid(PyramidRow::new(UUID).level(1616, 1077))
        .level_file(Level::sized(UUID, 1616, 1077))
        .write()
}

#[test]
fn the_documented_json_fields_are_exactly_the_ones_scan_emits() {
    let vault = vault();
    let output = scan(&vault)
        .arg("--json")
        .output()
        .expect("run scan --json");
    let emitted: Value = serde_json::from_slice(&output.stdout).expect("one JSON object");
    let mut emitted: Vec<String> = emitted
        .as_object()
        .expect("a JSON object")
        .keys()
        .cloned()
        .collect();
    emitted.sort();

    let documented: Vec<String> = table_after(&readme(), "emits these fields, and only these")
        .iter()
        .filter_map(|row| row.first())
        .flat_map(|cell| names(cell))
        .collect();
    let mut documented = documented;
    documented.sort();
    documented.dedup();

    assert_eq!(
        documented, emitted,
        "the README's `--json` table must list exactly the emitted fields; \
         a missing field is undocumented, an extra one does not exist"
    );
}

#[test]
fn every_documented_flag_exists_on_some_command() {
    let readme = readme();
    let flags: Vec<String> = table_after(&readme, "names which command each flag belongs to")
        .iter()
        .filter_map(|row| row.first())
        .flat_map(|cell| names(cell))
        .filter(|name| name.starts_with("--"))
        .collect();
    assert!(!flags.is_empty(), "the flag table should name flags");

    let mut help = String::new();
    for command in ["scan", "extract", "masks"] {
        let output = Command::cargo_bin("lrsalvage")
            .expect("the binary")
            .arg(command)
            .arg("--help")
            .output()
            .expect("run --help");
        help.push_str(&String::from_utf8_lossy(&output.stdout));
    }

    let missing: Vec<&String> = flags
        .iter()
        .filter(|flag| !help.contains(flag.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "the README documents flags no command has: {missing:?}"
    );
}

/// The README's own `report.csv` column order, which a script may rely on.
#[test]
fn the_documented_report_csv_columns_match_the_header() {
    let vault = vault();
    let mut extract = Command::cargo_bin("lrsalvage").expect("the binary");
    extract
        .arg("extract")
        .arg("--catalog")
        .arg(vault.catalog())
        .arg("--previews")
        .arg(vault.previews())
        .arg("--out")
        .arg(vault.out());
    extract.assert().code(0);

    let csv = std::fs::read_to_string(vault.out().join("report.csv")).expect("report.csv");
    let header = csv.lines().next().expect("a header");
    assert!(
        header.ends_with(",note,payload,payload_metadata"),
        "the README promises `payload` and then `payload_metadata`, last: {header}"
    );
    assert_eq!(
        header
            .split(',')
            .filter(|column| *column == "payload")
            .count(),
        1,
        "`payload_metadata` is a field of its own, not a second `payload`: {header}"
    );
}

/// The end of a release tag starting at `start`, if one starts there.
///
/// A tag is `v<major>.<minor>.<patch>`, with an optional pre-release suffix. The
/// three parts are required because this page also carries the *catalog's*
/// version numbers — "LrC 13+", "v11 verified" — and none of those is a tag a
/// user downloads. The suffix keeps its dots only where a digit follows, so
/// `v0.2.0-rc.1.tgz` yields `v0.2.0-rc.1` rather than swallowing the extension.
fn tag_at(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&b'v') {
        return None;
    }
    let mut end = start + 1;
    for part in 0..3 {
        if part > 0 {
            if bytes.get(end) != Some(&b'.') {
                return None;
            }
            end += 1;
        }
        let digits = end;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end == digits {
            return None;
        }
    }
    if bytes.get(end) == Some(&b'-') {
        end += 1;
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric()
                || (bytes[end] == b'.' && bytes.get(end + 1).is_some_and(u8::is_ascii_digit)))
        {
            end += 1;
        }
    }
    Some(end)
}

/// Every release tag in `text`, in the order they appear.
fn tags_in(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        // A `v` inside a word is not a tag.
        let starts_word = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        match tag_at(bytes, i).filter(|_| starts_word) {
            Some(end) => {
                found.push(text[i..end].to_string());
                i = end;
            }
            None => i += 1,
        }
    }
    found
}

/// Every tag the README tells a user to fetch is the release this crate builds.
///
/// The page names the release in twenty-one places — the version paragraph, three
/// archive filenames, the release link, three verify commands, the `cargo
/// install` line and the binstall paragraph — and none of them is generated. The
/// man page's two version strings are held by `scripts/check.sh completions`;
/// these were held by whoever remembered, so a bump that rewrote the manifest and
/// forgot the page shipped archive names that do not exist.
#[test]
fn every_tag_the_readme_names_is_this_crates_version() {
    // The tags this page names as history, each with the sentence that justifies
    // it: `v0.2.0-rc.1` proved the release workflow under its own tag spelling,
    // and `v0.2.0` is the release that first had something to download at all.
    const OTHER_TAGS: [&str; 2] = ["v0.2.0", "v0.2.0-rc.1"];

    let version = env!("CARGO_PKG_VERSION");
    let current = format!("v{version}");
    let text = readme();

    let named = tags_in(&text);
    assert!(!named.is_empty(), "the README names no tag at all");
    let stale: Vec<&String> = named
        .iter()
        .filter(|tag| **tag != current && !OTHER_TAGS.contains(&tag.as_str()))
        .collect();
    assert!(
        stale.is_empty(),
        "the README names tags this crate does not build: {stale:?} — it builds {current}"
    );

    // `Version <number>:` opens the paragraph that says what the number covers,
    // and it names the version without a `v` for the rest of the paragraph to use.
    let opening = text
        .lines()
        .find(|line| line.starts_with("Version "))
        .expect("the README opens a paragraph with `Version <number>:`");
    assert!(
        opening.starts_with(&format!("Version {version}:")),
        "the version paragraph opens {opening:?}, and this crate builds {version}"
    );
}
