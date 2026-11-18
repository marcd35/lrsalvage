//! The package tree, asserted where a mistake is cheap to find.
//!
//! `lrsalvage-fixtures` is `publish = false`, so the published manifest drops the
//! dev-dependency: a test that reaches it must live where `[package] exclude` can
//! drop it too — `tests/`, never a `#[cfg(test)]` module inside `src/`
//! (`specs/fixtures.md`, *Absorbing the three copies, and where a test lives*).
//! #123 was that mistake measured from the outside, and `scripts/check.sh package`
//! only sees it after two `cargo package` runs and two cold tarball builds. This
//! test answers in milliseconds, inside the `test` gate, on every platform CI
//! runs — so the rule fails where it is easy to fix rather than in the slowest
//! gate of the list.
//!
//! It is excluded from the package itself: it spells the crate's name in order to
//! look for it, and this file is the one place that spelling is not a caller.

use std::fs;
use std::path::{Path, PathBuf};

/// The forms that actually reach the crate — a `use`, a path call, an
/// `extern crate`. Prose about it is written `lrsalvage-fixtures`, which matches
/// none of them, so a comment explaining the rule cannot trip this check.
const REACHES: [&str; 3] = [
    "lrsalvage_fixtures::",
    "use lrsalvage_fixtures",
    "extern crate lrsalvage_fixtures",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn reaches_fixtures(path: &Path) -> bool {
    let source = fs::read_to_string(path).expect("a readable source file");
    REACHES.iter().any(|form| source.contains(form))
}

/// Every `.rs` beneath `dir`, at any depth.
fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("a readable directory") {
        let path = entry.expect("a readable entry").path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

fn offenders(paths: &[PathBuf], keep: impl Fn(&Path) -> bool) -> Vec<String> {
    paths
        .iter()
        .filter(|path| keep(path))
        .map(|path| path.display().to_string())
        .collect()
}

#[test]
fn nothing_inside_src_reaches_the_fixtures_crate() {
    let mut found = Vec::new();
    sources(&root().join("src"), &mut found);
    assert!(!found.is_empty(), "the crate ships sources");

    // `src/` ships in every tarball, and a file there cannot be excluded: a crate
    // missing a module does not compile. One import is enough to turn an
    // auditor's `cargo test` into `E0433`.
    let offenders = offenders(&found, reaches_fixtures);
    assert!(
        offenders.is_empty(),
        "these ship in the package and cannot resolve `lrsalvage-fixtures` there — \
         move the test into `tests/` and exclude it: {offenders:#?}"
    );
}

#[test]
fn every_test_file_that_reaches_it_is_excluded_from_the_package() {
    let mut found = Vec::new();
    sources(&root().join("tests"), &mut found);
    let excluded = excluded_paths();

    let offenders = offenders(&found, |path| {
        reaches_fixtures(path) && !excluded.contains(&relative(path))
    });
    assert!(
        offenders.is_empty(),
        "these ship in the package and import a dev-dependency the published \
         manifest drops — add each to [package] exclude: {offenders:#?}"
    );
}

/// A path as `[package] exclude` spells it: slash-separated, crate-root relative.
fn relative(path: &Path) -> String {
    path.strip_prefix(root())
        .expect("a path under the crate root")
        .to_string_lossy()
        .replace('\\', "/")
}

/// The strings of the manifest's single `exclude = [...]` list.
fn excluded_paths() -> Vec<String> {
    let manifest = fs::read_to_string(root().join("Cargo.toml")).expect("the crate manifest");
    let list = manifest
        .split("exclude = [")
        .nth(1)
        .expect("a [package] exclude list")
        .split(']')
        .next()
        .expect("a closed list");
    list.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}
