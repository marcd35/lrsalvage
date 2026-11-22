//! `plan`'s decisions about paths, statuses and stats, driven from a vault.
//!
//! Every catalog, cache and payload here comes from `lrsalvage-fixtures`, so
//! this suite keeps no byte layout of its own; `specs/fixtures.md` says why a
//! test that needs it lives in `tests/` rather than in `src/plan.rs`.
//!
//! The medium is a `legacy` vault: every payload is an `.lrprev` container, so a
//! level's only declaration is the `PyramidLevel` row that names it. A level file
//! can instead be ranked by the `_<n>` in its own leaf when no row explains it
//! (`adr-0005-discover-payloads-by-index` ¶3, ¶5) — which is why a container is
//! the medium where a row and a payload can be made to disagree, and the suspect
//! and per-level stats below are about exactly that.

use std::path::{Component, Path};

use lrsalvage_core::plan::{Control, Options, Plan, PlanItem, Progress, Status};
use lrsalvage_fixtures::vault::{ImageRow, Level, PyramidRow, Vault, digest_of};

const UUID: &str = "AAAA0001-1111-4111-8111-000000000001";

/// A uuid and digest shaped like a real payload address, so the tree walk
/// recognises an unclaimed file as a pyramid (§5.2).
const ORPHAN_UUID: &str = "0BAD0999-1111-4111-8111-000000000999";

/// A `PyramidLevel` row at `width` x `height` and the `.lrprev` chunk holding one
/// such level: the shape almost every test below needs.
fn with_payload(vault: Vault, uuid: &str, width: u32, height: u32) -> Vault {
    vault
        .pyramid(PyramidRow::new(uuid).level(width, height))
        .container(&[Level::sized(uuid, width, height)])
}

fn item(plan: &Plan, image_id: i64) -> &PlanItem {
    plan.items
        .iter()
        .find(|item| {
            item.image
                .as_ref()
                .is_some_and(|image| image.id_local == image_id)
        })
        .unwrap_or_else(|| panic!("no plan item for image {image_id}"))
}

fn items_for(plan: &Plan, image_id: i64) -> Vec<&PlanItem> {
    plan.items
        .iter()
        .filter(|item| {
            item.image
                .as_ref()
                .is_some_and(|image| image.id_local == image_id)
        })
        .collect()
}

fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[test]
fn a_hostile_path_from_root_cannot_write_outside_the_output_root() {
    // The catalog stores the folder path verbatim; `..` must not survive the
    // hop from `pathFromRoot` to a destination.
    let vault = with_payload(
        Vault::legacy().image(
            ImageRow::new(1, "IMG_0001")
                .path_from_root("../../../escaped/")
                .cache(UUID),
        ),
        UUID,
        1616,
        1077,
    )
    .write();
    let plan = vault.plan(&Options::default());

    let output = item(&plan, 1).output.as_ref().expect("output path");
    assert!(
        output.starts_with(vault.out()),
        "{} must stay under {}",
        output.display(),
        vault.out().display()
    );
    assert_eq!(
        slashed(output),
        slashed(
            &vault
                .out()
                .join("originals")
                .join("escaped")
                .join("IMG_0001.jpg")
        )
    );
    assert!(
        !output
            .components()
            .any(|component| matches!(component, Component::ParentDir)),
        "{output:?}"
    );
}

#[test]
fn an_image_whose_original_is_still_on_disk_is_not_a_salvage_target() {
    let second_uuid = "AAAA0002-1111-4111-8111-000000000002";
    let vault = with_payload(
        Vault::legacy()
            .image(ImageRow::new(1, "IMG_0001").original_on_disk().cache(UUID))
            .image(ImageRow::new(2, "IMG_0002").cache(second_uuid)),
        UUID,
        1616,
        1077,
    );
    let vault = with_payload(vault, second_uuid, 1616, 1077).write();

    let plan = vault.plan(&Options::default());

    // Premise: image 1's original really is on disk.
    let image = item(&plan, 1).image.clone().expect("image data");
    assert!(
        image.original_path.is_file(),
        "{}",
        image.original_path.display()
    );

    let present = item(&plan, 1);
    assert_eq!(present.status, Status::OriginalPresent);
    assert_eq!(present.status.as_str(), "original-present");
    assert!(
        !present.selected,
        "an image with its original is not salvaged"
    );
    assert!(present.output.is_none());
    assert!(
        present
            .note
            .as_deref()
            .is_some_and(|note| note.contains("original still on disk")),
        "{:?}",
        present.note
    );

    assert_eq!(plan.stats.original_present, 1);
    assert_eq!(plan.stats.recoverable, 1, "image 2 is the salvage target");
    assert_eq!(plan.selected().count(), 1);
    assert_eq!(item(&plan, 2).status, Status::Recoverable);
}

