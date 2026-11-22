//! Prepare the shared synthetic vault for the hosted browser writer check.

use std::fs;
use std::path::{Path, PathBuf};

use lrsalvage_app::vault::copy_tree;
use lrsalvage_fixtures::review;
use serde_json::json;

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [out] = args.as_slice() else {
        eprintln!("usage: vault-writer-scale <new directory>");
        std::process::exit(2);
    };
    let out = PathBuf::from(out);
    fs::create_dir(&out).expect("create a fresh vault directory");
    let vault = review::writer_scale().write();
    copy_tree(vault.dir(), &out);
    let relative = |path: &Path| {
        path.strip_prefix(vault.dir())
            .expect("a path inside the vault")
            .to_string_lossy()
            .replace('\\', "/")
    };
    let mut sizes = std::collections::BTreeMap::<u64, usize>::new();
    for path in vault.level_files() {
        let size = path.metadata().expect("measure a level file").len();
        *sizes.entry(size).or_default() += 1;
    }
    let manifest = json!({
        "catalog": relative(vault.catalog()),
        "cache": relative(vault.previews()),
        "payloads_by_size": sizes,
    });
    fs::write(out.join("fixture.json"), manifest.to_string()).expect("write the fixture manifest");
}
