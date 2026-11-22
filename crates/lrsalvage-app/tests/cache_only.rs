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
        .level_file(
            Level::sized(KEY, 1600, 1000)
                .raw(&lrsalvage_fixtures::bytes::header_only_jpeg(1600, 1000)),
        )
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
    use lrsalvage_core::{Error, error::IndexProblemKind};
    let empty = Vault::modern().write();
    let unsupported_db = std::fs::read(empty.catalog()).expect("synthetic catalog SQLite bytes");
    for (vault, expected) in [
        (Vault::unindexed(), IndexProblemKind::Missing),
        (
            Vault::modern().raw_index(b"not SQLite"),
            IndexProblemKind::Unreadable,
        ),
        (
            Vault::modern().raw_index(&unsupported_db),
            IndexProblemKind::Unsupported,
        ),
    ] {
        let vault = vault.write();
        let error = CacheRecovery::scan(vault.previews(), &mut |_| Control::Continue).unwrap_err();
        assert!(
            matches!(error, Error::RequiredPreviewIndex { path, kind, .. }
            if path == vault.previews().join("previews.db") && kind == expected)
        );
    }
    let scan = CacheRecovery::scan(empty.previews(), &mut |_| Control::Continue)
        .unwrap()
        .unwrap();
    assert!(scan.items.is_empty());
}

#[test]
fn warns_when_a_complete_level_does_not_match_its_indexed_size() {
    use lrsalvage_fixtures::bytes::jpeg;
    let vault = Vault::modern()
        .pyramid(PyramidRow::new(KEY).level(1600, 1000))
        .level_file(Level::sized(KEY, 1600, 1000).raw(&jpeg(1200, 800)))
        .write();
    let scan = CacheRecovery::scan(vault.previews(), &mut |_| Control::Continue)
        .expect("readable index")
        .expect("finished");
    let item = &scan.items[0];
    assert_eq!(item.status, Status::Recoverable);
    assert!(
        item.dimension_mismatch,
        "a 1200x800 file for a 1600x1000 row"
    );
    assert!(!item.fallback, "nothing larger was indexed and unreadable");
    assert_eq!(
        item.preview.as_ref().expect("recoverable").dimensions,
        (1200, 800)
    );
}

#[test]
fn an_equal_area_tie_prefers_the_level_file_over_the_legacy_chunk() {
    use lrsalvage_fixtures::bytes::{exif_app1, jpeg, jpeg_with};
    let level_file = jpeg_with(960, 640, &[exif_app1(1)]);
    let vault = Vault::mixed()
        .pyramid(PyramidRow::new(KEY).level(960, 640))
        .level_file(Level::sized(KEY, 960, 640).raw(&level_file))
        .container(&[Level::sized(KEY, 960, 640).raw(&jpeg(960, 640))])
        .write();
    let scan = CacheRecovery::scan(vault.previews(), &mut |_| Control::Continue)
        .expect("readable index")
        .expect("finished");
    let preview = scan.items[0].preview.as_ref().expect("recoverable");
    assert_eq!(preview.dimensions, (960, 640));
    assert_eq!(
        preview.read().expect("certified bytes"),
        level_file,
        "the modern level file wins an area tie"
    );
}

#[test]
fn equal_area_level_files_tie_on_their_cache_relative_path() {
    let vault = Vault::modern()
        .pyramid(PyramidRow::new(KEY).level(960, 640).level(640, 960))
        .level_file(Level::sized(KEY, 960, 640))
        .level_file(Level::sized(KEY, 640, 960))
        .write();
    let scan = CacheRecovery::scan(vault.previews(), &mut |_| Control::Continue)
        .expect("readable index")
        .expect("finished");
    let preview = scan.items[0].preview.as_ref().expect("recoverable");
    assert_eq!(
        preview.dimensions,
        (640, 960),
        "the `_640` leaf sorts before the `_960` leaf"
    );
}

#[test]
fn equal_area_legacy_chunks_keep_their_chunk_order() {
    let vault = Vault::mixed()
        .pyramid(PyramidRow::new(KEY).level(960, 640).level(640, 960))
        .container(&[Level::sized(KEY, 960, 640), Level::sized(KEY, 640, 960)])
        .write();
    let scan = CacheRecovery::scan(vault.previews(), &mut |_| Control::Continue)
        .expect("readable index")
        .expect("finished");
    let preview = scan.items[0].preview.as_ref().expect("recoverable");
    assert_eq!(
        preview.dimensions,
        (960, 640),
        "the first `level_N` chunk wins"
    );
}

#[test]
fn generated_names_depend_only_on_the_indexed_pair() {
    let alone = Vault::modern()
        .pyramid(PyramidRow::new(KEY).level(640, 400))
        .level_file(Level::sized(KEY, 640, 400))
        .write();
    let mut crowded = Vault::modern()
        .pyramid(PyramidRow::new(KEY).level(640, 400))
        .level_file(Level::sized(KEY, 640, 400));
    // Another cache root, and pyramids this pair has nothing to do with.
    for number in 1..=8 {
        let uuid = format!("B{number:07}-1111-4111-8111-{number:012}");
        crowded = crowded
            .pyramid(PyramidRow::new(&uuid).level(800, 600))
            .level_file(Level::sized(&uuid, 800, 600));
    }
    let crowded = crowded.write();
    let names = |vault: &Vault| -> Vec<String> {
        let scan = CacheRecovery::scan(vault.previews(), &mut |_| Control::Continue)
            .expect("readable index")
            .expect("finished");
        scan.items
            .into_iter()
            .filter(|item| item.status == Status::Recoverable)
            .map(|item| item.filename)
            .collect()
    };
    let alone = names(&alone);
    assert_eq!(alone.len(), 1, "one indexed pyramid, one name");
    assert!(
        names(&crowded).contains(&alone[0]),
        "the pair keeps its name beside unrelated pyramids, in another cache root"
    );
    let hex = alone[0]
        .strip_prefix("preview-")
        .and_then(|rest| rest.strip_suffix(".jpg"))
        .expect("preview-<hash>.jpg");
    assert_eq!(hex.len(), 64);
    assert!(hex.chars().all(|character| character.is_ascii_hexdigit()));
    assert!(
        hex.chars().all(|character| !character.is_ascii_uppercase()),
        "the hash is lowercase"
    );
}

#[test]
fn a_differently_cased_disk_key_does_not_match_an_indexed_pyramid() {
    let vault = Vault::modern()
        .pyramid(PyramidRow::new(&KEY.to_ascii_lowercase()).level(640, 400))
        .level_file(Level::sized(KEY, 640, 400))
        .write();
    let scan = CacheRecovery::scan(vault.previews(), &mut |_| Control::Continue)
        .unwrap()
        .unwrap();
    assert_eq!(scan.counts.indexed, 1);
    assert_eq!(scan.counts.missing, 1);
}
