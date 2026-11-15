//! The spec graph against the code it governs.
//!
//! `specs/modules/<module>.md` declares `depends-on`, and AGENTS.md makes module
//! ownership load-bearing. Prose alone cannot hold the two in step: this slice
//! added `use crate::jpeg` to `previews_db` and left its `depends-on` saying
//! `snapshot, model`, which `spec_validate` cannot see because the link it checks
//! resolves. So the agreement is asserted here, mechanically.
//!
//! Two properties, both cheap:
//!
//! 1. **Every `crate::<module>` a shipped module reaches is declared** in that
//!    module's `depends-on`. A used-but-undeclared dependency is the bug above.
//! 2. **Every shipped module has a spec.** A new module with no design decision
//!    recorded is the same class of surprise, one commit earlier.
//!
//! `error` is exempt: it is the crate's shared error type, imported by nearly
//! every module and declared by none, so listing it everywhere would be noise
//! rather than a decision. Everything else has to be named.

use std::fs;
use std::path::{Path, PathBuf};

/// The one module whose dependency is crate-wide rather than a design seam.
const SHARED: &str = "error";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn sources() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(root().join("src"))
        .expect("the crate's source directory")
        .map(|entry| entry.expect("a readable entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .collect();
    found.sort();
    found
}

/// The module a source file defines: its file stem, with `lib` as the crate root.
fn module_name(path: &Path) -> String {
    path.file_stem()
        .expect("a source file has a stem")
        .to_string_lossy()
        .into_owned()
}

/// Every module the *shipped* code reaches through `crate::`, comments and test
/// modules excluded.
///
/// Both forms count: `use crate::{jpeg, lrprev}` and a fully-qualified
/// `crate::snapshot::open_readonly(..)`. The inline form is the one this repo
/// prefers, so a scan of `use` statements alone would see almost nothing.
fn dependencies(path: &Path, modules: &[String]) -> Vec<String> {
    let source = fs::read_to_string(path).expect("a readable source file");
    let source = strip_comments_and_strings(&source);
    // The test module is not the module's design: it may reach anything.
    let source = source
        .split("#[cfg(test)]")
        .next()
        .expect("a split always yields a first part");

    let mut found: Vec<String> = Vec::new();
    for occurrence in source.match_indices("crate::") {
        let rest = &source[occurrence.0 + "crate::".len()..];
        let names: Vec<String> = match rest.strip_prefix('{') {
            // `crate::{a, b}`: every name in the group.
            Some(group) => group
                .split('}')
                .next()
                .unwrap_or_default()
                .split(',')
                .filter_map(identifier)
                .collect(),
            // `crate::a::b`: the first segment.
            None => identifier(rest).into_iter().collect(),
        };
        for name in names {
            if name != SHARED && modules.contains(&name) && !found.contains(&name) {
                found.push(name);
            }
        }
    }
    found.sort();
    found
}

/// The leading identifier of `text`, if it starts with one.
fn identifier(text: &str) -> Option<String> {
    let name: String = text
        .chars()
        .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
        .collect();
    (!name.is_empty() && !name.starts_with(|ch: char| ch.is_ascii_digit())).then_some(name)
}

/// Drop `//` and `/* */` comments and the contents of string and char
/// literals, so a mention in prose is not read as a dependency.
///
/// A char literal is three or four bytes (`'x'`, `'\''`, `'\\'`), which is what
/// separates it from a lifetime (`'a`) — the repo has both.
fn strip_comments_and_strings(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        match ch {
            '/' if chars.get(index + 1) == Some(&'/') => {
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                }
            }
            '/' if chars.get(index + 1) == Some(&'*') => {
                index += 2;
                while index < chars.len()
                    && !(chars[index] == '*' && chars.get(index + 1) == Some(&'/'))
                {
                    index += 1;
                }
                index += 2;
            }
            '"' => {
                index += 1;
                while index < chars.len() && chars[index] != '"' {
                    if chars[index] == '\\' {
                        index += 1;
                    }
                    index += 1;
                }
                index += 1;
            }
            '\'' => {
                // A lifetime (`'a`) skips only the quote; a char literal skips all
                // of itself.
                index += char_literal_len(&chars[index..]).unwrap_or(1);
            }
            _ => {
                out.push(ch);
                index += 1;
            }
        }
    }
    out
}

