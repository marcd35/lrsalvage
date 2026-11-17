//! The spec graph against the code it governs.
//!
//! `specs/modules/<module>.md` declares `depends-on`, and AGENTS.md makes module
//! ownership load-bearing. Prose alone cannot hold the two in step: this slice
//! added `use crate::jpeg` to `previews_db` and left its `depends-on` saying
//! `snapshot, model`, which `spec_validate` cannot see because the link it checks
//! resolves. So the agreement is asserted here, mechanically.
//!
//! Two properties about the code, both cheap:
//!
//! 1. **Every `crate::<module>` a shipped module reaches is declared** in that
//!    module's `depends-on`. A used-but-undeclared dependency is the bug above.
//! 2. **Every shipped module has a spec.** A new module with no design decision
//!    recorded is the same class of surprise, one commit earlier.
//!
//! And three about the graph itself, which are `spec_validate`'s job as well —
//! but that is an in-app tool, so CI and a checkout without the spec tools cannot
//! ask it, and AGENTS.md's "a slice that re-links a spec ends on a clean graph"
//! rests on an agent remembering. These read the same files and answer the same
//! three questions:
//!
//! 3. **Every id is unique.** Two specs sharing one is a link that resolves to
//!    whichever file was read first.
//! 4. **Every link resolves** — `parent` and each of `depends-on`, `references`,
//!    `implements`, `covers`. A dangling target is a decision nobody can reach.
//! 5. **No spec is its own ancestor.** A `parent` loop has no root to read from.
//!
//! `error` is exempt: it is the crate's shared error type, imported by nearly
//! every module and declared by none, so listing it everywhere would be noise
//! rather than a decision. Everything else has to be named.

