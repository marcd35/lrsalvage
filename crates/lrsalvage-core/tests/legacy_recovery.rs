//! Recovery from a preview cache with no `previews.db` (#198).
//!
//! A candidate is one `.lrprev` file wherever it sits, and the plan answers one
//! outcome per file: usable, unreadable or invalid, or unsupported. Everything
//! below is composed through `lrsalvage-fixtures`' vault and its byte shapes.

use lrsalvage_core::cache_recovery::{ContainerStatus, LegacyRecovery};
use lrsalvage_core::plan::{Control, Progress};
use lrsalvage_fixtures::bytes;
use lrsalvage_fixtures::vault::{Level, Vault};

const KEY: &str = "A0000001-1111-4111-8111-000000000001";
const OTHER: &str = "A0000002-1111-4111-8111-000000000002";

fn scan(vault: &Vault) -> LegacyRecovery {
    LegacyRecovery::scan(vault.previews(), &mut |_| Control::Continue)
        .expect("a cache tree can be walked")
        .expect("a callback that never stops cannot stop the scan")
}

#[test]
fn one_certified_jpeg_per_container_ranked_by_measured_area() {
    let large = bytes::jpeg(1600, 1067);
    let vault = Vault::unindexed_legacy()
        .container_at(
            "container.lrprev",
            &[
                Level::sized(KEY, 320, 213),
                Level::sized(KEY, 1600, 1067).raw(&large),
                Level::sized(KEY, 800, 533),
            ],
        )
        .write();
    let scan = scan(&vault);
    assert_eq!(scan.counts.containers, 1);
    assert_eq!(scan.counts.usable, 1);
    let item = &scan.items[0];
    assert_eq!(item.status, ContainerStatus::Usable);
    let preview = item.preview.as_ref().expect("a usable container has one");
    assert_eq!(preview.dimensions, (1600, 1067));
    assert_eq!(
        preview.read().expect("the certified bytes"),
        large,
        "the exported JPEG is the chunk's own bytes, verbatim"
    );
    assert!(preview.matches(&large));
    assert!(!preview.matches(b"foreign"));
}

#[test]
fn an_equal_area_tie_keeps_the_first_certified_chunk_in_file_order() {
    let first = bytes::jpeg(960, 640);
    let second = bytes::jpeg(640, 960);
    let vault = Vault::unindexed_legacy()
        .container_at(
            "tie.lrprev",
            &[
                Level::sized(KEY, 960, 640).raw(&first),
                Level::sized(KEY, 640, 960).raw(&second),
            ],
        )
        .write();
    let scan = scan(&vault);
    let preview = scan.items[0].preview.as_ref().expect("a usable container");
    assert_eq!(preview.dimensions, (960, 640));
    assert_eq!(preview.read().expect("bytes"), first);
}

#[test]
fn complete_chunks_before_a_damaged_boundary_stay_eligible() {
    let mut damaged = bytes::container(&[(1200, 800), (320, 213)]);
    // The declared lengths stop agreeing here: the two chunks before it were
    // proved by their own boundaries, which is what makes them readable.
    damaged.extend_from_slice(&bytes::damaged_tail());
    let vault = Vault::unindexed_legacy()
        .raw_file("shard/damaged.lrprev", &damaged)
        .write();
    let scan = scan(&vault);
    let item = &scan.items[0];
    assert_eq!(item.status, ContainerStatus::Usable);
    assert_eq!(
        item.preview
            .as_ref()
            .expect("a usable container")
            .dimensions,
        (1200, 800)
    );
    assert_eq!(scan.counts.invalid, 0);
}

#[test]
fn a_damaged_container_with_no_certified_chunk_is_invalid() {
    let mut truncated = bytes::chunk("level_1", &bytes::truncated_jpeg(1200, 800), 0);
    truncated.extend_from_slice(b"AgHg\x00\x00");
    let vault = Vault::unindexed_legacy()
        .raw_file("shard/truncated.lrprev", &truncated)
        .raw_file("shard/garbage.lrprev", b"not a container at all")
        .write();
    let scan = scan(&vault);
    assert_eq!(scan.counts.containers, 2);
    assert_eq!(scan.counts.invalid, 2);
    assert_eq!(scan.counts.usable, 0);
    assert!(scan.items.iter().all(|item| item.preview.is_none()));
}

