//! The native half of the parity run: build the vault, run the driver, report.
//!
//! `scripts/browser-parity.sh` runs this, then drives the same vault through
//! headless Chromium with the same driver compiled to `wasm32`, and compares the
//! two documents (`web/parity.js` does the comparison, in the page, so the two
//! sides are one implementation each rather than three).
//!
//! Two vaults, not one: the plain composition, and the same composition with its
//! catalog left between checkpoints so half the image rows are in the `-wal`.
//! That is the state `docs/browser-spike.md` §2 measured as 78-of-128, and it is
//! the one the seam's `copy` has to carry.
//!
//! ```text
//! cargo run -p lrsalvage-web --bin vault-parity -- <out-dir>
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use lrsalvage_web::vault::{composition, copy_tree};
use serde_json::json;

fn main() {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            eprintln!("usage: vault-parity <out-dir>");
            std::process::exit(2);
        });
    fs::create_dir_all(&out).expect("create the output directory");

    let mut vaults = Vec::new();
    for (name, wal) in [("plain", false), ("wal", true)] {
        let root = out.join(name);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create a vault directory");

        let vault = composition(wal).write();
        let files = copy_tree(vault.dir(), &root);
        let relative = |path: &Path| {
            path.strip_prefix(vault.dir())
                .expect("a path inside the vault")
                .to_string_lossy()
                .replace('\\', "/")
        };
        let catalog = relative(vault.catalog());
        let cache = relative(vault.previews());
        let output = relative(vault.out());

        let report = lrsalvage_web::run(
            &root.join(&catalog),
            &root.join(&cache),
            &root.join(&output),
        )
        .unwrap_or_else(|error| panic!("{name}: the native run failed: {error}"));

        vaults.push(json!({
            "name": name,
            "files": files,
            "catalog": catalog,
            "cache": cache,
            "output": output,
            "report": report,
        }));
    }

    // The seam's own checks, on the host. `dir` and `store` are two scratch
    // directories here and two mounts in the page.
    let dir =
        lrsalvage_core::fs::scratch_dir("lrsalvage-parity-dir-").expect("a scratch directory");
    let store =
        lrsalvage_core::fs::scratch_dir("lrsalvage-parity-store-").expect("a scratch directory");
    let checks = lrsalvage_web::probe(dir.path(), store.path());

    let document = json!({
        "target": "native",
        "separator": std::path::MAIN_SEPARATOR.to_string(),
        "vaults": vaults,
        "probe": lrsalvage_web::probe_report(&checks),
    });
    fs::write(
        out.join("native.json"),
        serde_json::to_vec_pretty(&document).expect("serialise the native report"),
    )
    .expect("write the native report");
    println!("{}", out.join("native.json").display());
}