#[test]
fn include_present_salvages_an_image_whose_original_is_on_disk() {
    let vault = with_payload(
        Vault::legacy().image(ImageRow::new(1, "IMG_0001").original_on_disk().cache(UUID)),
        UUID,
        1616,
        1077,
    )
    .write();
    let plan = vault.plan(&Options {
        include_present: true,
        ..Options::default()
    });

    let item = item(&plan, 1);
    assert_eq!(item.status, Status::Recoverable);
    assert!(item.selected);
    assert_eq!(plan.stats.original_present, 0);
    assert_eq!(plan.stats.recoverable, 1);
}

#[test]
fn a_cache_row_for_a_deleted_image_does_not_hide_its_payload() {
    let vault = with_payload(
        Vault::legacy().image(ImageRow::new(1, "IMG_0001").cache(UUID)),
        UUID,
        1616,
        1077,
    )
    // `imageId` 999 has no row in `Adobe_images` any more, and the payload it
    // names is on disk: a dead claim must not suppress the orphan sweep.
    .cache_entry_without_image(ORPHAN_UUID)
    .unclaimed(&[Level::sized(ORPHAN_UUID, 1024, 683)])
    .write();

    let plan = vault.plan(&Options::default());
    let unmapped: Vec<&PlanItem> = plan
        .items
        .iter()
        .filter(|item| item.status == Status::Unmapped)
        .collect();
    assert_eq!(
        unmapped.len(),
        1,
        "the dead row's payload is an orphan, not a claim: {:#?}",
        plan.items
    );
    assert!(
        unmapped[0]
            .payload
            .as_ref()
            .is_some_and(|payload| payload.path().is_file()),
        "{:?}",
        unmapped[0].payload
    );
    assert_eq!(plan.stats.unmapped, 1);
    assert_eq!(plan.stats.recoverable, 1);
}

