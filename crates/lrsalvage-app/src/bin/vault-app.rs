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

use lrsalvage_app::vault::{Compare, copy_tree, vaults};
use lrsalvage_fixtures::review;
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
    });
    let path = out.join("native.json");
    fs::write(
        &path,
        serde_json::to_vec_pretty(&document).expect("serialise the native report"),
    )
    .expect("write the native report");
    println!("{}", path.display());
}