use std::collections::{BTreeSet, HashMap};
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
        if let Some(value) = line.strip_prefix("depends-on:") {
            reading = true;
            // The spec editor writes a flow list (`depends-on: [a, b]`) while a
            // hand edit writes the block form below; one meaning, two shapes, and
            // this reader knows both. Reading only the block form made a
            // doc-only edit fail this gate with an empty dependency list.
            if let Some(flow) = value.trim().strip_prefix('[') {
                depends_on.extend(
                    flow.trim_end_matches(']')
                        .split(',')
                        .map(str::trim)
                        .filter(|entry| !entry.is_empty())
                        .map(|entry| entry.replace('-', "_")),
                );
                reading = false;
            }
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
fn a_flow_style_depends_on_reads_like_a_block_one() {
    let block =
        "---\nid: plan\ndepends-on:\n  - previews-db\n  - model\nreferences:\n  - adr-0001\n---\n";
    let flow = "---\nid: plan\ndepends-on: [previews-db, model]\nreferences: [adr-0001]\n---\n";

    let read = |text: &str| parse_spec(Path::new("plan.md"), text);
    assert_eq!(read(block).depends_on, vec!["previews_db", "model"]);
    assert_eq!(
        read(flow).depends_on,
        read(block).depends_on,
        "the spec editor's flow list and a hand-edited block list mean one thing"
    );
    assert_eq!(read(flow).id, "plan");
    assert!(
        read("---\nid: plan\ndepends-on: []\n---\n")
            .depends_on
            .is_empty(),
        "an empty flow list is still an empty list"
    );
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

/// The frontmatter list fields whose entries are ids of other specs.
const LINK_FIELDS: [&str; 4] = ["depends-on", "references", "implements", "covers"];

/// One file in the spec graph: the id it declares, the parent it names, and the
/// ids its list fields point at.
///
/// Unlike [`Spec`], every id is kept verbatim. `plan` names a Rust module and
/// `adr-0006-degrade-without-guessing` names a file, and a link is this graph's
/// edge only when it is the id it resolves to.
struct Node {
    id: String,
    path: PathBuf,
    parent: Option<String>,
    links: Vec<(&'static str, String)>,
}

/// Every indexed spec in the repository, found the way `spec_validate` finds
/// them: a markdown file whose frontmatter carries an `id`.
///
/// Not only `specs/`: `CONTEXT.md`, `docs/format-notes.md` and `docs/adr/*.md`
/// carry ids too, and links cross between them — `adr-0006-degrade-without-guessing`
/// is referenced from a module spec — so a reader that stopped at `specs/modules`
/// would call every ADR reference dangling.
fn indexed_specs() -> Vec<Node> {
    let mut found = Vec::new();
    let repo = root().join("../..");
    let mut stack = vec![repo.clone()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries {
            let path = entry.expect("a readable entry").path();
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if path.is_dir() {
                // `target/` holds the crate's own sources again among thousands of
                // build artefacts, and a dot directory is not ours to read.
                if name != "target" && !name.starts_with('.') {
                    stack.push(path);
                }
            } else if name.ends_with(".md") {
                let text = fs::read_to_string(&path).expect("a readable markdown file");
                // Repository-relative, so a failure names `CONTEXT.md` rather than
                // this machine's checkout of it.
                let relative = path.strip_prefix(&repo).unwrap_or(&path).to_path_buf();
                if let Some(node) = parse_node(&relative, &text) {
                    found.push(node);
                }
            }
        }
    }
    found.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.path.cmp(&b.path)));
    found
}

fn parse_node(path: &Path, text: &str) -> Option<Node> {
    let frontmatter = text
        .strip_prefix("---")
        .and_then(|rest| rest.split("\n---").next())
        .unwrap_or_default();

    let mut id = String::new();
    let mut parent = None;
    let mut links: Vec<(&'static str, String)> = Vec::new();
    let mut reading: Option<&'static str> = None;
    for line in frontmatter.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(value) = line.strip_prefix("id: ") {
            id = value.trim().to_string();
            reading = None;
            continue;
        }
        if let Some(value) = line.strip_prefix("parent: ") {
            parent = Some(value.trim().to_string());
            reading = None;
            continue;
        }
        // One meaning, two shapes: the spec editor writes a flow list, a hand
        // edit writes the block form. `parse_spec` above knows both for
        // `depends-on`, and a link check that knew one would miss half the graph.
        let mut handled = false;
        for field in LINK_FIELDS {
            let Some(value) = line
                .strip_prefix(field)
                .and_then(|rest| rest.strip_prefix(':'))
            else {
                continue;
            };
            handled = true;
            let value = value.trim();
            match value.strip_prefix('[') {
                Some(flow) => {
                    links.extend(
                        flow.trim_end_matches(']')
                            .split(',')
                            .map(str::trim)
                            .filter(|entry| !entry.is_empty())
                            .map(|entry| (field, entry.to_string())),
                    );
                    reading = None;
                }
                None => reading = Some(field),
            }
        }
        if handled {
            continue;
        }
        match line.strip_prefix("  - ") {
            Some(value) => {
                if let Some(field) = reading {
                    links.push((field, value.trim().to_string()));
                }
            }
            None => reading = None,
        }
    }

    (!id.is_empty()).then(|| Node {
        id,
        path: path.to_path_buf(),
        parent,
        links,
    })
}

#[test]
fn no_two_specs_share_an_id() {
    let nodes = indexed_specs();
    assert!(
        nodes.len() > 20,
        "the walk found {} specs, so it is not reading the graph at all",
        nodes.len()
    );

    let mut seen: Vec<(&str, &Path)> = Vec::new();
    let mut duplicates = Vec::new();
    for node in &nodes {
        match seen.iter().find(|(id, _)| *id == node.id) {
            Some((_, first)) => duplicates.push(format!(
                "`{}` is declared by {} and by {}",
                node.id,
                first.display(),
                node.path.display()
            )),
            None => seen.push((&node.id, &node.path)),
        }
    }

    assert!(
        duplicates.is_empty(),
        "two specs with one id leave every link to it ambiguous:\n  {}",
        duplicates.join("\n  ")
    );
}

#[test]
fn every_link_in_the_spec_graph_resolves() {
    let nodes = indexed_specs();
    let ids: BTreeSet<&str> = nodes.iter().map(|node| node.id.as_str()).collect();

    let mut dangling = Vec::new();
    for node in &nodes {
        let edges = node
            .parent
            .iter()
            .map(|parent| ("parent", parent))
            .chain(node.links.iter().map(|(field, target)| (*field, target)));
        for (field, target) in edges {
            if !ids.contains(target.as_str()) {
                dangling.push(format!(
                    "{}: `{field}: {target}` names no id",
                    node.path.display()
                ));
            }
        }
    }

    assert!(
        dangling.is_empty(),
        "a link that resolves to nothing is a decision no reader can reach:\n  {}",
        dangling.join("\n  ")
    );
}

#[test]
fn no_spec_is_its_own_ancestor() {
    let nodes = indexed_specs();
    let by_id: HashMap<&str, &Node> = nodes.iter().map(|node| (node.id.as_str(), node)).collect();

    let mut cycles = Vec::new();
    for node in &nodes {
        let mut chain = vec![node.id.as_str()];
        let mut current = node;
        while let Some(parent) = current
            .parent
            .as_deref()
            .and_then(|parent| by_id.get(parent))
        {
            if chain.contains(&parent.id.as_str()) {
                chain.push(parent.id.as_str());
                cycles.push(format!(
                    "{} closes: {}",
                    node.path.display(),
                    chain.join(" -> ")
                ));
                break;
            }
            chain.push(parent.id.as_str());
            current = parent;
        }
    }

    assert!(
        cycles.is_empty(),
        "a spec that is its own ancestor has no root to read up to:\n  {}",
        cycles.join("\n  ")
    );
}
