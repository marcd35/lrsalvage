//! The app's entry, on the host: one report per condition, and a stop.
//!
//! `scripts/browser-app.sh` runs the same reports through the page in a browser;
//! this is the half that a gate can run, and it asserts the thing `#117`'s user
//! stories 4 and 5 are about — that the ways a scan can come back empty are told
//! apart by counts rather than by one "nothing found".
//!
//! Every catalog and every cache here comes from `lrsalvage-fixtures`, composed by
//! `lrsalvage_app::vault` (the same seven the browser run uses), so this suite
//! keeps no byte layout of its own.

use lrsalvage_app::Stage;
use lrsalvage_app::vault::{no_index, vaults, working};
use lrsalvage_core::plan::{Control, Progress};
use serde_json::Value;

/// One scan of one vault, with a reporter that never stops.
fn scan(vault: &lrsalvage_fixtures::vault::Vault) -> Value {
    lrsalvage_app::scan(vault.catalog(), vault.previews(), &mut |_| {
        Control::Continue
    })
    .unwrap_or_else(|failure| {
        assert_eq!(
            failure.stage,
            Stage::Open,
            "an unexpected failure: {failure:?}"
        );
        serde_json::json!({ "ok": false, "problem": failure.to_json() })
    })
}

fn number(report: &Value, pointer: &str) -> u64 {
    report
        .pointer(pointer)
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("no number at {pointer} in {report}"))
}

fn text(report: &Value, pointer: &str) -> String {
    report
        .pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("no string at {pointer} in {report}"))
        .to_string()
}

#[test]
fn every_vault_reports_the_condition_it_was_built_for() {
    for (name, _compare, build) in vaults() {
        let vault = build().write();
        let report = scan(&vault);

        match name {
            "working" => {
                assert_eq!(report["ok"], Value::Bool(true), "{name}: {report}");
                assert_eq!(number(&report, "/stats/images"), 4);
                assert_eq!(number(&report, "/stats/recoverable"), 4);
                assert_eq!(text(&report, "/cache/layout"), "modern");
                assert_eq!(report["index"]["problem"], Value::Null);
                // The copy is proven, and the page shows these two: the digest of
                // what was read and of what was opened.
                assert_eq!(
                    text(&report, "/source_sha256"),
                    text(&report, "/snapshot_sha256"),
                    "the verified copy must be the catalog"
                );
            }
            "originals" => {
                // On the host the drive is reachable, which a page has to be
                // handed — the difference `originals` exists to measure.
                assert_eq!(number(&report, "/stats/original_present"), 1);
                assert_eq!(number(&report, "/stats/recoverable"), 1);
                assert_eq!(report["roots"][0]["in_reach"], Value::Bool(true));
                assert!(
                    text(&report, "/roots/0/path").ends_with(['/', std::path::MAIN_SEPARATOR]),
                    "the root is the catalog's own spelling, trailing separator included"
                );
            }
            "no-index" => {
                assert_eq!(text(&report, "/index/problem/kind"), "missing");
                assert_eq!(number(&report, "/stats/no_cache_entry"), 1);
                assert_eq!(number(&report, "/stats/unmapped"), 1);
            }
            "unreadable-index" => {
                assert_eq!(text(&report, "/index/problem/kind"), "unreadable");
                assert!(
                    text(&report, "/index/problem/text").contains("not a database"),
                    "SQLite's own words travel with the reason"
                );
                assert_eq!(number(&report, "/stats/no_cache_entry"), 1);
            }
            "no-payloads" => {
                assert_eq!(text(&report, "/cache/layout"), "none");
                assert_eq!(number(&report, "/stats/missing_payload"), 1);
                assert_eq!(number(&report, "/stats/unrecognised"), 0);
            }
            "unknown-layout" => {
                assert_eq!(text(&report, "/cache/layout"), "unknown");
                assert_eq!(number(&report, "/stats/unrecognised"), 1);
            }
            "not-a-catalog" => {
                assert_eq!(report["ok"], Value::Bool(false), "{name}: {report}");
                assert_eq!(text(&report, "/problem/stage"), "open");
                assert_eq!(text(&report, "/problem/kind"), "unreadable");
                assert!(
                    !text(&report, "/problem/message").is_empty(),
                    "a refusal says what happened"
                );
            }
            other => panic!("no expectation for the vault {other}"),
        }
    }
}

