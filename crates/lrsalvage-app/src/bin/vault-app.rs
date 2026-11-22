//! The native half of the app's browser run: compose the vaults, run the app's
//! own entry over each, and report what a page has to match.
//!
//! `scripts/browser-app.sh` runs this, then drives the same vaults through
//! headless Chromium with the same entry compiled to `wasm32`, and the page
//! compares its report with the one below — so the browser is held to the
//! desktop at the *entry*, not only at the module under it.
//!
//! The scan cases show the ways one catalog can be read or come back empty.
//! The review case covers #167 using a catalog with decodable JPEG bytes.
//!
//! | vault | what the page must say |
//! |---|---|
//! | `working` | images with previews to recover |
//! | `originals` | one image whose original is on disk — the recheck's premise |
//! | `no-index` | `previews.db` is missing |
//! | `unreadable-index` | `previews.db` is there and SQLite cannot read it |
//! | `no-payloads` | an index, a row, and no payload file |
//! | `unknown-layout` | payload-like files matching no shape |
//! | `not-a-catalog` | a file that is not a catalog at all |
//! | `review` | decodable previews, a decode failure and missing previews |
//!
//! The cases are compared with the page's report field by field. The
//! `originals` case is compared only after the page has been handed the
//! folder — before that the two targets *must* disagree, and
//! `lrsalvage_app::vault::Compare` is where that is said.
//!
//! ```text
//! cargo run -p lrsalvage-app --bin vault-app -- <out-dir>
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use lrsalvage_app::vault::{Compare, copy_tree, export, vaults};
use lrsalvage_core::plan::Control;
use lrsalvage_fixtures::review;
use lrsalvage_fixtures::vault::{Level, PyramidRow, Vault};
use serde_json::json;

fn main() {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            eprintln!("usage: vault-app <out-dir>");
            std::process::exit(2);
        });
    fs::create_dir_all(&out).expect("create the output directory");

    let mut reported = Vec::new();
    let mut cases = vaults();
    cases.push(("review", Compare::Full, review::vault));
    cases.push(("export", Compare::Full, export));
    for (name, compare, build) in cases {
        let root = out.join(name);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create a vault directory");

        let vault = build().write();
        let files = copy_tree(vault.dir(), &root);
        let relative = |path: &Path| {
            path.strip_prefix(vault.dir())
                .expect("a path inside the vault")
                .to_string_lossy()
                .replace('\\', "/")
        };
        let catalog = relative(vault.catalog());
        let cache = relative(vault.previews());

        // The app's own entry, on the host. `progress` is ignored: the native
        // half of the comparison is about the numbers, and `scripts/browser-app.sh`
        // is about whether a stop works.
        let report =
            lrsalvage_app::scan(&root.join(&catalog), &root.join(&cache), &mut |_progress| {
                lrsalvage_core::plan::Control::Continue
            })
            .unwrap_or_else(|failure| {
                json!({
                    "ok": false,
                    "problem": failure.to_json(),
                })
            });

        let mut entry = json!({
            "name": name,
            "compare": compare.as_str(),
            "files": files,
            "catalog": catalog,
            "cache": cache,
            "report": report,
        });
        if name == "review" {
            let originals = review::originals().write();
            entry["originals"] = json!(copy_tree(
                &originals.dir().join("originals"),
                &root.join("originals"),
            ));
        }
        reported.push(entry);
    }

    let document = json!({
        "target": "native",
        "separator": std::path::MAIN_SEPARATOR.to_string(),
        "vaults": reported,
        "cache_only": cache_only(&out),
    });
    let path = out.join("native.json");
    fs::write(
        &path,
        serde_json::to_vec_pretty(&document).expect("serialise the native report"),
    )
    .expect("write the native report");
    println!("{}", path.display());
}

fn cache_only(out: &Path) -> Vec<serde_json::Value> {
    [
        ("cache-legacy", recovery_vault(Vault::legacy())),
        ("cache-modern", recovery_vault(Vault::modern())),
        (
            "cache-mixed",
            recovery_vault(Vault::mixed()).container(&[Level::sized(&key(1), 1200, 800)]),
        ),
        ("cache-no-index", Vault::unindexed()),
        ("cache-bad-index", Vault::modern().raw_index(b"not SQLite")),
    ]
    .into_iter()
    .map(|(name, vault)| {
        let vault = vault.write();
        let root = out.join(name);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create an isolated recovery fixture");
        let files = copy_tree(vault.previews(), &root);
        match lrsalvage_app::scan_cache(&root, &mut |_| Control::Continue) {
            Ok(scan) => {
                let paths = scan.review.export_paths(true);
                let outcomes: Vec<_> = paths
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|row| {
                        let id = row["id"].as_str().unwrap().parse().unwrap();
                        let (_, outcome) = scan
                            .review
                            .prepare(id, true)
                            .expect("prepare indexed preview");
                        outcome
                    })
                    .collect();
                json!({
                    "name": name, "files": files, "report": scan.report,
                    "manifest": outcomes,
                })
            }
            Err(error) => json!({"name": name, "files": files, "error": error.kind}),
        }
    })
    .collect()
}

fn recovery_vault(mut vault: Vault) -> Vault {
    // More candidates than the product writer can schedule concurrently.
    for number in 1..=12 {
        let uuid = key(number);
        vault = vault
            .pyramid(PyramidRow::new(&uuid).level(960, 640))
            .level_file(Level::sized(&uuid, 960, 640));
    }
    let pruned = key(13);
    let corrupt = key(14);
    let unsupported = key(15);
    vault
        .pyramid(PyramidRow::new(&pruned).level(800, 600).level(320, 213))
        .level_file(Level::sized(&pruned, 320, 213))
        .pyramid(PyramidRow::new(&corrupt).level(960, 640))
        .level_file(Level::sized(&corrupt, 960, 640).raw(b"damaged"))
        .pyramid(PyramidRow::new(&unsupported).level(960, 640))
        .level_file(Level::sized(&unsupported, 960, 640).raw(b"GIF89a"))
        .pyramid(PyramidRow::new(&key(16)).level(960, 640))
}

fn key(id: u32) -> String {
    format!("A{id:07}-1111-4111-8111-{id:012}")
}
