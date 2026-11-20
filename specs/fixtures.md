---
id: fixtures
type: task-spec
title: "fixtures: the synthetic vault matrix"
status: draft
parent: architecture
depends-on:
  - plan
  - extract
  - report
  - previews-db
  - lrprev
  - exif
  - jpeg
  - catalog
references:
  - adr-0003-preserve-preview-bytes
  - adr-0004-detect-layout-not-version
  - adr-0005-discover-payloads-by-index
  - adr-0006-degrade-without-guessing
  - adr-0007-publish-a-verification-report
  - format-notes
  - glossary
---

# fixtures

## Responsibility

The contract between what the product promises and what a test proves. Every
status, statistic, report field and exit code named by
`goal-and-requirements`, `plan`, `extract`, `report`, `previews-db`, `lrprev`,
`exif` and `cli` is proved by one synthetic **vault** built by one shared builder;
a new status or stat arrives with a row here or it is not shipped.

A **vault** is one temporary directory holding the three things a run reads and
one it writes: `<dir>/catalog/<name>.lrcat`, `<dir>/<name> Previews.lrdata`, the
originals a row needs to still exist, and `<dir>/out`. Nothing is committed: every
vault is built at test time and dropped with its `TempDir`. Real catalogs are
reached only from `#[ignore]`d tests, in the four evidence families
`scripts/check.sh` gates by name — a converted cache, a cache Lightroom rendered
from scratch, a cache it built 1:1 previews for, and a cache it discarded the
standard and 1:1 previews of; real catalog bytes are never committed.

`format-notes` remains the only source of byte facts, each with its evidence
label. This matrix pins *decisions* against shapes it builds itself; it can never
show what Adobe writes, and the `Evidence` column says so row by row.

## The builder

### Home and wiring

`crates/lrsalvage-fixtures` — a third workspace member, `publish = false`, with
`version`/`edition`/`license`/`rust-version` inherited from the workspace. It is a
**dev-dependency** of both `lrsalvage-core` and `lrsalvage-cli` and depends on
`rusqlite` and `tempfile` only. No external dependency is added, so `deny.toml`
and `typos.toml` are untouched; `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace` and
`cargo build --release --locked` all see it because it is a member.

It takes no dependency on `assert_cmd` and knows nothing about the binary: it
yields paths, and each test crate wraps them in its own command. That keeps exit
codes — a CLI fact — out of the core crate's tests.

### Two layers

**`bytes` — raw shapes, no scenario.** The only place a leaf name, a chunk header
or a JPEG frame is constructed:

```text
jpeg(w, h)                           SOI, APP0, SOF0, SOS, EOI
jpeg_with(w, h, segments)            the same frame, with APP segments in front
exif_app1(orientation)               Exif APP1 carrying IFD0 0x0112
xmp_app1(xml)                        an XMP packet as its own APP1
icc_app2(profile)                    an ICC_PROFILE APP2 (chunked if large)
truncated_jpeg(w, h)                 a frame with no EOI
no_soi(w, h)                         the body without its SOI
not_a_jpeg()                         bytes that never were images
our_output(w, h, version)            a destination we could have written: complete,
                                     SOF (w, h), IFD0 0x0131 = "lrsalvage <version>"
foreign_output(w, h, software)       the same frame and the same tag, carrying a
                                     marker that is not ours (another tool's file)
chunk(name, payload, padding)        one AgHg chunk (24 + name.len() header)
container(levels)                    the header chunk plus one level_N chunk each
leaf(uuid, digest, long_edge)        "<UUID>-<digest>_<long_edge>"
leaf_dir(uuid)                       (uuid[0], uuid[0..4])
xmp_blob(xml)                        the catalog's zlib blob with its 4-byte prefix
```

**`vault` — scenario composition over those bytes.** One constructor per vault
archetype, then per-image composition:

```text
Vault::modern() / legacy() / mixed() / damaged() / empty() / unindexed() / unrecognised()
Vault::image(ImageRow)              catalog row, original path, orientation, XMP blob.
                                    `ImageRow::path_from_root` sets
                                    AgLibraryFolder.pathFromRoot, so an image's folder
                                    tree — a hostile `..` included — reaches the catalog
                                    as Lightroom stored it
Vault::pyramid(PyramidRow)          Pyramid + PyramidLevel rows, profile and quality names
Vault::level_file(level)            a level file at its derived leaf, or with raw bytes
Vault::row_without_file(level)      a PyramidLevel row whose file is absent (pruned)
Vault::file_without_row(level)      a level file the widened glob must find
Vault::tombstone(timestamp)         a DeletedPyramid row
Vault::cache_entry_without_image(uuid)  an ImageCacheEntry row whose imageId names no
                                    image: the dead row a deleted image leaves behind,
                                    whose payload is therefore an orphan
Vault::unclaimed(levels)            a pyramid no image claims
Vault::superseded(levels)           level files plus a deliberately unreadable .lrprev
Vault::unrecognised_file(shape)     payload-like, wrong shard / no suffix / bad uuid
Vault::preoccupied_destination(relative, bytes)  a file already in the output tree
Vault::same_destination_as(&Vault)  recover into another vault's `out`: two catalogs,
                                    one destination (M15)
Vault::raw_index(bytes)            bytes at `previews.db` instead of an index (a torn file)
Vault::raw_catalog(bytes)          bytes at the catalog instead of a database: a file a
                                    person picked believing it was a `.lrcat`, whose copy
                                    is carried and whose open is refused (`#149`)
