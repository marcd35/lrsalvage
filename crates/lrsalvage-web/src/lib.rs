//! The browser parity harness: one vault, the same modules, two targets.
//!
//! `#144` is the slice that gives the `fs` seam a browser implementation, and the
//! claim it has to make is not "it compiles" but "it answers what the desktop
//! answers". So this crate holds **one** driver — [`run`] — that scans a vault and
//! extracts its previews through `lrsalvage_core`'s own modules, and compiles it
//! twice: natively, where the seam is `std::fs`, and to `wasm32`, where the seam
//! is `lrsalvage_core::fs::wasm` — the browser backend, which exists only on that
//! target — over an origin-private store and a picked folder. The numbers go into
//! one JSON document on each side and are compared
//! with `MAIN_SEPARATOR` normalised (`docs/adr/0012-two-release-lines.md` D12).
//!
//! [`probe`] is the other half: the same walk over the seam's operations that
//! `crates/lrsalvage-core/src/fs.rs`'s own tests make, run on both targets, so the
//! browser backend is held to the specification #142 wrote rather than to a new
//! one.
//!
//! Nothing here is a gate: `scripts/browser-parity.sh` drives it through headless
//! Chromium, and `specs/modules/fs.md` (*Where the harness lives*) records why that
//! is a measurement rather than a gate — and what a machine needs to run it.

use std::path::{Path, PathBuf};

use lrsalvage_core::catalog::Catalog;
use lrsalvage_core::extract::{self, ExtractOptions};
use lrsalvage_core::fs;
use lrsalvage_core::plan::{Options, Plan};
use lrsalvage_core::previews_db;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// One scan-and-extract run, reported as the numbers a second target has to meet.
///
/// The three paths are the only input, which is what makes the driver the same
/// code on both targets: a mount root in a page, a directory on a disk.
///
/// The index is copied into a scratch directory before it is opened, so the
/// natively-run comparison does the same thing a browser has to do — SQLite can
/// only open a file the seam put somewhere it can reach — and so neither side
/// opens the vault's own `previews.db` in place.
pub fn run(catalog: &Path, cache: &Path, output: &Path) -> Result<Value, String> {
    (|| -> Result<Value, String> {
        extract::ensure_safe_destination_create(output, catalog, cache)
            .map_err(|error| format!("destination: {error}"))?;

        let (snapshot, catalog_db) =
            Catalog::open_verified(catalog).map_err(|error| format!("catalog: {error}"))?;
        let digests = snapshot.digests();

        let index_dir = fs::scratch_dir("lrsalvage-parity-index-")
            .map_err(|error| format!("an index scratch directory: {error}"))?;
        let index_copy = index_dir.path().join(previews_db::INDEX_FILE);
        let index_present = match fs::copy(&cache.join(previews_db::INDEX_FILE), &index_copy) {
            Ok(()) => true,
            // An indexless vault is a mode, not a failure (`adr-0006`), so a
            // missing index is reported and the plan degrades.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(format!("the index copy: {error}")),
        };
        let opened = previews_db::open(&index_copy);

        let plan = Plan::build(
            &catalog_db,
            &opened.index,
            cache,
            output,
            &Options::default(),
        )
        .map_err(|error| format!("plan: {error}"))?;

        let options = ExtractOptions {
            output_root: output.to_path_buf(),
            ..ExtractOptions::default()
        };
        let outcomes = extract::extract(&plan, &options, &mut |_outcome| {})
            .map_err(|error| format!("extract: {error}"))?;

        let mut written = Vec::new();
        for outcome in &outcomes {
            let relative = outcome
                .output
                .strip_prefix(output)
                .unwrap_or(&outcome.output)
                .to_string_lossy()
                .replace('\\', "/");
            written.push(json!({
                "path": relative,
                "status": outcome.status.as_str(),
                "bytes": outcome.bytes,
                "sha256": outcome.sha256,
            }));
        }
        written.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));

        let mut statuses: Vec<String> = plan
            .items
            .iter()
            .map(|item| {
                let image = item
                    .image
                    .as_ref()
                    .map(|image| image.id_local.to_string())
                    .unwrap_or_else(|| "-".to_string());
                format!("{image} {}", item.status.as_str())
            })
            .collect();
        statuses.sort();

        let stats = &plan.stats;
        Ok(json!({
            "separator": std::path::MAIN_SEPARATOR.to_string(),
            "index_present": index_present,
            "index_problem": opened.problem.as_ref().map(|problem| format!("{problem:?}")),
            "source_sha256": digests.map(|digests| digests.source().to_string()),
            "snapshot_sha256": digests.map(|digests| digests.snapshot().to_string()),
            "stats": {
                "images": stats.images,
                "recoverable": stats.recoverable,
                "missing_payload": stats.missing_payload,
                "no_cache_entry": stats.no_cache_entry,
                "unmapped": stats.unmapped,
                "suspect": stats.suspect,
                "failed": stats.failed,
                "original_present": stats.original_present,
            },
            "plan_items": plan.items.len(),
            "plan_selected": plan.items.iter().filter(|item| item.selected).count(),
            "plan_statuses": statuses,
            "plan_statuses_sha256": digest_of(&statuses),
            "written": written.len(),
            "outputs": written,
            "outputs_sha256": digest_of(
                &written
                    .iter()
                    .map(|entry| format!(
                        "{} {} {}",
                        entry["path"].as_str().unwrap_or_default(),
                        entry["status"].as_str().unwrap_or_default(),
                        entry["sha256"].as_str().unwrap_or_default(),
                    ))
                    .collect::<Vec<_>>()
            ),
        }))
    })()
}

