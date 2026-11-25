//! The indexless entry (#198): one report, one review, one batch protocol.
//!
//! The scan's own rules are `lrsalvage-core`'s and are tested there; this is the
//! app's projection — the names review shows, the bytes the writer gets, and the
//! mode word the published files carry.

use lrsalvage_app::scan_legacy;
use lrsalvage_core::plan::{Control, Progress};
use lrsalvage_fixtures::bytes;
use lrsalvage_fixtures::vault::{Level, Vault};

const KEY: &str = "A0000001-1111-4111-8111-000000000001";
const OTHER: &str = "A0000002-1111-4111-8111-000000000002";

#[test]
fn the_report_names_the_mode_and_counts_containers() {
    let png = bytes::other_image();
    let mut unsupported = bytes::chunk("header", &bytes::header(&[(900, 600)]), 0);
    unsupported.extend_from_slice(&bytes::chunk("level_1", &png, 0));
    let vault = Vault::unindexed_legacy()
        .container_at("loose/preview.lrprev", &[Level::sized(KEY, 640, 427)])
        .raw_file("shard/unsupported.lrprev", &unsupported)
        .raw_file("shard/garbage.lrprev", b"not a container")
        .write();
    let scan = scan_legacy(vault.previews(), &mut |_| Control::Continue).expect("a walkable cache");
    let report = &scan.report;
    assert_eq!(report["ok"], true);
    assert_eq!(report["stopped"], false);
    assert_eq!(report["mode"], "legacy");
    assert_eq!(report["scan"]["containers"], 3);
    assert_eq!(report["scan"]["usable"], 1);
    assert_eq!(report["scan"]["unsupported"], 1);
    assert_eq!(report["scan"]["invalid"], 1);
    assert!(
        report.get("layout").is_none(),
        "an indexless run classifies no layout"
    );
    let photos = report["photos"].as_array().expect("rows");
    assert_eq!(photos.len(), 1, "only a usable container has a row");
    assert_eq!(photos[0]["preview"], true);
    assert_eq!(photos[0]["checked"], true);
    assert_eq!(photos[0]["size"], serde_json::json!([640, 427]));
    assert_eq!(photos[0]["folder"], serde_json::Value::Null);
    assert!(
        photos[0]["name"]
            .as_str()
            .expect("a generated name")
            .starts_with("legacy-preview-")
    );
}

#[test]
fn the_entry_selects_one_jpeg_per_container_and_exports_it_verbatim() {
    let large = bytes::jpeg(1600, 1067);
    let other = bytes::jpeg(1200, 800);
    let vault = Vault::unindexed_legacy()
        .container_at(
            "loose/one.lrprev",
            &[
                Level::sized(KEY, 320, 213),
                Level::sized(KEY, 1600, 1067).raw(&large),
            ],
        )
        .container_at(
            "loose/two.lrprev",
            &[Level::sized(OTHER, 1200, 800).raw(&other)],
        )
        .write();
    let scan = scan_legacy(vault.previews(), &mut |_| Control::Continue).expect("a walkable cache");
    let paths = scan.review.export_paths(true);
    let rows = paths.as_array().expect("one path per usable container");
    assert_eq!(rows.len(), 2, "every usable preview starts selected");

    let mut exported = Vec::new();
    for row in rows {
        let id = row["id"]
            .as_str()
            .expect("ids travel as strings")
            .parse()
            .unwrap();
        let (jpeg, outcome) = scan.review.prepare(id, true).expect("a certified preview");
        assert_eq!(jpeg.len() as u64, row["bytes"].as_u64().unwrap());
        assert_eq!(
            outcome.output.to_string_lossy(),
            row["path"].as_str().unwrap(),
            "the outcome names the planned path"
        );
        assert_eq!(scan.review.read(id).expect("the preview reads back"), jpeg);
        let existing = scan
            .review
            .existing(id, true, &jpeg)
            .expect("bytes are inspected")
            .expect("matching bytes are already recovered");
        assert_eq!(existing.status.as_str(), "skipped");
        assert!(
            scan.review
                .existing(id, true, b"different bytes")
                .expect("bytes are inspected")
                .is_none(),
            "different bytes are a conflict, not a recovery"
        );
        exported.push(jpeg);
    }
    assert!(exported.contains(&large));
    assert!(exported.contains(&other));
}

#[test]
fn the_published_files_carry_the_legacy_mode() {
    let vault = Vault::unindexed_legacy()
        .container_at("loose/preview.lrprev", &[Level::sized(KEY, 640, 427)])
        .write();
    let scan = scan_legacy(vault.previews(), &mut |_| Control::Continue).expect("a walkable cache");
    let rows = scan.review.export_paths(true);
    let id = rows.as_array().unwrap()[0]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let (_, outcome) = scan.review.prepare(id, true).expect("a certified preview");
    let files = scan.review.reports(true, vec![outcome], 1_700_000_000);
    let report_csv = files["report.csv"].as_str().expect("csv text");
    assert!(
        report_csv.starts_with("mode,filename,status,width,height,container\r\nlegacy,"),
        "{report_csv}"
    );
    let extract_csv = files["extract.csv"].as_str().expect("csv text");
    assert!(extract_csv.starts_with("mode,status,bytes,sha256,output,note\r\nlegacy,"));
    let manifest: serde_json::Value =
        serde_json::from_str(files["manifest.json"].as_str().expect("json text")).expect("json");
    assert_eq!(manifest["schema"], "lrsalvage.manifest");
    assert_eq!(manifest["mode"], "legacy");
    assert_eq!(manifest["scan"]["containers"], 1);
    assert_eq!(manifest["batch"]["saved"], 1);

    let verification = scan.review.verification(
        lrsalvage_core::report::CacheBatch {
            saved: 1,
            already_recovered: 0,
            failed: 0,
            omitted: 0,
            confirmed_on_disk: Some(1),
        },
        1_700_000_000,
    );
    let verification: serde_json::Value = serde_json::from_str(&verification).expect("json");
    assert_eq!(verification["schema"], "lrsalvage.verification");
    assert_eq!(verification["mode"], "legacy");
    assert_eq!(verification["scan"]["usable"], 1);
    assert!(!verification.to_string().contains("legacy-preview-"));
}

#[test]
fn an_empty_cache_yields_no_candidate_and_no_save() {
    let vault = Vault::unindexed_legacy().write();
    let scan = scan_legacy(vault.previews(), &mut |_| Control::Continue).expect("a walkable cache");
    assert_eq!(scan.report["scan"]["containers"], 0);
    assert_eq!(scan.report["photos"], serde_json::json!([]));
    assert_eq!(
        scan.review.export_paths(true),
        serde_json::json!([]),
        "the page has nothing to offer a save for"
    );
}

#[test]
fn a_stopped_scan_is_a_report_and_holds_no_plan() {
    let vault = Vault::unindexed_legacy()
        .container_at("loose/preview.lrprev", &[Level::sized(KEY, 640, 427)])
        .write();
    let scan = scan_legacy(vault.previews(), &mut |progress| match progress {
        Progress::Payloads { .. } => Control::Stop,
        _ => Control::Continue,
    })
    .expect("the walk itself succeeds");
    assert_eq!(scan.report["stopped"], true);
    assert_eq!(scan.review.export_paths(true), serde_json::json!([]));
}