#[test]
fn a_stopped_scan_reports_a_stop_and_no_plan() {
    let vault = working().write();
    let mut reports = 0;
    let mut stopped = false;
    let report = lrsalvage_app::scan(vault.catalog(), vault.previews(), &mut |progress| {
        reports += 1;
        if matches!(progress, Progress::Images { .. }) {
            stopped = true;
            Control::Stop
        } else {
            Control::Continue
        }
    })
    .expect("a stop is not a failure");

    assert!(
        stopped,
        "the images stage reports, so the stop had a boundary"
    );
    assert!(reports > 0);
    assert_eq!(report["stopped"], Value::Bool(true), "{report}");
    assert_eq!(
        report.get("stats"),
        None,
        "a stopped scan must not present a plan"
    );
}

#[test]
fn a_scan_of_a_cache_without_an_index_still_reports_the_payloads() {
    // The page's "nothing found" is four different answers; this is the one where
    // the cache is fine and the map is gone, so the payloads are still counted.
    let vault = no_index().write();
    let report = scan(&vault);
    assert_eq!(report["ok"], Value::Bool(true));
    assert_eq!(number(&report, "/cache/shapes/level_file"), 1);
    assert_eq!(number(&report, "/stats/unmapped"), 1);
    assert_eq!(number(&report, "/stats/no_cache_entry"), 1);
    // The vault's own originals folder is not beside this catalog, so the host
    // itself reports the recorded root as out of reach — the same answer a page
    // gives before it is handed the folder.
    assert_eq!(report["roots"][0]["in_reach"], Value::Bool(false));
}

#[test]
fn review_reads_only_the_certified_jpeg_for_each_catalog_image() {
    use lrsalvage_fixtures::review;

    let vault = review::vault().write();
    let scan = lrsalvage_app::scan_for_review(vault.catalog(), vault.previews(), &mut |_| {
        Control::Continue
    })
    .expect("scan the review vault");
    let photos = scan.report["photos"].as_array().expect("catalog photos");
    assert_eq!(photos.len(), 10);
    let row = |id| photos.iter().find(|photo| photo["id"] == id).unwrap();
    assert_eq!(row("2")["checked"], true);
    assert_eq!(row("7")["checked"], false);
    assert_eq!(row("5")["folder"], Value::Null);
    assert_eq!(row("9")["status"], "no-cache-entry");
    assert_eq!(row("4")["declared"], serde_json::json!([720, 500]));
    assert_eq!(row("4")["size"], serde_json::json!([768, 512]));
    assert_eq!(scan.review.read(2).unwrap(), review::BOUNDARY);
    assert!(
        scan.review.read(7).is_ok(),
        "a small preview is inspectable"
    );
    assert!(
        scan.review.read(9).is_err(),
        "a missing preview has no bytes"
    );

    let portrait = scan.review.read(3).unwrap();
    let jpeg = lrsalvage_core::jpeg::certify(&portrait).unwrap();
    assert_eq!(jpeg.dimensions, (800, 512));
    assert_eq!(
        lrsalvage_core::exif::orientation(&portrait, &jpeg.segments),
        Some(lrsalvage_core::model::Orientation::Rotate90),
    );

    let source = vault
        .level_files()
        .iter()
        .find(|path| path.to_string_lossy().contains("AAAA0002-"))
        .unwrap();
    std::fs::write(source, lrsalvage_fixtures::bytes::jpeg(800, 600)).unwrap();
    assert!(matches!(
        scan.review.read(2),
        Err(lrsalvage_core::Error::PreviewDimensionsChanged {
            actual: (800, 600),
            expected: (640, 480),
        }),
    ));
}