/// How long the char literal starting at `chars[0]` is, or `None` for a lifetime.
fn char_literal_len(chars: &[char]) -> Option<usize> {
    match (chars.get(1), chars.get(2), chars.get(3)) {
        // `'\''` and `'\\'`: an escaped quote or backslash.
        (Some('\\'), _, Some('\'')) => Some(4),
        // `'x'`.
        (Some(_), Some('\''), _) => Some(3),
        _ => None,
    }
}

/// One module spec: its id and the dependencies it declares.
struct Spec {
    id: String,
    path: PathBuf,
    depends_on: Vec<String>,
}

fn specs() -> Vec<Spec> {
    let mut found: Vec<Spec> = fs::read_dir(root().join("../../specs/modules"))
        .expect("the spec graph's module folder")
        .map(|entry| entry.expect("a readable entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .map(|path| {
            let text = fs::read_to_string(&path).expect("a readable spec");
            parse_spec(&path, &text)
        })
        .collect();
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found
}

fn parse_spec(path: &Path, text: &str) -> Spec {
    let frontmatter = text
        .strip_prefix("---")
        .and_then(|rest| rest.split("\n---").next())
        .unwrap_or_default();

    let mut id = String::new();
    let mut depends_on = Vec::new();
    let mut reading = false;
    for line in frontmatter.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(value) = line.strip_prefix("id: ") {
            id = value.trim().to_string();
            continue;
        }
        if line.starts_with("depends-on:") {
            reading = true;
            continue;
        }
        match line.strip_prefix("  - ") {
            Some(value) if reading => depends_on.push(value.trim().replace('-', "_")),
            // Any other indentation or key ends the list.
            _ => reading = false,
        }
    }
    Spec {
        id,
        path: path.to_path_buf(),
        depends_on,
    }
}

#[test]
fn every_module_reaches_only_the_dependencies_its_spec_declares() {
    let modules: Vec<String> = sources().iter().map(|path| module_name(path)).collect();
    let specs = specs();

    let mut undeclared = Vec::new();
    for path in sources() {
        let module = module_name(&path);
        if module == "lib" {
            continue;
        }
        let spec_id = module.replace('_', "-");
        let Some(spec) = specs.iter().find(|spec| spec.id == spec_id) else {
            // The coverage test below names the missing spec; nothing to compare.
            continue;
        };
        for dependency in dependencies(&path, &modules) {
            if !spec.depends_on.contains(&dependency) {
                undeclared.push(format!(
                    "{module} reaches crate::{dependency}, but {} declares depends-on: {}",
                    spec.path.display(),
                    spec.depends_on.join(", ")
                ));
            }
        }
    }

    assert!(
        undeclared.is_empty(),
        "a module's spec must declare every module it reaches — add the missing entry under \
         `depends-on:` in the named spec:\n  {}",
        undeclared.join("\n  ")
    );
}

#[test]
fn every_module_has_a_spec() {
    let specs = specs();
    let missing: Vec<String> = sources()
        .into_iter()
        .map(|path| module_name(&path))
        .filter(|module| module != "lib")
        .filter(|module| {
            let id = module.replace('_', "-");
            !specs.iter().any(|spec| spec.id == id)
        })
        .map(|module| {
            format!(
                "src/{module}.rs has no specs/modules/{}.md",
                module.replace('_', "-")
            )
        })
        .collect();

    assert!(
        missing.is_empty(),
        "a module with no spec has no recorded boundary or decision:\n  {}",
        missing.join("\n  ")
    );
}
