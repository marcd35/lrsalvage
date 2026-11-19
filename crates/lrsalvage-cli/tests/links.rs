//! Every relative link and heading anchor in the repository's prose resolves.
//!
//! The repository writes a lot of prose and links it together: the README into
//! `docs/`, a checklist into the ADRs and the specs, a module spec into its
//! neighbours. Both halves of a link are hand-written, and a broken one fails
//! silently — the page renders, the sentence reads, and only the reader who
//! clicks learns that the heading was renamed last release. Nothing else here
//! reads a markdown link, so this is the only thing that can.
//!
//! What this checks: a relative path that ends in `.md` exists, and an anchor —
//! in the same file or across files — matches one of the target's headings under
//! GitHub's slug rules, duplicate headings included.
//!
//! What it does not: an `https://` target (a test opens no socket, and link rot
//! is not this repository's to fix), and whether the sentence the link sits in is
//! true. A link can resolve and still mislead; that stays a reviewer's job.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The prose this repository keeps, as absolute paths and repo-relative labels.
///
/// `docs/` and `specs/` are walked; the rest are named, because a new root-level
/// page should be a decision rather than something a walk picked up.
fn prose_files(root: &Path) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    for name in [
        "README.md",
        "AGENTS.md",
        "CODING_STANDARDS.md",
        "CONTEXT.md",
    ] {
        let path = root.join(name);
        if path.is_file() {
            files.push((path, name.to_owned()));
        }
    }
    for dir in ["docs", "specs"] {
        walk(&root.join(dir), &mut files, root);
    }
    for entry in std::fs::read_dir(root.join("crates")).expect("read crates/") {
        let readme = entry.expect("crates/ entry").path().join("README.md");
        if readme.is_file() {
            let label = readme
                .strip_prefix(root)
                .expect("under the root")
                .to_string_lossy()
                .replace('\\', "/");
            files.push((readme, label));
        }
    }
    files.sort_by(|a, b| a.1.cmp(&b.1));
    files
}

fn walk(dir: &Path, files: &mut Vec<(PathBuf, String)>, root: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            walk(&path, files, root);
        } else if path.extension().is_some_and(|ext| ext == "md") {
            let label = path
                .strip_prefix(root)
                .expect("under the root")
                .to_string_lossy()
                .replace('\\', "/");
            files.push((path, label));
        }
    }
}

/// GitHub's heading anchor: lowercased, punctuation dropped, spaces hyphenated.
fn slug(heading: &str) -> String {
    let mut out = String::with_capacity(heading.len());
    for ch in heading.trim().to_lowercase().chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == ' ' {
            out.push(ch);
        }
    }
    out.replace(' ', "-")
}

/// Every anchor a file offers, with GitHub's `-1`, `-2` suffixes for repeats.
fn anchors(text: &str) -> Vec<String> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix('#') else {
            continue;
        };
        let heading = rest.trim_start_matches('#').trim();
        if heading.is_empty() {
            continue;
        }
        let base = slug(heading);
        let count = seen.entry(base.clone()).or_insert(0);
        out.push(if *count == 0 {
            base.clone()
        } else {
            format!("{base}-{count}")
        });
        *count += 1;
    }
    out
}

/// Inline link targets in `text`, as `(line number, target)`, skipping fenced
/// code. Reference-style links and images are not used in this repository's
/// prose; an `![alt](target)` is still a target, so it is checked too.
fn link_targets(text: &str) -> Vec<(usize, String)> {
    let mut targets = Vec::new();
    let mut in_fence = false;
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        for (at, _) in line.match_indices("](") {
            let rest = &line[at + 2..];
            let Some(end) = rest.find(')') else {
                continue;
            };
            let target = rest[..end].trim().trim_matches('<').trim_matches('>');
            // `](target "title")` — the title is not part of the target.
            let target = target.split_whitespace().next().unwrap_or(target);
            if !target.is_empty() {
                targets.push((index + 1, target.to_owned()));
            }
        }
    }
    targets
}

#[test]
fn every_relative_link_and_heading_anchor_resolves() {
    let root = repo_root();
    let files = prose_files(&root);

    // Anchors per file, read once: a page linked to by ten others is parsed once.
    let mut anchors_by_file: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    for (path, _) in &files {
        let text = std::fs::read_to_string(path).expect("read prose file");
        anchors_by_file.insert(path.clone(), anchors(&text));
    }

    let mut failures = Vec::new();
    for (path, label) in &files {
        let text = std::fs::read_to_string(path).expect("read prose file");
        for (line, target) in link_targets(&text) {
            if target.contains("://") || target.starts_with("mailto:") {
                continue;
            }
            let (file_part, anchor) = match target.split_once('#') {
                Some((file, anchor)) => (file, Some(anchor)),
                None => (target.as_str(), None),
            };
            let destination = if file_part.is_empty() {
                path.clone()
            } else {
                let joined = path.parent().expect("a parent").join(file_part);
                if !joined.exists() {
                    failures.push(format!(
                        "{label}:{line}: `{target}` — no such file: {}",
                        joined.strip_prefix(&root).unwrap_or(&joined).display()
                    ));
                    continue;
                }
                joined
            };
            let Some(anchor) = anchor.filter(|anchor| !anchor.is_empty()) else {
                continue;
            };
            let Some(offered) = anchors_by_file.get(&destination) else {
                // A non-markdown destination is linked by path only.
                continue;
            };
            if !offered.iter().any(|candidate| candidate == anchor) {
                failures.push(format!(
                    "{label}:{line}: `{target}` — no heading in that file slugs to `{anchor}`"
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} markdown link(s) resolve to nothing:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