#[test]
fn a_container_of_recognized_non_jpeg_images_is_unsupported() {
    let png = bytes::other_image();
    let mut file = bytes::chunk("header", &bytes::header(&[(1200, 800)]), 0);
    file.extend_from_slice(&bytes::chunk("level_1", &png, 0));
    let vault = Vault::unindexed_legacy()
        .raw_file("shard/unsupported.lrprev", &file)
        .write();
    let scan = scan(&vault);
    assert_eq!(scan.items[0].status, ContainerStatus::Unsupported);
    assert_eq!(scan.counts.unsupported, 1);
    assert!(scan.items[0].preview.is_none());
}

#[test]
fn a_container_outside_the_shard_folders_is_a_candidate() {
    let vault = Vault::unindexed_legacy()
        .container_at("loose/preview.lrprev", &[Level::sized(KEY, 800, 533)])
        .container(&[Level::sized(OTHER, 1200, 800)])
        .write();
    assert!(!vault.previews().join("previews.db").exists());
    let scan = scan(&vault);
    assert_eq!(scan.counts.containers, 2);
    assert_eq!(scan.counts.usable, 2);
    assert!(
        scan.items
            .iter()
            .any(|item| item.relative == std::path::Path::new("loose/preview.lrprev"))
    );
}

#[test]
fn an_indexless_cache_with_no_container_offers_nothing() {
    let vault = Vault::unindexed_legacy().write();
    let scan = scan(&vault);
    assert!(scan.items.is_empty());
    assert_eq!(scan.counts.containers, 0);
    assert_eq!(scan.counts.usable, 0);
}

#[test]
fn generated_names_depend_only_on_the_cache_relative_path() {
    let path = "loose/preview.lrprev";
    let first = Vault::unindexed_legacy()
        .container_at(path, &[Level::sized(KEY, 640, 427)])
        .write();
    // Another cache root, and another container this one has nothing to do with.
    let crowded = Vault::unindexed_legacy()
        .container_at(path, &[Level::sized(KEY, 640, 427)])
        .container_at("other/first.lrprev", &[Level::sized(OTHER, 640, 427)])
        .write();
    let named = |vault: &Vault| -> Vec<(String, String)> {
        scan(vault)
            .items
            .into_iter()
            .map(|item| {
                (
                    item.relative.to_string_lossy().replace('\\', "/"),
                    item.filename,
                )
            })
            .collect()
    };
    let first = named(&first);
    let crowded = named(&crowded);
    assert_eq!(first.len(), 1);
    assert_eq!(
        first[0].1, crowded[0].1,
        "the same cache-relative path keeps its name beside an unrelated container"
    );
    assert_ne!(
        crowded[0].1, crowded[1].1,
        "two containers with identical previews have distinct names"
    );
    let hash = first[0]
        .1
        .strip_prefix("legacy-preview-")
        .and_then(|rest| rest.strip_suffix(".jpg"))
        .expect("legacy-preview-<hash>.jpg");
    assert_eq!(hash.len(), 64);
    assert!(
        hash.chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase()),
        "a lowercase SHA-256, as the indexed mode's names are"
    );
}

#[test]
fn a_stopped_scan_has_no_plan() {
    let vault = Vault::unindexed_legacy()
        .container_at("one.lrprev", &[Level::sized(KEY, 640, 427)])
        .write();
    let mut reports = 0;
    let stopped = LegacyRecovery::scan(vault.previews(), &mut |progress| {
        if let Progress::Payloads { .. } = progress {
            reports += 1;
            return Control::Stop;
        }
        Control::Continue
    })
    .expect("the walk itself succeeds");
    assert!(stopped.is_none());
    assert_eq!(reports, 1);
}