Vault::with_wal()                  leave the catalog between checkpoints: half its
                                    image rows in the main file and half in the `-wal`
                                    beside it, which is what Lightroom leaves and what
                                    a copy that ignores the sidecar reads half of
                                    (`docs/browser-spike.md` §2)
Vault::write() -> Vault             catalog(), previews(), out(), plan(&Options),
                                    level_files(), containers()
```

Every scenario type carries a `raw(bytes)` override, so a byte-shaped row never
needs a fourth ad-hoc writer. `level_files()` and `containers()` report what
`write()` actually put on disk, so a vault's `shapes` counts are derived from the
composition rather than copied into a test.

### Absorbing the three copies, and where a test lives

The builder replaced all three implementations of the same bytes: the
`chunk`/`jpeg` helpers that `crates/lrsalvage-core/tests/pipeline.rs`,
`crates/lrsalvage-cli/tests/cli.rs` and `crates/lrsalvage-core/src/plan.rs`'s
`#[cfg(test)]` module each carried are gone. The plan's own tests came last: they
now live in `crates/lrsalvage-core/tests/plan.rs` on a `Vault`, next to
`tests/extract.rs` and `tests/exif.rs`, whose subjects are the writer and the
metadata reader. The matrix itself is asserted from two files —
`crates/lrsalvage-core/tests/matrix.rs` (rows whose medium is the plan) and
`crates/lrsalvage-cli/tests/matrix.rs` (runs: exit codes, report files, output
bytes) — so the existing suites stay about their own seams.

