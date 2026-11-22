use lrsalvage_core::cache_recovery::{CacheRecovery, Status};
use lrsalvage_core::plan::Control;
use lrsalvage_fixtures::vault::{Level, PyramidRow, Vault};

const KEY: &str = "A0000001-1111-4111-8111-000000000001";

#[test]
fn recovers_the_largest_complete_level_across_both_formats() {
    let vault = Vault::mixed()
        .raw_catalog(b"unreadable catalog")
        .pyramid(PyramidRow::new(KEY).level(1600, 1000))
        .container(&[Level::sized(KEY, 1200, 800)])
        .level_file(Level::sized(KEY, 900, 600))
        .level_file(Level::sized(KEY, 1600, 1000).raw(b"truncated"))
        .write();

    let scan = CacheRecovery::scan(vault.previews(), &mut |_| Control::Continue)
        .expect("readable index")
        .expect("finished");
    assert_eq!(scan.items.len(), 1);
    let item = &scan.items[0];
    assert_eq!(item.status, Status::Recoverable);
    assert!(item.fallback);
    let preview = item.preview.as_ref().expect("complete legacy level");
    assert_eq!(preview.dimensions, (1200, 800));
    let jpeg = preview.read().expect("certified bytes");
    assert!(preview.matches(&jpeg));
    assert!(!preview.matches(b"foreign"));
}

#[test]
fn accounts_for_indexed_pyramids_with_no_payload() {
    let vault = Vault::modern()
        .pyramid(PyramidRow::new(KEY).level(200, 100))
        .write();
    let scan = CacheRecovery::scan(vault.previews(), &mut |_| Control::Continue)
        .expect("readable index")
        .expect("finished");
    assert_eq!(scan.items.len(), 1);
    assert_eq!(scan.items[0].status, Status::Missing);
}

#[test]
fn only_positive_index_rows_add_pyramids() {
    use lrsalvage_fixtures::vault::digest_of;
    let row_only = "A0000002-1111-4111-8111-000000000002";
    let entry_only = "A0000003-1111-4111-8111-000000000003";
    let disk_only = "A0000004-1111-4111-8111-000000000004";
    let old = "d34d0000000000000000000000000000";
    let vault = Vault::modern()
        .pyramid(PyramidRow::new(KEY).level(640, 400))
        .level_file(Level::sized(KEY, 640, 400))
        .pyramid(PyramidRow::new(KEY).digest(old).level(320, 200))
        .level_file(Level::sized(KEY, 320, 200).digest(old))
        .level_file(Level::sized(entry_only, 640, 400))
        .row_without_file(Level::sized(row_only, 640, 400))
        .cache_entry_without_image(entry_only)
        .level_file(Level::sized(disk_only, 640, 400))
        .tombstone(disk_only, 5.0)
        .write();
    let scan = CacheRecovery::scan(vault.previews(), &mut |_| Control::Continue)
        .unwrap()
        .unwrap();
    assert_eq!(scan.counts.indexed, 4);
    assert_eq!(scan.counts.recoverable, 3);
    assert_eq!(scan.counts.missing, 1);
    assert_eq!(digest_of(KEY), "a0000001111141118111000000000001");
    assert!(scan.items.iter().any(|item| item.filename
        == "preview-bbcd7af7454a756a6b60ea95450f70a149c1f02aa946c3e7715d8b8da7968312.jpg"
        && item.status == Status::Recoverable));
}

#[test]
fn distinguishes_missing_damaged_and_unsupported_indexes_from_an_empty_index() {
    use lrsalvage_core::previews_db::RequiredIndexProblem;
    let empty = Vault::modern().write();
    let unsupported_db = std::fs::read(empty.catalog()).expect("synthetic catalog SQLite bytes");
    for (vault, expected) in [
        (Vault::unindexed(), RequiredIndexProblem::Missing),
        (
            Vault::modern().raw_index(b"not SQLite"),
            RequiredIndexProblem::Unreadable,
        ),
        (
            Vault::modern().raw_index(&unsupported_db),
            RequiredIndexProblem::Unsupported,
        ),
    ] {
        let vault = vault.write();
        let error = CacheRecovery::scan(vault.previews(), &mut |_| Control::Continue).unwrap_err();
        assert_eq!(error, expected);
    }
    let scan = CacheRecovery::scan(empty.previews(), &mut |_| Control::Continue)
        .unwrap()
        .unwrap();
    assert!(scan.items.is_empty());
}
