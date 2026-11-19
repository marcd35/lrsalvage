//! The filesystem seam, held by a search rather than by a reviewer's eye.
//!
//! `src/fs.rs` is the crate's one answer to a filesystem question, and
//! `specs/modules/fs.md` says nothing else in `lrsalvage-core` names `std::fs`,
//! `tempfile` or `filetime`. Prose alone cannot hold that: a module that needs
//! one more read has no reason to remember the rule, and the next target then
//! discovers the call site instead of the build. So the rule is asserted here,
//! the same way `spec_graph.rs` holds a module's `depends-on` to the code.
//!
//! The scan reads each source file up to its own `#[cfg(test)]` module. A test
//! may reach the disk directly — a test that checked the seam through the seam
//! would prove nothing — and `scripts/check.sh package` runs this file inside the
//! published tarball, where the rule has to hold as well.
//!
//! One exception is recorded in the spec and in this list: `masks` keeps its own
//! `walkdir` walk, because it is native-only and outside the seam (#117), and
//! `walkdir` is not one of the three names below.

use std::fs;
use std::path::{Path, PathBuf};

/// The module that owns filesystem access, and the directory holding its
/// backends. Every path under it is exempt.
const SEAM: &str = "src/fs";

/// The names only the seam may use.
const FORBIDDEN: [&str; 3] = ["std::fs", "tempfile", "filetime"];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every `.rs` file under `dir`, recursively, as a path relative to the crate.
fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("a readable source directory") {
        let path = entry.expect("a readable entry").path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let relative = path
                .strip_prefix(root())
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            found.push(PathBuf::from(relative));
        }
    }
}

/// The module's shipped code: everything before its own test module.
fn shipped_code(source: &str) -> &str {
    source
        .split("#[cfg(test)]")
        .next()
        .expect("a split always yields a first part")
}

/// Whether this file is the seam, or one of its backends.
fn is_seam(relative: &str) -> bool {
    relative == format!("{SEAM}.rs") || relative.starts_with(&format!("{SEAM}/"))
}

#[test]
fn nothing_outside_the_seam_names_a_filesystem() {
    let mut files = Vec::new();
    sources(&root().join("src"), &mut files);
    assert!(
        files.len() > 10,
        "the scan found {} source files, so it is not reading the crate",
        files.len()
    );

    let mut reaching = Vec::new();
    let mut seam_found = false;
    for relative in &files {
        let name = relative.to_string_lossy().into_owned();
        let source = fs::read_to_string(root().join(relative)).expect("a readable source file");
        let shipped = shipped_code(&source);

        if is_seam(&name) {
            seam_found = true;
            continue;
        }
        for (line_number, line) in shipped.lines().enumerate() {
            for forbidden in FORBIDDEN {
                if line.contains(forbidden) {
                    reaching.push(format!("{name}:{}: {line}", line_number + 1));
                }
            }
        }
    }

    assert!(
        seam_found,
        "the scan never reached {SEAM}.rs, so it is looking in the wrong place"
    );
    assert!(
        reaching.is_empty(),
        "a module reaches around the filesystem seam — ask crate::fs for the \
         operation instead (specs/modules/fs.md):\n  {}",
        reaching.join("\n  ")
    );
}

#[test]
fn the_seam_itself_is_where_a_filesystem_is_named() {
    // The other half of the rule: the search above must be able to find these
    // names at all, or an empty answer would mean nothing.
    let seam = fs::read_to_string(root().join(format!("{SEAM}.rs"))).expect("the seam module");
    let native =
        fs::read_to_string(root().join(format!("{SEAM}/native.rs"))).expect("the native backend");

    assert!(
        ["std::fs", "tempfile", "filetime"]
            .iter()
            .all(|name| native.contains(name)),
        "the native backend is the one place these crates are used"
    );
    assert!(
        seam.contains("mod native") && seam.contains("mod wasm"),
        "the seam selects a backend by cfg, so both halves exist"
    );
}