#[test]
fn all_levels_counts_images_once_and_names_only_the_extra_levels() {
    let vault = Vault::legacy()
        .image(ImageRow::new(1, "IMG_0001").cache(UUID))
        .pyramid(PyramidRow::new(UUID).level(1616, 1077))
        // A second level the row does not declare: the widened glob finds it.
        .container(&[
            Level::sized(UUID, 1024, 683),
            Level::sized(UUID, 1616, 1077),
        ])
        .write();
    let plan = vault.plan(&Options {
        all_levels: true,
        ..Options::default()
    });

    let items = items_for(&plan, 1);
    assert_eq!(items.len(), 2, "both levels are planned");
    let mut names: Vec<String> = items
        .iter()
        .map(|item| {
            item.output
                .as_ref()
                .expect("output path")
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec!["IMG_0001 - 1024x683.jpg", "IMG_0001.jpg"],
        "only the extra level carries a size suffix"
    );

    assert_eq!(plan.stats.recoverable, 1, "one image, not two levels");
    assert_eq!(plan.stats.levels, 2);
    assert_eq!(plan.stats.selected_items, 2);
    assert_eq!(plan.stats.selected_images, 1);
    assert_eq!(plan.selected_images(), 1);
    assert_eq!(plan.stats.below_floor, 0);
}

#[test]
fn stats_count_images_rather_than_levels() {
    let second_uuid = "AAAA0002-1111-4111-8111-000000000002";
    let vault = Vault::legacy()
        .image(ImageRow::new(1, "IMG_0001").cache(UUID))
        .pyramid(PyramidRow::new(UUID).level(1616, 1077))
        .container(&[
            Level::sized(UUID, 1024, 683),
            Level::sized(UUID, 1616, 1077),
        ])
        // The largest level disagrees with previews.db: the image is suspect.
        .image(ImageRow::new(2, "IMG_0002").cache(second_uuid))
        .pyramid(PyramidRow::new(second_uuid).level(1616, 1077))
        .container(&[
            Level::sized(second_uuid, 800, 600),
            Level::sized(second_uuid, 1024, 683),
        ])
        .write();
    let plan = vault.plan(&Options {
        all_levels: true,
        ..Options::default()
    });

    let all: Vec<&PlanItem> = plan.items.iter().collect();
    assert_eq!(all.len(), 4, "two readable levels per image");
    assert_eq!(plan.stats.levels, 4);
    assert_eq!(plan.stats.recoverable, 1, "image 1");
    assert_eq!(plan.stats.suspect, 1, "image 2, counted once");
    assert_eq!(plan.stats.failed, 0);
    assert_eq!(plan.stats.selected_items, 4);
    assert_eq!(plan.stats.selected_images, 2);
}

#[test]
fn an_unmapped_payload_has_no_output_path_but_names_its_payload() {
    let vault = with_payload(
        Vault::legacy().image(ImageRow::new(1, "IMG_0001").cache(UUID)),
        UUID,
        1616,
        1077,
    )
    .unclaimed(&[Level::sized(ORPHAN_UUID, 1024, 683)])
    .write();

    let plan = vault.plan(&Options::default());
    let unmapped: Vec<&PlanItem> = plan
        .items
        .iter()
        .filter(|item| item.status == Status::Unmapped)
        .collect();
    assert_eq!(unmapped.len(), 1);
    assert!(
        unmapped[0].output.is_none(),
        "an unmapped payload is listed, never written: {:?}",
        unmapped[0].output
    );
    let note = unmapped[0].note.as_deref().expect("note");
    assert!(note.contains(ORPHAN_UUID), "note names the payload: {note}");
    assert!(
        note.contains(&digest_of(ORPHAN_UUID)),
        "note names the payload: {note}"
    );
}

#[test]
fn a_suspect_payload_is_still_selected_and_the_note_explains_why() {
    let vault = Vault::legacy()
        .image(ImageRow::new(1, "IMG_0001").cache(UUID))
        .pyramid(PyramidRow::new(UUID).level(1616, 1077))
        // One chunk that measures 800x600 against a row that claims 1616x1077.
        .container(&[Level::sized(UUID, 800, 600)])
        .write();
    let plan = vault.plan(&Options::default());

    let suspect = item(&plan, 1);
    assert_eq!(suspect.status, Status::Suspect);
    assert!(suspect.selected, "a suspect payload is still a real photo");
    let note = suspect.note.as_deref().expect("note");
    assert!(
        note.contains("800x600") && note.contains("1616x1077"),
        "note names both sizes: {note}"
    );
    assert_eq!(plan.stats.suspect, 1);
    assert_eq!(plan.stats.recoverable, 0);
}

#[test]
fn an_image_without_a_file_row_gets_an_id_based_output_name() {
    let vault = with_payload(
        Vault::legacy().image(ImageRow::new(9, "").without_file().cache(UUID)),
        UUID,
        1616,
        1077,
    )
    .write();
    let plan = vault.plan(&Options::default());

    let planned = item(&plan, 9);
    assert_eq!(planned.status, Status::Recoverable);
    let output = planned.output.as_ref().expect("output path");
    assert_eq!(
        slashed(output),
        slashed(&vault.out().join("image-9.jpg")),
        "no file row means no base name and no folder, and `.jpg` alone is not a file name"
    );
    assert!(planned.selected);
}

#[test]
fn a_virtual_copy_keeps_its_own_name_and_its_own_output_path() {
    let copy_uuid = "AAAA0002-1111-4111-8111-000000000002";
    let vault = with_payload(
        Vault::legacy()
            .image(ImageRow::new(1, "IMG_0001").cache(UUID))
            .image(
                ImageRow::new(2, "IMG_0001")
                    .copy("Black and white")
                    .cache(copy_uuid),
            ),
        UUID,
        1616,
        1077,
    );
    let vault = with_payload(vault, copy_uuid, 1616, 1077).write();
    let plan = vault.plan(&Options::default());

    let master = item(&plan, 1);
    let copy = item(&plan, 2);
    assert_eq!(master.image.as_ref().unwrap().output_stem(), "IMG_0001");
    assert_eq!(
        copy.image.as_ref().unwrap().output_stem(),
        "IMG_0001 - Black and white"
    );
    assert_ne!(master.output, copy.output);
    assert_eq!(
        copy.output.as_ref().unwrap().file_name().unwrap(),
        "IMG_0001 - Black and white.jpg"
    );
}

#[test]
fn a_cache_row_whose_payload_is_gone_is_missing_payload() {
    let vault = Vault::legacy()
        .image(ImageRow::new(1, "IMG_0001").cache(UUID))
        // The row is written, the file it names is not: a pruned payload.
        .pyramid(PyramidRow::new(UUID).level(1616, 1077))
        .write();
    let plan = vault.plan(&Options::default());

    let missing = item(&plan, 1);
    assert_eq!(missing.status, Status::MissingPayload);
    assert!(!missing.selected);
    assert!(missing.output.is_none());
    let payload = missing.payload.as_ref().expect("payload path");
    assert!(
        payload.path().starts_with(vault.previews()),
        "{}",
        payload.path().display()
    );
    assert!(!payload.path().exists());
    assert_eq!(plan.stats.missing_payload, 1);
    assert_eq!(plan.stats.recoverable, 0);
}

#[test]
fn two_images_whose_names_differ_only_in_case_do_not_collide() {
    let second_uuid = "AAAA0002-1111-4111-8111-000000000002";
    let vault = with_payload(
        Vault::legacy()
            .image(ImageRow::new(1, "IMG_0001").cache(UUID))
            .image(ImageRow::new(2, "img_0001").cache(second_uuid)),
        UUID,
        1616,
        1077,
    );
    let vault = with_payload(vault, second_uuid, 1616, 1077).write();
    let plan = vault.plan(&Options::default());
    let first = item(&plan, 1).output.as_ref().expect("output");
    let second = item(&plan, 2).output.as_ref().expect("output");

    assert_eq!(first.file_name().unwrap(), "IMG_0001.jpg");
    assert_ne!(first, second);
    if cfg!(windows) {
        assert_eq!(
            second.file_name().unwrap(),
            "img_0001 (2).jpg",
            "one overwrites the other unless the dedupe key is case-insensitive"
        );
    }
}

// -- reporting and stopping (#149) -------------------------------------------
//
// The contract these hold: every report is a boundary a caller may stop at, a
// report carries the number of units *finished*, and the reported build is the
// only build — so a plan that was never reported is the same plan.

/// Three images with a payload each, in three different pyramids, as level
/// files: a legacy container's levels live inside one file, so only this medium
/// gives the measured pass more than one unit to report.
fn three_images() -> Vault {
    let second = "AAAA0002-1111-4111-8111-000000000002";
    let third = "AAAA0003-1111-4111-8111-000000000003";
    Vault::modern()
        .image(ImageRow::new(1, "IMG_0001").cache(UUID))
        .image(ImageRow::new(2, "IMG_0002").cache(second))
        .image(ImageRow::new(3, "IMG_0003").cache(third))
        .pyramid(PyramidRow::new(UUID).level(1616, 1077))
        .pyramid(PyramidRow::new(second).level(1616, 1077))
        .pyramid(PyramidRow::new(third).level(1616, 1077))
        .level_file(Level::sized(UUID, 1616, 1077))
        .level_file(Level::sized(second, 1616, 1077))
        .level_file(Level::sized(third, 1616, 1077))
        .write()
}

#[test]
fn a_reported_build_is_the_build_it_reports() {
    let vault = three_images();
    let options = Options::default();
    let plain = vault.plan(&options);

    let mut events = Vec::new();
    let reported = vault
        .plan_reported(&options, &mut |progress| {
            events.push(progress);
            Control::Continue
        })
        .expect("a callback that never stops cannot stop a build");

    // Same decisions, item for item: the reported path is not a second opinion.
    assert_eq!(reported.items.len(), plain.items.len());
    assert_eq!(reported.stats, plain.stats);
    assert_eq!(reported.cache, plain.cache);
    for (left, right) in reported.items.iter().zip(&plain.items) {
        assert_eq!(left.status, right.status);
        assert_eq!(left.output, right.output);
    }

    // The stage order, and every unit in it: one cache file per walk step, one
    // level file measured, one image finished — and the last report is the
    // stage's end at `total`.
    let cache: Vec<u64> = events
        .iter()
        .filter_map(|progress| match progress {
            Progress::Cache { files } => Some(*files),
            _ => None,
        })
        .collect();
    assert_eq!(
        cache,
        (0..cache.len() as u64).collect::<Vec<_>>(),
        "one unit per file, and never a skipped or repeated count"
    );
    let walked = lrsalvage_core::fs::walk_files(vault.previews())
        .expect("walk the cache")
        .len() as u64;
    assert_eq!(
        cache.len() as u64,
        walked,
        "the walk reports every file it looks at"
    );

    let payloads: Vec<(u64, u64)> = events
        .iter()
        .filter_map(|progress| match progress {
            Progress::Payloads { done, total } => Some((*done, *total)),
            _ => None,
        })
        .collect();
    assert!(!payloads.is_empty(), "the measured pass reports too");
    assert!(payloads.iter().all(|(_, total)| *total == payloads[0].1));

    let images: Vec<(u64, u64)> = events
        .iter()
        .filter_map(|progress| match progress {
            Progress::Images { done, total } => Some((*done, *total)),
            _ => None,
        })
        .collect();
    assert_eq!(
        images,
        vec![(0, 3), (1, 3), (2, 3), (3, 3)],
        "one report before each image, and the stage's end at the total"
    );

    // The order is the order the work happens in.
    let stage = |progress: &Progress| match progress {
        Progress::Cache { .. } => 0,
        Progress::Payloads { .. } => 1,
        Progress::Images { .. } => 2,
    };
    let mut last = 0;
    for progress in &events {
        let now = stage(progress);
        assert!(now >= last, "stage went backwards at {progress:?}");
        last = now;
    }
}

#[test]
fn a_stop_lands_on_an_image_boundary_and_leaves_no_plan() {
    let vault = three_images();
    let mut reported = Vec::new();
    let built = vault.plan_reported(&Options::default(), &mut |progress| {
        reported.push(progress);
        // Stop the build at the boundary where one image is finished.
        match progress {
            Progress::Images { done: 1, .. } => Control::Stop,
            _ => Control::Continue,
        }
    });

    assert!(built.is_none(), "a stopped build is not a partial plan");
    assert_eq!(
        reported.last(),
        Some(&Progress::Images { done: 1, total: 3 }),
        "the boundary is the one the stop was asked for, and the build asked \
         nothing about the image after it"
    );
    assert!(
        !reported
            .iter()
            .any(|progress| matches!(progress, Progress::Images { done, .. } if *done > 1)),
        "no image past the stop was reported, so none was read"
    );
}

#[test]
fn a_stop_during_the_cache_walk_ends_before_any_payload_is_read() {
    let vault = three_images();
    let mut reported = Vec::new();
    let built = vault.plan_reported(&Options::default(), &mut |progress| {
        reported.push(progress);
        Control::Stop
    });

    assert!(built.is_none());
    assert_eq!(
        reported,
        vec![Progress::Cache { files: 0 }],
        "the first boundary is before the first file, and nothing later ran"
    );
}

#[test]
fn the_last_image_count_is_informational_and_every_unit_is_behind_it() {
    let vault = three_images();
    let mut reported = Vec::new();
    let built = vault.plan_reported(&Options::default(), &mut |progress| {
        reported.push(progress);
        // Ask to stop exactly at the count that says every image is decided.
        match progress {
            Progress::Images { done, total } if done == total => Control::Stop,
            _ => Control::Continue,
        }
    });

    let plan = built.expect("a stop at the last count has no unit left to stop");
    assert_eq!(
        reported.last(),
        Some(&Progress::Images { done: 3, total: 3 }),
        "the stage's end is the count, not the plan"
    );
    assert_eq!(plan.stats.images, 3);
    assert_eq!(
        plan.items.iter().filter(|item| item.selected).count(),
        plan.stats.selected_items as usize,
        "the work after the count happened even though the report answered stop"
    );
}

#[test]
fn an_empty_catalog_still_reports_the_matching_stage_ending_at_zero() {
    let vault = Vault::modern().write();
    let mut reported = Vec::new();
    let built = vault
        .plan_reported(&Options::default(), &mut |progress| {
            reported.push(progress);
            Control::Continue
        })
        .expect("an empty catalog is a plan, not a stop");

    assert_eq!(built.stats.images, 0);
    assert_eq!(
        reported
            .iter()
            .filter(|progress| matches!(progress, Progress::Images { .. }))
            .collect::<Vec<_>>(),
        vec![&Progress::Images { done: 0, total: 0 }],
        "one report, at zero, so a surface has a boundary to end the stage on"
    );
}
