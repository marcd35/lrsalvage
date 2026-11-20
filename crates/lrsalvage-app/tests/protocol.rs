//! One home for the bridge's two code lists, held by a test.
//!
//! The protocol is `crates/lrsalvage-core/src/fs/bridge.rs` on one side and
//! `crates/lrsalvage-app/web/seam/protocol.js` on the other: two implementations,
//! kept in step by reading them. A review once found the third copy this test
//! exists to prevent — the page's host had its own `OP_*`/`KIND_*`, so an
//! operation added in the two named places would have answered `Unsupported` in
//! the third, and the spec would have said there was nowhere else to look.
//!
//! So this is `tests/fs_seam.rs` and `lrsalvage-cli`'s `tests/links.rs` for the
//! browser half: a source scan where the alternative is a reviewer's eye. The
//! numbers have to agree as well as the names — a code that drifted by one is the
//! same bug with a better disguise. It lives here because the JavaScript half does
//! (`#149`); the scan reaches the harness's page too, because a code defined there
//! would be the same third copy in a different directory.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// `const OP_READ: u32 = 0;` in Rust, `export const OP_READ = 0;` in JavaScript.
fn definitions(path: &Path) -> BTreeMap<String, u32> {
    let source =
        fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let mut found = BTreeMap::new();
    for line in source.lines() {
        let line = line.trim_start();
        let line = line.strip_prefix("export ").unwrap_or(line);
        // `const` in JavaScript, `pub(crate) const` in Rust: the visibility is
        // not part of the list.
        let line = line
            .strip_prefix("pub(crate) ")
            .or_else(|| line.strip_prefix("pub "))
            .unwrap_or(line);
        let Some(rest) = line.strip_prefix("const ") else {
            continue;
        };
        let Some((name, value)) = rest.split_once('=') else {
            continue;
        };
        // `OP_READ: u32` in Rust, `OP_READ` in JavaScript: the type is not ours.
        let name = name.split(':').next().unwrap_or(name).trim();
        if !(name.starts_with("OP_") || name.starts_with("KIND_")) {
            continue;
        }
        let value = value.trim().trim_end_matches(';').trim();
        let value: u32 = value
            .parse()
            .unwrap_or_else(|_| panic!("{}: `{name}` is not a number: `{value}`", path.display()));
        found.insert(name.to_string(), value);
    }
    found
}

/// This crate's `web/`, which holds the protocol.
fn web_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("web")
}

/// The Rust half of the protocol, in the crate the browser half answers. A
/// sibling path rather than a dependency on its internals: the file is the
/// authority, and both browser crates are workspace members that are never
/// packaged.
fn bridge_file() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../lrsalvage-core/src/fs/bridge.rs")
}

/// Every JavaScript file either page can load, `pkg/` excepted — that is
/// `wasm-bindgen`'s generated glue, which defines no protocol code.
fn javascript_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in [
        web_dir(),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../lrsalvage-web/web"),
    ] {
        for entry in walkdir::WalkDir::new(&root) {
            let entry = entry.expect("walk a web directory");
            if entry
                .path()
                .components()
                .any(|component| component.as_os_str() == "pkg")
            {
                continue;
            }
            if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "js")
            {
                files.push(entry.path().to_path_buf());
            }
        }
    }
    files
}

#[test]
fn the_rust_and_javascript_code_lists_are_the_same_list() {
    let rust = definitions(&bridge_file());
    let js = definitions(&web_dir().join("seam/protocol.js"));

    assert!(!rust.is_empty(), "bridge.rs defines no operation codes");
    assert_eq!(
        rust.keys().collect::<Vec<_>>(),
        js.keys().collect::<Vec<_>>(),
        "the two files must name the same codes"
    );
    for (name, value) in &rust {
        assert_eq!(
            js.get(name),
            Some(value),
            "{name} is {value} in bridge.rs and {:?} in protocol.js",
            js.get(name)
        );
    }
}

#[test]
fn no_other_javascript_file_defines_a_code() {
    let protocol = fs::canonicalize(web_dir().join("seam/protocol.js")).expect("the protocol");
    let mut checked = 0;
    for path in javascript_files() {
        if fs::canonicalize(&path).expect("a javascript file") == protocol {
            continue;
        }
        checked += 1;
        let found = definitions(&path);
        assert!(
            found.is_empty(),
            "{} defines {:?}; the codes live in web/seam/protocol.js and bridge.rs",
            path.display(),
            found.keys().collect::<Vec<_>>()
        );
    }
    assert!(
        checked >= 6,
        "the scan found {checked} JS files; is the path right?"
    );
}