/// A digest over the newline-joined lines, in the order given.
///
/// Sorted by the caller: two targets must agree on what they produced, not on the
/// order a walk handed it back in.
fn digest_of(lines: &[String]) -> String {
    let mut hasher = Sha256::new();
    for line in lines {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// One seam check's outcome, and whether the two targets are allowed to differ.
#[derive(Clone)]
pub struct Check {
    pub name: &'static str,
    pub outcome: String,
    /// True where a target is *expected* to answer differently — the one
    /// divergence this backend records rather than hides.
    pub diverges: bool,
}

fn check(name: &'static str, result: std::io::Result<bool>, ok: &str, not_ok: &str) -> Check {
    let outcome = match result {
        Ok(true) => ok.to_string(),
        Ok(false) => not_ok.to_string(),
        Err(error) => format!("error:{:?}", error.kind()),
    };
    Check {
        name,
        outcome,
        diverges: false,
    }
}

/// The seam's own tests, on whichever target this is.
///
/// `dir` is a writable mount and `store` is the origin-private one; natively both
/// are scratch directories, so the same calls run in the same order and the two
/// targets' outcomes can be compared line by line. `set_mtime` is the recorded
/// divergence: it answers natively and is refused in a browser, which is what
/// `specs/modules/fs.md` says it does.
pub fn probe(dir: &Path, store: &Path) -> Vec<Check> {
    let mut checks = Vec::new();
    let whole = dir.join("whole.bin");
    let head = dir.join("head.bin");

    checks.push(check(
        "write_then_read_every_byte",
        (|| {
            fs::write(&whole, b"the whole file")?;
            Ok(fs::read(&whole)? == b"the whole file" && fs::read(&dir.join("absent")).is_err())
        })(),
        "ok",
        "mismatch",
    ));

    checks.push(check(
        "read_prefix_stops_at_the_length",
        (|| {
            fs::write(&head, b"\xFF\xD8rest")?;
            Ok(fs::read_prefix(&head, 2)? == b"\xFF\xD8"
                && fs::read_prefix(&head, 4096)? == b"\xFF\xD8rest")
        })(),
        "ok",
        "mismatch",
    ));

    checks.push(check(
        "exists_separates_a_file_a_directory_and_nothing",
        (|| {
            Ok(fs::exists(&whole)?
                && fs::is_file(&whole)?
                && fs::exists(dir)?
                && !fs::is_file(dir)?
                && !fs::exists(&dir.join("absent"))?
                && !fs::is_file(&dir.join("absent"))?)
        })(),
        "ok",
        "mismatch",
    ));

    checks.push(check(
        "list_files_answers_one_directory",
        (|| {
            let listing = dir.join("listing");
            let nested = listing.join("nested");
            fs::create_dir_all(&nested)?;
            fs::write(&listing.join("a.bin"), b"a")?;
            fs::write(&listing.join("b.bin"), b"b")?;
            fs::write(&nested.join("c.bin"), b"c")?;
            let mut names = file_names(fs::list_files(&listing)?);
            names.sort();
            Ok(names == vec!["a.bin".to_string(), "b.bin".to_string()]
                && fs::list_files(&dir.join("absent")).is_err())
        })(),
        "ok",
        "mismatch",
    ));

    checks.push(check(
        "walk_files_answers_every_descendent",
        (|| {
            let root = dir.join("tree");
            let deep = root.join("one").join("two");
            fs::create_dir_all(&deep)?;
            fs::write(&root.join("top.bin"), b"t")?;
            fs::write(&root.join("one").join("mid.bin"), b"m")?;
            fs::write(&deep.join("deep.bin"), b"d")?;
            let mut names = file_names(fs::walk_files(&root)?);
            names.sort();
            Ok(names
                == vec![
                    "deep.bin".to_string(),
                    "mid.bin".to_string(),
                    "top.bin".to_string(),
                ]
                && fs::walk_files(&dir.join("absent")).is_err())
        })(),
        "ok",
        "mismatch",
    ));

    checks.push(check(
        "create_dir_all_is_idempotent",
        (|| {
            let nested = dir.join("a").join("b").join("c");
            fs::create_dir_all(&nested)?;
            fs::create_dir_all(&nested)?;
            fs::exists(&nested)
        })(),
        "ok",
        "mismatch",
    ));

    checks.push(check(
        "create_new_refuses_a_taken_path",
        (|| {
            let path = dir.join("claimed.bin");
            let _ = fs::remove_file(&path);
            fs::create_new(&path)?;
            let empty = fs::read(&path)?.is_empty();
            fs::write(&path, b"somebody's bytes")?;
            let refused = fs::create_new(&path)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::AlreadyExists);
            Ok(empty && refused && fs::read(&path)? == b"somebody's bytes")
        })(),
        "ok",
        "mismatch",
    ));

    checks.push(check(
        "write_atomic_leaves_no_staged_sibling",
        (|| {
            let path = dir.join("atomic.bin");
            fs::write(&path, b"the old contents")?;
            fs::write_atomic(&path, b"the new contents")?;
            let names = file_names(fs::list_files(dir)?);
            let only_ours = names
                .iter()
                .filter(|name| name.starts_with("atomic"))
                .count()
                == 1;
            Ok(fs::read(&path)? == b"the new contents" && only_ours)
        })(),
        "ok",
        "mismatch",
    ));

    checks.push(check(
        "remove_file_errors_on_a_second_call",
        (|| {
            let path = dir.join("removed.bin");
            fs::write(&path, b"x")?;
            fs::remove_file(&path)?;
            Ok(!fs::exists(&path)? && fs::remove_file(&path).is_err())
        })(),
        "ok",
        "mismatch",
    ));

    checks.push(check(
        "copy_carries_the_bytes_across_mounts",
        (|| {
            let from = dir.join("from.bin");
            let to = store.join("copied.bin");
            fs::write(&from, b"the catalog as it was")?;
            fs::copy(&from, &to)?;
            let source = b"the catalog as it was";
            Ok(fs::read(&to)? == source && fs::read(&from)? == source)
        })(),
        "ok",
        "mismatch",
    ));

    checks.push(check(
        "sha256_file_is_the_digest_of_the_bytes",
        (|| {
            let path = dir.join("hashed.bin");
            fs::write(&path, b"abc")?;
            Ok(fs::sha256_file(&path)?
                == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        })(),
        "ok",
        "mismatch",
    ));

    let nested = dir.join("canon").join("b");
    checks.push(Check {
        name: "canonicalize_resolves_a_relative_spelling",
        outcome: match (|| {
            fs::create_dir_all(&nested)?;
            let root = fs::canonicalize(dir)?;
            let direct = fs::canonicalize(&nested)?;
            let roundabout = fs::canonicalize(&nested.join(".").join("..").join("b"))?;
            Ok::<_, std::io::Error>((root, direct, roundabout))
        })() {
            // The promise is one directory, one spelling, under the directory that
            // was asked about. Not that `Path::is_absolute` agrees: that is a
            // target property, and `path_spelling` records it.
            Ok((root, direct, roundabout)) if direct == roundabout && direct.starts_with(&root) => {
                "ok".into()
            }
            Ok((root, direct, roundabout)) => {
                format!("root={root:?} direct={direct:?} roundabout={roundabout:?}")
            }
            Err(error) => format!("error:{:?}", error.kind()),
        },
        diverges: false,
    });

    // Free space: a number on the desktop, a refusal in a page, and the refusal is
    // the design (#117). `extract::ensure_room` turns either into "measured" or
    // "proceed unmeasured", so this is a declared divergence rather than a gap.
    checks.push(Check {
        name: "available_space_answers_or_refuses",
        outcome: match fs::available_space(dir) {
            Ok(bytes) if bytes > 0 => "answered".to_string(),
            Ok(bytes) => format!("answered:{bytes}"),
            Err(error) => format!("refused:{:?}", error.kind()),
        },
        diverges: true,
    });

    // A copy onto a store name whose `-wal` companion exists is refused on the
    // target and carried out on the desktop: the pool will not publish an image a
    // pending WAL describes, while `std::fs::copy` would overwrite the main file
    // and leave the stale sidecar beside it. Declared, because the two targets
    // answer differently on purpose (`specs/modules/fs.md`).
    checks.push(Check {
        name: "store_copy_refuses_a_sidecar_name",
        outcome: (|| {
            let main = store.join("sidecar-refusal.bin");
            let sidecar = store.join("sidecar-refusal.bin-wal");
            let source = dir.join("sidecar-refusal-source.bin");
            fs::write(&main, b"an image a WAL describes")?;
            fs::write(&sidecar, b"frames")?;
            fs::write(&source, b"new bytes")?;
            let outcome = match fs::copy(&source, &main) {
                Ok(()) => "copied".to_string(),
                Err(error) => format!("refused:{:?}", error.kind()),
            };
            // Leave nothing behind either way: a later check reads this store.
            let _ = fs::remove_file(&main);
            let _ = fs::remove_file(&sidecar);
            let _ = fs::remove_file(&source);
            Ok::<_, std::io::Error>(outcome)
        })()
        .unwrap_or_else(|error| format!("error:{:?}", error.kind())),
        diverges: true,
    });

    // What this target's `Path` makes of an absolute path, recorded rather than
    // assumed: a caller that asks `is_absolute` before anchoring a relative
    // destination (`extract::resolve_pending`) is asking the standard library a
    // question whose answer is not the same on both targets.
    let mut joined = Path::new("/").to_path_buf();
    joined.push("/a");
    checks.push(Check {
        name: "path_spelling",
        outcome: format!(
            "separator={:?} is_absolute={} join={:?}",
            std::path::MAIN_SEPARATOR,
            Path::new("/a").is_absolute(),
            joined,
        ),
        diverges: true,
    });

    // The one operation whose answer differs by design: the File System API has
    // no way to set a file's modification time, so a browser refuses and the
    // desktop does not. Reported, never hidden.
    checks.push(Check {
        name: "set_mtime_sets_the_time_or_refuses",
        outcome: match fs::set_mtime(&whole, 1_248_825_600) {
            Ok(()) => "set".to_string(),
            Err(error) => format!("refused:{:?}", error.kind()),
        },
        diverges: true,
    });

    // A scratch directory: it exists while a file is in it, and it is gone with
    // its handle. In the store a directory is a prefix, so the check asks what
    // the seam promises — the copy goes away — rather than that an empty prefix
    // is an entry, which `specs/modules/fs.md` records it is not.
    checks.push(check(
        "a_scratch_dir_goes_away_with_its_handle",
        (|| {
            let path = {
                let inner = fs::scratch_dir("lrsalvage-probe-")?;
                let path = inner.path().to_path_buf();
                fs::write(&path.join("copy.bin"), b"the catalog as it was")?;
                if !fs::exists(&path)? {
                    return Ok(false);
                }
                path
            };
            Ok(!fs::exists(&path)?)
        })(),
        "ok",
        "mismatch",
    ));

    checks
}

fn file_names(paths: Vec<PathBuf>) -> Vec<String> {
    paths
        .iter()
        .filter_map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .collect()
}

/// The probe's checks as the JSON both sides print.
pub fn probe_report(checks: &[Check]) -> Value {
    json!(
        checks
            .iter()
            .map(|check| json!({
                "name": check.name,
                "outcome": check.outcome,
                "diverges": check.diverges,
            }))
            .collect::<Vec<_>>()
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub mod vault;
#[cfg(target_arch = "wasm32")]
pub mod wasm;