**A test that needs the builder lives in `tests/`, never in a `#[cfg(test)]`
module inside `src/`.** `lrsalvage-fixtures` is `publish = false`, so the
published manifest drops the dev-dependency, and a test under `src/` cannot be
dropped with it: excluding a source file leaves a crate that does not compile
(#123, and `architecture`'s *artifact carries a suite it can run*). Which files
ship is `cargo package --list`'s answer, and `scripts/check.sh package` runs each
tarball's suite outside the checkout.

A `src/` test that cannot import the builder still keeps bytes of its own, in
three places, and they are not equal. `exif`'s `tiny_jpeg` and `extract`'s
hand-built `Plan` are legitimate — a 4x4 frame and a `Plan` literal are not a
*vault* — and they are what the package carries, so it runs a suite where it
lands. `extract`'s `chunk`/`jpeg`/`payload_bytes` are debt: a fourth
implementation of `bytes::chunk` and `bytes::container`, older than this rule. It
stays because `extract`'s suite hand-builds the plan on purpose, to pin the
writer with no catalog and no index; moving it is its own slice, and the rule
reads "no *new* writer" until then.

### OS neutrality

Rows are built and asserted with `Path::join` and compared through one `slashed()`
helper, never by string concatenation with `/`. No row depends on case folding,
symlinks, file timestamps or a separator: the uuid hex, the shard folders and the
leaf are all lower-case ASCII, and no two names in a vault differ only by case —
except the pair that pins `plan`'s case-folded dedupe, the one state whose whole
point is breaking that rule.

## Vaults

A vault is the unit of a **cache-level** fact — the preview layout, whether the
index exists, whether a `.lrprev` coexists — because ADR 0004 decides those once
per cache. A per-image status is a row inside the vault. Exit codes are properties
of a **run**, not of an image, so each vault lists the invocations asserted against
it; a row's `Exit` is the code of the run it names.

| Vault | Cache holds | Layout | DB / CRS version | Runs → exit |
|---|---|---|---|---|
| `modern` | index + level files | `modern` | `1504001` / `18.0.0` | `scan` 0 · `extract` 0 · `extract` rerun 0 · `extract --all-levels` 0 · `extract --min-long-edge 99999` 3 · preoccupied foreign destination 2 · `--no-metadata` then `extract` 0 then 2 |
| `legacy` | index + `.lrprev` containers | `legacy` | `1200001` / `15.4.0` | `scan` 0 · `extract` 0 |
| `mixed` | index + both representations | `mixed` | `1504001` / `18.0.0` | `scan` 0 · `extract` 0 |
| `damaged` | index + level files, three broken winners | `modern` | `1504001` / `18.0.0` | `scan` 0 · `extract` 2 · `extract --all-levels` 2 · `extract --min-long-edge 99999` 3 |
| `empty` | index with no rows, no payload files | `none` | `1200001` / `15.4.0` | `scan` 3 · `extract` 3, no report file |
| `unindexed` | level files, no `previews.db` | `modern` | `1504001` / `18.0.0` | `scan` 3 · `extract` 3 · `--previews` that is not a directory 1 |
| `unrecognised` | index + payload-like files of no known shape | `unknown` | `9900001` / `99.0.0` | `scan` 3 · `extract` 3, no report file |

Version strings are reported verbatim and never interpreted. `unrecognised`
carries a version no release produced precisely because no version may refuse a
run: it exits 3 for "nothing recognised", never 1.

## Rows

Rows are per-image variations inside one vault, unless the row composes a state
that vault cannot hold — a row that builds its own vault says so in its prose, and
the section's pinned totals then describe the base vault rather than it. M15 is
the one row whose state needs two catalogs, and it says so. `Assertion` is `count`
(a plan or report field: status, stats, `declared`, `payload_metadata`, CSV
column, manifest member), `bytes` (the bytes of a written file), or `both` (the
item's fields and the file it produced). `Evidence` uses `format-notes`' labels
plus `synthetic` for a state this process has never observed. A row that composes
a state a run *did* observe carries both, as `verified §5.11 (state); synthetic
(bytes)`: the label on the left is what the measurement holds, the one on the
right is what the vault builds.

### `modern` — 14 images, 2 unclaimed pyramids

Pinned totals for a plain `plan`: `images 14`, `recovered 8`, `suspect 1`,
`missing_payload 3`, `no_cache_entry 1`, `original_present 1`, `unmapped 2`
(items), `deleted 1`, `superseded 0`, `below_floor 1`, `levels 9`,
`selected_images 8`.

| # | Variation | Builder produces | Status | Stats | Run | Exit | Assertion | Evidence | Pins |
|---|---|---|---|---|---|---|---|---|---|
| M1 | recovered, one level file | `pyramid(1616×1077)` + `level_file(1616)`, bare payload | `recovered` | `recoverable +1`, `levels +1` | `extract` | 0 | both | verified §5.2, §5.3 | the leaf is derived from the row, not globbed; output = payload verbatim + exactly one of our APP1 |
| M2 | recovered, five level files | rows and files for `101, 202, 404, 808, 1616` | `recovered` | `levels +1`; `101` is not an item | `extract` | 0 | count | verified §5.2 | largest level present wins the image |
| M3 | pruned level: the row's file is gone, a smaller level survives | rows `1616` and `808`; only `_808` on disk | `recovered` at 808×539 | `declared` = the 808 row | `extract` | 0 | count | unmeasured §5.5; the discard measured there leaves no stale row (§5.11) | a stale row is a note, not `missing-payload` (ADR 0006 ¶5) |
| M4 | level file found only by the scoped glob | row `1616` (file absent); `_640` file with no row | `recovered` at 640×427, `declared: None` | `levels +1` | `extract` | 0 | count+bytes | unmeasured §5.5 | the glob is bounded to the row's own `(uuid, digest)`; a file with no row cannot be `suspect` however small (ADR 0005 ¶3–¶4) |
| M5 | suspect | row `1616×1077`; file named `_1616` whose SOF is 800×600 | `suspect` | `suspect +1`, selected | `extract` | 0 | both | verified §5.2 | measured dimensions disagree with the row that produced the file; still written (ADR 0001) |
| M5b | swapped dimensions | row `1616×1077`; file SOF `1077×1616` | `recovered` | — | `extract` | 0 | count | verified §5.2 | width/height swapped is not a disagreement |
| M6 | `missing-payload` with a tombstone | row, no file, `DeletedPyramid(t)` | `missing-payload` | `missing_payload +1`, `deleted +1` | `scan` | 0 | count | synthetic; table verified §5.4 | the tombstone is a separate count and never a cause (ADR 0006 ¶5) |
| M7 | `missing-payload` without one | row, no file | `missing-payload` | `missing_payload +1` | `scan` | 0 | count | synthetic | the same status with no claimed cause |
| M8 | no index row | image row, no `ImageCacheEntry` | `no-cache-entry` | `no_cache_entry +1` | `scan` | 0 | count | — | |
| M9 | original still on disk | image + pyramid + level file + the original file present | `original-present` | `original_present +1`, not selected | `scan` | 0 | count | — | not a salvage target unless `--include-present` |
| M10 | unclaimed pyramid | level files for a `(uuid, digest)` no image claims, no `Pyramid` row | `unmapped` | `unmapped +1` | `scan` | 0 | count | verified §5.10; ADR 0005 ¶8 | one orphan per pyramid; listed, never written. A pyramid no entry claims and no row explains stays visible however small it is |
| M10c | two pyramids, one uuid | a claimed preview pyramid (`standard`, `AdobeRGB`, 320/960/1920/3840) and an unclaimed `thumbnail` pyramid (`sRGB`, 320), the thumbnail row written **last** | `recovered` at 3840×2560 | `unmapped 0`, `thumbnail_pyramids +1`, `shapes.level_file` counts both pyramids | `scan` | 0 | count | verified §5.10 | the entry's `(uuid, digest)` names the pyramid, so `color_profile`, `quality` and `best_level` cannot be decided by row order (ADR 0005 ¶1, ADR 0008) |
| M10d | `thumbnail` row, no live claim | a `thumbnail` pyramid whose uuid no cache entry names | `no-cache-entry` + `unmapped` | `unmapped +1`, `thumbnail_pyramids 0` | `scan` | 0 | count | synthetic; ADR 0008 | a uuid nothing maps keeps every pyramid visible, thumbnail included — the clause that keeps a lost photograph's last payload reportable |
| M10e | `thumbnail` row that lies about its size | a claimed pyramid beside a `thumbnail` row declaring 320, whose only file measures 3840 | `unmapped` | `unmapped +1`, `thumbnail_pyramids 0` | `scan` | 0 | count | synthetic; ADR 0008 | the row's word is never enough: the bytes must also be small, so a mislabelled row cannot hide a payload the tool would write |
| M10f | rescue from the companion | the claimed pyramid's rows with no files anywhere; its `thumbnail` companion holding one 320 file | `recovered` at 320×213, tier `thumbnail` | `recoverable +1`, `below_floor +1`, `missing_payload 0`, `thumbnail_pyramids 1` | `extract` with `--min-long-edge 0` | 0 | both | unmeasured §5.5 (attempted 2026-11-16, §5.11); ADR 0009 | a surviving payload is not `missing-payload`: it is read from the companion, judged against the companion's own row, counted against the floor, and written under the image's own name only when the floor allows. No measured deletion produces the state: a discard takes the companion too (§5.11) |
| M10g | a sibling that may not rescue | claimed pyramid's rows with no files, plus a `bigThumbnail` sibling holding a 320 file | `missing-payload` | `missing_payload +1`, `unmapped +1`, `thumbnail_pyramids 0` | `scan` | 0 | count | synthetic; ADR 0009 | the rescue is bound to the pyramid §5.10 measured a render writing; another quality's payload stays an orphan and the image stays unmapped |
| M10h | a discarded pyramid | one claimed pyramid in the state a discard leaves: `quality bigThumbnail`, levels `320`, `960` and `1920` with their files, no companion | `recovered` at 1920×1280, tier `large` | `quality bigThumbnail` on the **claimed** pyramid, `recoverable +1`, `levels +1`, `declared` = the 1920 row, `selected`, `unmapped 0`, `thumbnail_pyramids 0`, `missing_payload 0`, `below_floor 0` | `scan` | 0 | count | verified §5.11 (state); synthetic (bytes); ADR 0008, ADR 0009 | a demotion changes what `quality` means — the levels a pyramid currently holds, never the build that wrote it — and decides nothing else: the pair still claims the pyramid, so the largest surviving level is the best level and is written under the image's own name, while the demoted pyramid is neither an orphan nor a rescue candidate. Two arms of §5.11's state are omitted, each for its own reason: the leftover `DeletedPyramid` row names a pair that is also live, so `deleted` — which counts only per `missing-payload` item — would be 0 and the row would assert nothing; and the `final` pyramid a discard leaves alone is **unmeasured**: §5.11's cache
held that row but no 1:1 level for the command to delete, so that arm is the
command's name rather than a run's evidence, and it needs a second image, so it
is a row of its own if anyone wants it |
| M10b | an orphan whose names lie | one unclaimed pyramid, its `_640` file holding a 1616×1077 frame and its `_1616` file a 640×427 one | `unmapped` | `unmapped +1`, `shapes.suffix_matches_sof 0` | `scan` | 0 | count | synthetic; ADR 0005 ¶9 | the item names the file whose **bytes** are the largest — one rule in every mode, so an indexless cache ranks the same way (U1) |
| M11 | digest-mismatched file | row `(UUID, digestA)`; file `<UUID>-digestB_1616` | `missing-payload` + `unmapped` | `missing_payload +1`, `unmapped +1` | `scan` | 0 | count | synthetic; ADR 0005 ¶6 | only the pair maps, so the file is a second pyramid, not this one's level |
| M12 | `longDimension` stored REAL | row `longDimension 101.0`, file `_101` at 101×68 | `recovered` | `below_floor +1`, tier `thumbnail`, unselected | `scan` | 0 | count | verified §5.7 | the suffix is the rounded integer, and the floor excludes without failing |
| M13 | payload carrying its own EXIF, XMP and ICC | `jpeg_with(1616×1077, [exif_app1(6), xmp_app1(x), icc_app2(p)])`; catalog orientation `AB`, catalog XMP | `recovered` | `payload_metadata = exif,xmp,icc`, `effective_orientation = 6` | `extract` | 0 | both | verified §5.10 for the segments; synthetic for the payload's own orientation | ADR 0003's precedence: the payload's orientation and ICC win, our EXIF replaces its EXIF, its XMP packet is untouched. A render does write all three segments (§5.8), but its `Exif` carries no orientation tag, so the precedence's orientation arm is still exercised only here |
| M14 | bare payload, catalog claims orientation and profile | bare `jpeg(1616×1077)`; catalog `BC`, `Pyramid.colorProfile = 'AdobeRGB'` | `recovered` | `payload_metadata` empty | `extract` | 0 | bytes | verified §6.4 | `0x0112 = 6` is written from the catalog, our `AdobeRGB` profile is embedded, and `payload_metadata` stays empty: ours is not the payload's |
| M16 | a cap that demotes | level files for `320, 960, 1920, 3840`, one row each; `--max-long-edge 1920` | `recovered` at 1920×1280 | `above_cap 0` without `--all-levels`, `levels 1`, selected | `extract` | 0 | bytes | verified §5.12 (rule); synthetic (bytes); ADR 0013 | the cap is a winner rule, not a filter after one: the 1920 level keeps the image's plain name and is what gets written. The rule is measured against a real 1:1 cache (§5.12), where no level measures 1,920 and the cap still chooses the largest step beneath it; the vault here pins the boundary no run has exercised — a cap naming a level the cache holds |
| M17 | a cap no level clears | one `3840` level file; `--max-long-edge 1920` | `recovered` at 3840×2560, unselected | `above_cap 1`, `failed 0`, `selected_images 0` | `extract` | 3 | count | synthetic; ADR 0013 | the image is not `failed` and gets no new status: a cap bounds what is written, never what a payload is |
| M18 | `--all-levels` under both bounds | M16's ladder, `--min-long-edge 640 --max-long-edge 1920` | `recovered` at 1920×1280 | `levels 4`, `below_floor 1`, `above_cap 1`, `selected_items 2` | `extract` | 0 | count+bytes | synthetic; ADR 0013 | levels above the cap stay items and lose the write, exactly as levels below the floor do; the cap's choice keeps the plain name and the smaller survivor takes the ` - WxH` suffix |
| M19 | a destination without room for what is left to write | one image at 1616×1077 with its level file; the seam's free-space substitution set one byte under what the run has left to write | `recovered`, selected — composed, not asserted: the row's claim is the refusal | — (the number compared is the plan's total) | `extract` | 4 | empty tree | synthetic | the refusal fires once the plan exists and before the first write, so the exit code and an empty destination are one claim. **It cannot show a short disk**: too little free space on a destination is not a vault state a fixture can compose, and the number arrives through the seam's debug-only substitution (`specs/modules/fs.md`) — so this row pins the refusal and its exit code, never what a real full filesystem does to a run halfway through. The check's own boundary, one byte short refused and what is left proceeding, is the run suite's beside this row |

**No cap row appears in the run table below, and that is a decision.** Every run
there is uncapped, so none of its numbers moved; the cap's claim is what lands on
disk, and M16–M18 pin it with their own vaults. The cap also reaches the
companion rescue (M10f): one rule for a level, whichever pyramid held it. With
the default floor it cannot bind on a companion, because a classified companion's
every level measures under 640 px (ADR 0008, third clause) — but a floor lowered
with `--min-long-edge 0` leaves a cap that does, so a 320 px companion level under
`--max-long-edge 200` is `above_cap` and unselected, exactly as a claimed level of
that size would be.

**Runs on `modern`.**

| Run | Asserts |
|---|---|
| `scan` | the totals above; `layout`, `db_version`, `crs_version` in `--json` |
| `extract` | exit 0, 8 files written; each output starts with the payload's own scan data, ends with EOI, carries exactly one Exif APP1 whose `0x0131` starts with `lrsalvage `; no `.partial` survives |
| `extract` (rerun) | exit 0, 8 `skipped`, no byte rewritten (the marker prefix + completeness + SOF is what verifies) |
| `extract --all-levels` | exit 0; 15 items; `levels 15`, `below_floor 3`, `selected_items 12`; `images` and every status unchanged (M2 alone contributes 5 items and M10h three) |
| `extract --min-long-edge 99999` | exit 3, `nothing to extract`, nothing written |
| `extract` with `preoccupied_destination(M1, our_output(1616×1077))` | exit 0; M1 `skipped`, the seven others written |
| `extract` with `preoccupied_destination(M1, jpeg(1616×1077))` | exit 2; M1 `failed` with `destination exists and is not a verified extraction; use --force`; the foreign file is byte-for-byte untouched; a same-size complete JPEG without the marker is never ours |
| `extract --no-metadata` then `extract` | first run exit 0 and 8 outputs byte-identical to their payloads (no APP1 of ours); second run exit 2, 8 `failed`, every output untouched — a metadata-free run never verifies anything |
| `extract` (report files) | `report.csv` header ends `…,note,payload,payload_metadata`; an unmapped item carries `payload` and `-` for `output` and `original_path`; `manifest.json` carries `stats.superseded` and `stats.deleted` (0 and 1 here) and one `payload` member per unclaimed pyramid; `report.csv` never carries `superseded` or `deleted` |

### Two catalogs, one destination — row M15

A row inside one vault cannot express this state: it needs two catalogs, each with
its own cache and its own root folder, deriving the same output path into one
`out`. So the row stands on a table of its own and carries a `Cannot show` entry;
the run it names belongs to neither vault's `Runs` row above, because it needs
both. The second vault composes onto the first with `same_destination_as`.

| # | Variation | Builder produces | Status | Stats | Run | Exit | Assertion | Evidence | Pins | Cannot show |
|---|---|---|---|---|---|---|---|---|---|---|
| M15 | a second catalog recovering into a written destination | two `modern` vaults sharing one `out`: the first writes `IMG_0001.jpg` and `IMG_0002.jpg` at 1616×1077, `IMG_0003.jpg` at 960×1077, and an `IMG_0004.jpg` that already carries another tool's marker; the second derives all four names — its `IMG_0001` is a different 1616×1077 frame carrying its own XMP packet, its `IMG_0002` is 1616×640, its `IMG_0003` is 640×1077, and its `IMG_0004` is 1616×1077 | `recovered` and selected on all four; the run's outcomes are `skipped`, `failed`, `failed`, `failed` | — (the run prints `4 evaluated`, `0 written`, `1 skipped`, `3 failed`) | `extract` on the second catalog, after the first catalog's own `extract` | 2 | both | synthetic | the identity is the marker's `lrsalvage ` prefix, a complete JPEG and matching `SOF` dimensions — so another photograph at the same name and the same size is `skipped`, its own bytes never written. The marker and both halves of the size check have a destination only this row decides: a size sharing the width, a size sharing the height, and the same size behind a marker that names this tool without preceding with it (`lrsalvage-compatible importer 1.2`, which a `contains` test would wrongly keep) are every one of them `failed`, never skipped. Completeness is not this row's — every file it composes is a whole JPEG — and neither is the `--force` that does not replace a collided file: both are `extract`'s own unit tests. Strengthening the identity is refused until a report of the collision it would prevent (`adr-0003-preserve-preview-bytes`, *Resume identity*) | this is **our** limitation, not Adobe's behaviour: no measured cache produces a collision, and the state is composed from two catalogs this repository builds. The row says nothing about what Lightroom does when two catalogs share a folder |

Exit 2 is the run's, not the collision's: one item is skipped while three are
refused, and only the skip is the limitation.

### `legacy` — 5 images, 1 unclaimed pyramid

Totals: `images 5`, `recovered 2`, `suspect 1`, `missing_payload 1`,
`no_cache_entry 1`, `unmapped 1`, `levels 3`, `selected_images 3`.

| # | Variation | Builder produces | Status | Stats | Run | Exit | Assertion | Evidence | Pins |
|---|---|---|---|---|---|---|---|---|---|
| L1 | recovered from a container | `container([101, 404, 1616])`, rows for all three | `recovered` at 1616×1077 | `recoverable +1` | `extract` | 0 | both | verified §1 | the `Payload::Container` arm writes the chunk the plan certified |
| L2 | container levels out of order | chunks `level_1=101, level_2=1616, level_3=404` | `recovered` at 1616×1077 | — | `extract` | 0 | count | verified §1 | ranking is by decoded area, not by chunk number |
| L3 | suspect from a container | best chunk 800×600; row 1616×1077 | `suspect` | `suspect +1` | `extract` | 0 | count | verified §1 | a container keeps its comparison (ADR 0001) |
| L4 | `missing-payload` | row, no `.lrprev` on disk | `missing-payload` | `missing_payload +1` | `scan` | 0 | count | ADR 0006 ¶5 | |
| L5 | no index row | image, no row | `no-cache-entry` | `no_cache_entry +1` | `scan` | 0 | count | — | |
| L6 | unclaimed container | `.lrprev` no image claims | `unmapped` | `unmapped +1` | `scan` | 0 | count | ADR 0005 ¶8 | an unclaimed container is one orphan pyramid |
| L7 | a cap over a container | `container([101, 808, 1616])`, a row for each; `--max-long-edge 1000` | `recovered` at 808×539 | `above_cap 0`, `suspect 0` | `extract` | 0 | bytes | synthetic; ADR 0013 | the container is ranked by decoded area, and the chunk a cap chooses is judged against the row that declares its size — not the pyramid's best row, which would flag an ordinary level `suspect`. The row builds its own vault, so the totals above describe the base one |

Container framing failures (a chunk length running past EOF) are **not** a row:
`failed` is already pinned three ways in `damaged`, and the byte walk is
`lrprev`'s own unit tests. Stated here so the omission reads as a decision.

### `mixed` — 3 images

Totals: `images 3`, `recovered 3`, `superseded 1`, `levels 3`, `selected 3`.

| # | Variation | Builder produces | Status | Stats | Run | Exit | Assertion | Evidence | Pins |
|---|---|---|---|---|---|---|---|---|---|
| X1 | superseded pyramid | 1616 level file + a `.lrprev` that is deliberately not a valid container | `recovered` from the level file | `recoverable +1`, `superseded +1` | `extract` | 0 | both | unmeasured §5.5 | the level files are the pyramid and the container is never read: reading it would fail the row, and its bytes are asserted unchanged (ADR 0006 ¶6) |
| X2 | `.lrprev`-only pyramid in a mixed cache | container only | `recovered` from the container | `recoverable +1` | `extract` | 0 | count | unmeasured §5.5 | coexistence resolves per pyramid, not per cache |
| X3 | level-file-only pyramid in a mixed cache | level file only | `recovered` from the file | `recoverable +1` | `extract` | 0 | count | verified §5.2 | |

`layout` is `mixed` for the vault and is asserted once, not per row.

### `damaged` — 4 images

Totals: `images 4`, `failed 3`, `recovered 1`, `levels 4`, `selected_images 1`.

| # | Variation | Builder produces | Status | Stats | Run | Exit | Assertion | Evidence | Pins |
|---|---|---|---|---|---|---|---|---|---|
| D1 | truncated winner, a good smaller level beside it | rows `1616` and `808`; `_1616` = `truncated_jpeg`, `_808` intact | `failed` | `failed +1` | `extract` | 2 | count+bytes | synthetic | no quiet fallback to the smaller level (ADR 0005 ¶6); nothing is written for the image |
| D2 | non-JPEG bytes at the derived path | `_1616` = `not_a_jpeg()` | `failed` | `failed +1` | `extract` | 2 | count | synthetic | |
| D3 | no SOI | `_1616` = `no_soi(1616, 1077)` | `failed` | `failed +1` | `extract` | 2 | count | synthetic | nothing is scanned for `FF D8` to reach a frame behind the damage |
| D4 | intact sibling | `_1616` intact, row agrees | `recovered` | `recoverable +1`, selected | `extract` | 2 | bytes | verified §5.2 | one failure makes the run partial without stopping the others; the file is written |
| D5 | `--all-levels` over D1 | the D1 pyramid | image stays `failed`; two items | `levels +2` | `extract --all-levels` | 2 | count+bytes | synthetic | every item is judged against its own row, the image keeps one status, and the good 808 is written as the suffixed name (` - 808x539`) while the plain name belongs to the failed 1616 (`--all-levels`, settled in #12) |

`extract --min-long-edge 99999` on this vault exits **3**, not 2: nothing is
selected, so `extract` never starts. A vault of failures alone therefore reads as
"nothing to do", which is why the recovered sibling exists.

### `empty` — 2 images

| # | Variation | Builder produces | Status | Stats | Run | Exit | Assertion | Evidence | Pins |
|---|---|---|---|---|---|---|---|---|---|
| E1 | an index that holds no rows | `previews.db` with the modern schema and no rows; no payload files | `no-cache-entry` for both images | `images 2`, `no_cache_entry 2`, layout `none` | `scan` | 3 | count | synthetic | "this cache holds nothing" is a layout, not an error |
| E2 | nothing selected, nothing unclaimed | the same vault | — | — | `extract` | 3 | count | ADR 0006 ¶7 | no `report.csv`, no `manifest.json`, no JPEG: an empty run is not something to report on. `scan --report` on the same vault **does** write its file (R6): the two artifacts answer different questions |

### `unindexed` — 2 images, 2 pyramids

| # | Variation | Builder produces | Status | Stats | Run | Exit | Assertion | Evidence | Pins |
|---|---|---|---|---|---|---|---|---|---|
| U1 | no `previews.db` | level files for two pyramids (one with two levels); catalog with two images | `no-cache-entry` for both | `images 2`, `no_cache_entry 2`, `unmapped 2`, layout `modern` | `scan` | 3 | count | verified §5.2 + ADR 0006 ¶1 | identity survives without rows: the leaf gives `(uuid, digest)`, the shard must agree; one hint names the missing file |
| U2 | the run still hands over its list | the same vault | — | — | `extract` | 3 | count | ADR 0006 ¶4, ¶7 | `report.csv` and `manifest.json` are written because `unmapped > 0`; the output tree holds no JPEG |
| U3 | a mistyped `--previews` | the same vault, `--previews` pointing at a file | — | — | `scan` | 1 | count | ADR 0006 ¶3 | a path that is not a directory is a usage error, never an empty cache |
| U4 | a torn `previews.db` | the same vault with `.raw_index` bytes SQLite cannot read | `no-cache-entry`, the payload still listed | `images 1`, `unmapped 1`, layout `modern` | `scan` | 3 | count | ADR 0006 ¶2 | a damaged index degrades exactly like a missing one: the hint carries `could not be read (sqlite error: …)` and `fix or move the file to name them`, and no fatal SQLite line escapes |

### `unrecognised` — 1 image

| # | Variation | Builder produces | Status | Stats | Run | Exit | Assertion | Evidence | Pins |
|---|---|---|---|---|---|---|---|---|---|
| N1 | payload-like files of no known shape | three JPEGs: right shard with no numeric suffix; right leaf in the wrong shard; a leaf whose uuid does not parse | `missing-payload` | `missing_payload 1`, `unmapped 0`, `unrecognised 3`, layout `unknown` | `scan` | 3 | count | synthetic; ADR 0005 ¶6 | unrecognised is not an orphan: no item, no output path, one hint; the pyramid has no file at its own address |
| N2 | a version no release produced | `Adobe_DBVersion 9900001`, `Adobe_CrsVersion 99.0.0` | — | both strings verbatim | `scan --json` | 3 | count | ADR 0004 | no version refuses a run: a newer number reports, it does not error |
| N3 | nothing to report on | the same vault | — | — | `extract` | 3 | count | ADR 0006 ¶7 | no extract report file: `selected 0` and `unmapped 0`. `scan --report` still writes the verification report (R4) |

`unrecognised 3` and `layout unknown` are asserted in `--json`,
`manifest.json` and the verification report (R4). Neither is a status, a CSV column or an exit code: the count says
what the cache held, not what any image became (`adr-0004-detect-layout-not-version`).

## The verification report — eleven rows

A report row is a property of a **run**, not of an image, so these are listed once
here instead of being repeated in each vault's `Runs` table; each row names the
vault it is asserted against. `count` rows read the parsed object's fields, `bytes`
rows read the file back, `both` rows do one then the other. R2 is the row this
whole artifact exists for, and it is the matrix's first **negative** assertion: it
fails if a single path, uuid, digest, shard folder or leaf name reaches the file.

| # | Vault | Run | Exit | Asserts | Pins |
|---|---|---|---|---|---|
| R1 | `modern` | `scan --report <out>/scan.json` | 0 | count: the object's keys are exactly the frozen set (`schema`, `version`, `generated_unix`, `layout`, `db_version`, `crs_version`, `source_sha256`, `snapshot_sha256`, `options`, `stats`, `shapes`); `layout modern`; `db_version 1504001`; `crs_version 18.0.0`; both digests `null` and `options.verify_source false`; the other `options` echo `640`, `false`, `false`; every `stats` member equals the vault's pinned totals; `shapes.level_file.count` equals the level files built, `suffix_matches_sof` exactly one lower — M5's `_1616` file measures 800×600 — and `container.count 0` with no template | the report describes the run, and its field set is a contract, not a snapshot of the plan; a flag moves no member, only a value |
| R2 | `modern` | the bytes R1 wrote | 0 | bytes: the file contains none of the vault's absolute paths (catalog, cache, output root), none of its uuids, none of its digests, none of its shard folder names and none of its leaf names | the regression the decision exists for: one leaked string fails the row. Compared against names the **builder** produced, never a hard-coded sample |
| R3 | `mixed` | `scan --report` | 0 | count: `shapes.level_file.count` and `shapes.container.count` are both non-zero and each carries its own template; `stats.superseded 1` | a mixed cache publishes two grammars, which is why the template belongs to the class rather than to the cache |
| R4 | `unrecognised` | `scan --report` | 3 | count: `layout unknown`; `stats.unrecognised 3`; `shapes.level_file.count 0` and `shapes.container.count 0`, both templates absent; the file exists although the run wrote nothing else, and the one hint still prints on stderr | an exit-3 run still hands over its report; the histogram has no bucket for "no shape" (ADR 0005 ¶6) |
| R5 | `unindexed` | `scan --report` | 3 | count: `unmapped 2`, `no_cache_entry 2`, `layout modern`, `shapes.level_file.count` equals every level file the builder wrote — derived from the leaf alone, with no rows to consult — and the file exists | the shape facts survive without an index; the hint and the report serve different readers |
| R6 | `empty` | `scan --report` | 3 | count: the file exists; `images 2`, `no_cache_entry 2`, `recovered 0`, `layout none`, every `shapes` count `0` | a requested artifact is written even when nothing was selected — the contrast with E2, where `extract` writes no report at all |
| R7 | `modern` | `--report` naming a file that already exists | 0 | count+bytes: the run writes `scan (2).json`, says so on stderr, and the existing file is byte-identical afterwards | a claimed name, never an overwrite (`cli`) |
| R8 | `modern` | `--report` whose parent is inside the cache | 4 | count: exit `4`, no file inside the cache and no file anywhere else | every named write obeys the destination rule, and a failed report write is a destination error rather than "nothing selected" |
| R9 | `modern` | `scan --json --report` (and `--verbose`) | 0 | count: stdout parses as exactly one object with the `--json` field set, `--verbose` lines are on stderr, and the report file is written and path-free | the two surfaces do not mix: a file for a stranger, one object for a script |
| R10 | `modern` | `scan --verify-source --report` | 0 | count: `source_sha256` and `snapshot_sha256` are lowercase hex and equal, `source_sha256` equals `snapshot::sha256_file` of **the vault's catalog** — the member's meaning, not its presence — and `options.verify_source true` | the promise this release exists for: the copy is the catalog, published as a number a stranger can compare |
| R11 | `modern` | `extract --verify-source` | 0 | count: the run writes its JPEGs and exits `0`, says nothing about a digest on stderr, and neither `report.csv` nor `manifest.json` gains a member — no `source_sha256`, `snapshot_sha256` or `verify_source` | the flag proves the acquisition on the writing run without becoming a second report surface |

The **mismatch** — a copy that is not the catalog — is not a run row. No test can
make this binary change the user's catalog under itself without a test-only hook
in production code, so it is asserted where it is reachable: `snapshot`'s
`a_copy_that_is_not_the_catalog_fails_naming_both_digests` drives `verify_copy`
with a tampered copy and asserts the error and its stated message, and `main.rs`'
unit test pins that error's exit code to `5`. The gap is stated here rather than
papered over by a row that would pass by never reaching the branch.

## Conventions

- **Counts and bytes.** `count` rows assert the plan and the report: `Status`,
  `Stats`, `declared`, `payload_metadata`, `effective_orientation`, a CSV column,
  a manifest member. `bytes` rows assert the written file: its frame, its segments
  and that the payload's compressed data appears verbatim. `both` rows assert the
  claim and then the bytes it produced, in that order.
- **A run's exit code is asserted once, in the cli crate.** The `Exit` column
  repeats it so the table reads as a contract; the tests assert it per invocation,
  next to the report files and the output tree that invocation produced.
- **Note prose is never asserted.** The notes that say "the row is stale" or quote
  a tombstone timestamp are for humans; the structured fields a note describes —
  `status`, `declared`, `width`, `height`, `deleted` — are what a row pins
  (`report.md`). Existing assertions on note text in `pipeline.rs` move onto those
  fields when the fixtures migrate.
- **A row whose state is ours carries `Cannot show`.** M15 composes a state no run
  over a real cache can produce — two catalogs, one destination — so its table adds
  that entry, naming what the row therefore cannot establish about Lightroom. Every
  other row's limit is already the label in `Evidence`, so no other table carries
  the column.
- **Row ids are stable.** A row is referenced by id (`M3`, `X1`, …) from code
  comments, the resolution of the ticket that added it, and later specs.
- **Every row is built, never loaded.** No vault, no payload and no SQLite file is
  committed.
- **Cache-level counts are asserted per vault**, not per row: `superseded` in
  `mixed`, `deleted` in `modern`, `unrecognised` in `unrecognised`, and the three
  `shapes` counts wherever the vault has payload files.
- **A negative row asserts absence, and only absence.** `!bytes.contains(…)` over
  every identifying string the builder produced for that vault is the one shape a
  promise like "never a full path" can be tested in; it asserts nothing else, so
  the row cannot pass by being empty. A one-character shard folder is asserted
  through the path fragment it would appear in, because a single letter is not a
  string whose absence means anything.

## What has no row, and why

- **Container framing failures** — `lrprev`'s unit tests; `failed` is already
  pinned three ways in `damaged`.
- **Virtual copies and AI masks** — out of scope for this effort (the map's Notes).
- **The contact sheet** — `report.rs`'s own tests build `PlanItem`s directly; the
  sheet's `smallest_level` rule (#12) is a report seam, not a cache shape, and the
  matrix adds no row that would need an HTML parser.

## Recorded as a spec, not an ADR

No ADR. The builder's home is a `cargo` move, the vault split is a test-layout
choice, and neither is hard to reverse; the decisions that are hard to reverse —
parse don't carve, map via `previews.db`, detect layout not version, discover by
index, degrade without guessing, preserve the payload's bytes — are ADRs 0001–0006
and are cited, not restated. What this spec adds is *coverage*: which shapes exist,
which medium proves each promise, and where the bytes come from.

## What this changes elsewhere

- `goal-and-requirements` — a Verification statement: every promise is proved by a
  synthetic vault from one shared builder.
- `architecture` — an invariant: the published artifact carries a suite it can
  run, so a test that needs the builder lives in `tests/` and never in a
  `#[cfg(test)]` module inside `src/`.
- `plan` — the `deleted` stat, named and counted per `missing-payload` item whose
  pyramid carries a `DeletedPyramid` row.
- `report` — where `deleted` and `unrecognised` surface (a summary line when
  non-zero, `--json`, `manifest.json`, never `report.csv`), beside `superseded`.
- `format-notes` §5.8 — the metadata-bearing-payload TODO becomes a pointer to
  row M13, which pins the order and cannot show what Adobe writes; §5.5 gains a
  cross-reference from each state it leaves unmeasured to the row that pins the
  reader's behaviour in it — M3, M4, X1, X2 and M10f — and to M13 as the
  `unverified` one.
