//! The native baseline the browser is held to.
//!
//! `scripts/browser-parity.sh` compares a browser run against a native one, and a
//! comparison of two things that both changed is not a comparison. This test pins
//! the native half: the fixture composition's counts, its statuses by image, and
//! the digest over the JPEGs written — so a change to the driver or the pipeline
//! shows up here, in a gate, instead of becoming a quiet agreement between two
//! targets that agree on the wrong number.
//!
//! It is deliberately small: the pipeline's own behaviour is `tests/matrix.rs`'s
//! and `tests/extract.rs`'s to pin, at length, in `lrsalvage-core`. What is new
//! here is one driver and one composition, run on two targets.

use lrsalvage_web::vault::composition;

#[test]
fn the_native_baseline_is_what_the_driver_reports() {
    let vault = composition(false).write();
    let report = lrsalvage_web::run(vault.catalog(), vault.previews(), vault.out())
        .expect("the driver runs on a fixture vault");

    assert_eq!(
        report["stats"]["images"], 6,
        "every catalog image is planned"
    );
    assert_eq!(report["stats"]["recoverable"], 5);
    assert_eq!(
        report["stats"]["missing_payload"], 1,
        "the row with no file"
    );
    assert_eq!(report["plan_items"], 6);
    assert_eq!(report["written"], 4, "four images have a preview to write");
    assert_eq!(
        report["index_problem"],
        serde_json::Value::Null,
        "a present, readable index is no problem at all"
    );

    let outputs: Vec<&str> = report["outputs"]
        .as_array()
        .expect("a list of outcomes")
        .iter()
        .map(|entry| entry["path"].as_str().expect("a path"))
        .collect();
    assert_eq!(
        outputs,
        vec![
            "photos/2023/IMG_0001.jpg",
            "photos/2023/IMG_0002 - Copy 1.jpg",
            "photos/2023/IMG_0002.jpg",
            "photos/2023/IMG_0004.jpg",
        ],
        "the catalog's own folder tree, and a virtual copy's own name"
    );

    assert_eq!(
        report["source_sha256"], report["snapshot_sha256"],
        "the verified acquisition hashes both halves, and they agree"
    );
    // The digest is over the bytes a run writes, and those carry our EXIF
    // segment's `Software` marker — `lrsalvage <version>`, from
    // `env!("CARGO_PKG_VERSION")`. So a workspace version bump moves this
    // literal by design, and updating it is part of the bump rather than a
    // regression to be investigated: what the literal detects is a change to the
    // driver or the pipeline, and a version is neither.
    assert_eq!(
        report["outputs_sha256"],
        "b44eb74e742ff5bf92bb9bb186f7ed3bcb5bfb939743440a1f89b8e9a41ae15b",
        "the JPEG bytes, as one digest over the paths and their own"
    );
}

#[test]
fn a_sidecar_the_run_ignored_would_lose_an_image() {
    // The same composition with half its image rows in the `-wal`: the snapshot
    // has to carry the sidecar and checkpoint the copy, or the plan is built from
    // half a catalog. `docs/browser-spike.md` §2 is this at real scale.
    let vault = composition(true).write();
    let report = lrsalvage_web::run(vault.catalog(), vault.previews(), vault.out())
        .expect("the driver runs on a vault with a sidecar");

    assert_eq!(
        report["stats"]["images"], 6,
        "the rows in the `-wal` are the catalog's too"
    );
    assert_eq!(
        report["written"], 4,
        "including the one only the sidecar names"
    );
}

#[test]
fn the_seams_own_checks_pass_on_the_host() {
    let dir = lrsalvage_core::fs::scratch_dir("lrsalvage-baseline-dir-").expect("a scratch dir");
    let store =
        lrsalvage_core::fs::scratch_dir("lrsalvage-baseline-store-").expect("a scratch dir");
    let checks = lrsalvage_web::probe(dir.path(), store.path());

    let failures: Vec<&str> = checks
        .iter()
        .filter(|check| !check.diverges && check.outcome != "ok")
        .map(|check| check.name)
        .collect();
    assert!(
        failures.is_empty(),
        "the seam's specification has to pass here too: {failures:?}"
    );
    assert!(
        checks.iter().any(|check| check.diverges),
        "the probe reports the divergences a target is allowed"
    );
}