#[test]
fn the_published_files_carry_the_legacy_mode_and_its_own_counts() {
    use lrsalvage_core::report::{self, CacheBatch};
    use serde_json::{Value, json};

    let png = bytes::other_image();
    let mut unsupported = bytes::chunk("header", &bytes::header(&[(900, 600)]), 0);
    unsupported.extend_from_slice(&bytes::chunk("level_1", &png, 0));
    let vault = Vault::unindexed_legacy()
        .container_at("loose/preview.lrprev", &[Level::sized(KEY, 640, 427)])
        .raw_file("shard/other.lrprev", &unsupported)
        .write();
    let scan = scan(&vault);

    let csv = report::legacy_csv(&scan);
    let lines: Vec<&str> = csv.split("\r\n").filter(|line| !line.is_empty()).collect();
    assert_eq!(lines[0], "mode,filename,status,width,height,container");
    assert_eq!(lines.len(), 3, "a header and one row per container: {csv}");
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("legacy,legacy-preview-")
                && line.contains(",usable,640,427,loose/preview.lrprev")),
        "{csv}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains(",unsupported,,,shard/other.lrprev")),
        "{csv}"
    );

    let batch = CacheBatch {
        saved: 1,
        already_recovered: 2,
        failed: 3,
        omitted: 0,
        confirmed_on_disk: Some(3),
    };
    let manifest: Value = serde_json::from_str(&report::legacy_manifest_json(
        &scan,
        &json!([{"output": "legacy-preview-aa.jpg", "status": "written"}]),
        batch,
        "9.9.9",
        1_700_000_000,
    ))
    .expect("json");
    let mut keys: Vec<&String> = manifest.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "batch",
            "generated_unix",
            "mode",
            "outputs",
            "scan",
            "schema",
            "version",
        ],
        "the legacy member set, frozen in specs/modules/report.md"
    );
    assert_eq!(manifest["schema"], "lrsalvage.manifest");
    assert_eq!(manifest["mode"], "legacy");
    assert_eq!(manifest["scan"]["containers"], 2);
    assert_eq!(manifest["scan"]["usable"], 1);
    assert_eq!(manifest["scan"]["unsupported"], 1);
    assert_eq!(manifest["batch"]["saved"], 1);

    let text = report::legacy_verification_json(&scan, batch, "9.9.9", 1_700_000_000);
    let verification: Value = serde_json::from_str(&text).expect("json");
    let mut keys: Vec<&String> = verification.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "generated_unix",
            "mode",
            "scan",
            "schema",
            "version",
            "writes",
        ],
        "the frozen legacy member set: no item, no filename, no container path"
    );
    assert_eq!(verification["mode"], "legacy");
    assert_eq!(verification["scan"]["invalid"], 0);
    assert!(
        !text.contains("legacy-preview-") && !text.contains("loose/"),
        "a shared report names no generated filename and no container: {text}"
    );
}

#[test]
fn a_root_that_cannot_be_enumerated_is_an_inventory_error() {
    // A folder the walk cannot read is never an empty answer: the inventory is
    // incomplete, so the scan fails instead of reporting nothing.
    let vault = Vault::unindexed_legacy().write();
    let missing = vault.previews().join("not-a-folder");
    let error = LegacyRecovery::scan(&missing, &mut |_| Control::Continue)
        .expect_err("a missing root is not a complete inventory");
    assert!(
        matches!(&error, lrsalvage_core::Error::Io { path, .. } if path == &missing),
        "{error}"
    );
}

#[test]
fn a_scan_opens_the_cache_read_only() {
    let vault = Vault::unindexed_legacy()
        .container_at("one/two.lrprev", &[Level::sized(KEY, 640, 427)])
        .raw_file("shard/other.lrprev", &bytes::container(&[(800, 533)]))
        .write();
    let before = tree(vault.previews());
    let scan = scan(&vault);
    assert_eq!(scan.counts.usable, 2);
    assert_eq!(
        tree(vault.previews()),
        before,
        "a scan changes no byte of the cache it reads"
    );
}

/// Every file under `root` with its own bytes, in path order — the cache's
/// whole state, so two calls say whether anything changed.
fn tree(root: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut paths = lrsalvage_core::fs::walk_files(root).expect("walk the cache");
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let bytes = lrsalvage_core::fs::read(&path).expect("read a cache file");
            (path, bytes)
        })
        .collect()
}
