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
reached only from `#[ignore]`d tests via `LRSALVAGE_TEST_CATALOG` and
`LRSALVAGE_TEST_PREVIEWS`; real catalog bytes are never committed.

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
Vault::image(ImageRow)              catalog row, original path, orientation, XMP blob
Vault::pyramid(PyramidRow)          Pyramid + PyramidLevel rows, profile and quality names
Vault::level_file(level)            a level file at its derived leaf, or with raw bytes
Vault::row_without_file(level)      a PyramidLevel row whose file is absent (pruned)
Vault::file_without_row(level)      a level file the widened glob must find
Vault::tombstone(timestamp)         a DeletedPyramid row
Vault::unclaimed(levels)            a pyramid no image claims
Vault::superseded(levels)           level files plus a deliberately unreadable .lrprev
Vault::unrecognised_file(shape)     payload-like, wrong shard / no suffix / bad uuid
Vault::preoccupied_destination(relative, bytes)  a file already in the output tree
Vault::write() -> Vault             catalog(), previews(), out(), plan(&Options)
```

Every scenario type carries a `raw(bytes)` override, so a byte-shaped row never
needs a fourth ad-hoc writer.

### Absorbing the three copies

The builder replaces all three existing implementations of the same bytes, in the
same change that adopts it:

- `crates/lrsalvage-core/tests/pipeline.rs` — `chunk` `:744`, `jpeg` `:766`;
- `crates/lrsalvage-cli/tests/cli.rs` — `chunk` `:52`, `jpeg` `:70`, `jpeg_files`
  `:219`;
- `crates/lrsalvage-core/src/plan.rs` (test module) — `chunk` `:896`, `jpeg` `:912`.

`pipeline.rs` and `cli.rs` migrate onto the builder; they do not keep private
fixtures beside it. The matrix itself is asserted from two new files —
`crates/lrsalvage-core/tests/matrix.rs` (rows whose medium is the plan) and
`crates/lrsalvage-cli/tests/matrix.rs` (runs: exit codes, report files, output
bytes) — so the existing suites stay about their own seams.

### OS neutrality

Rows are built and asserted with `Path::join` and compared through one `slashed()`
helper, never by string concatenation with `/`. No row depends on case folding,
symlinks, file timestamps or a separator: the uuid hex, the shard folders and the
leaf are all lower-case ASCII, and no two names in a vault differ only by case.

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

Rows are per-image variations inside one vault. `Assertion` is `count` (a plan or
report field: status, stats, `declared`, `payload_metadata`, CSV column, manifest
member), `bytes` (the bytes of a written file), or `both` (the item's fields and
the file it produced). `Evidence` uses `format-notes`' labels plus `synthetic` for
a state this process has never observed.

### `modern` — 13 images, 2 unclaimed pyramids

Pinned totals for a plain `plan`: `images 13`, `recovered 7`, `suspect 1`,
`missing_payload 3`, `no_cache_entry 1`, `original_present 1`, `unmapped 2`
(items), `deleted 1`, `superseded 0`, `below_floor 1`, `levels 8`,
`selected_images 7`.

| # | Variation | Builder produces | Status | Stats | Run | Exit | Assertion | Evidence | Pins |
|---|---|---|---|---|---|---|---|---|---|
| M1 | recovered, one level file | `pyramid(1616×1077)` + `level_file(1616)`, bare payload | `recovered` | `recoverable +1`, `levels +1` | `extract` | 0 | both | verified §5.2, §5.3 | the leaf is derived from the row, not globbed; output = payload verbatim + exactly one of our APP1 |
| M2 | recovered, five level files | rows and files for `101, 202, 404, 808, 1616` | `recovered` | `levels +1`; `101` is not an item | `extract` | 0 | count | verified §5.2 | largest level present wins the image |
| M3 | pruned level: the row's file is gone, a smaller level survives | rows `1616` and `808`; only `_808` on disk | `recovered` at 808×539 | `declared` = the 808 row | `extract` | 0 | count | unmeasured §5.5 | a stale row is a note, not `missing-payload` (ADR 0006 ¶5) |
| M4 | level file found only by the scoped glob | row `1616` (file absent); `_640` file with no row | `recovered` at 640×427, `declared: None` | `levels +1` | `extract` | 0 | count+bytes | unmeasured §5.5 | the glob is bounded to the row's own `(uuid, digest)`; a file with no row cannot be `suspect` however small (ADR 0005 ¶3–¶4) |
| M5 | suspect | row `1616×1077`; file named `_1616` whose SOF is 800×600 | `suspect` | `suspect +1`, selected | `extract` | 0 | both | verified §5.2 | measured dimensions disagree with the row that produced the file; still written (ADR 0001) |
| M5b | swapped dimensions | row `1616×1077`; file SOF `1077×1616` | `recovered` | — | `extract` | 0 | count | verified §5.2 | width/height swapped is not a disagreement |
| M6 | `missing-payload` with a tombstone | row, no file, `DeletedPyramid(t)` | `missing-payload` | `missing_payload +1`, `deleted +1` | `scan` | 0 | count | synthetic; table verified §5.4 | the tombstone is a separate count and never a cause (ADR 0006 ¶5) |
| M7 | `missing-payload` without one | row, no file | `missing-payload` | `missing_payload +1` | `scan` | 0 | count | synthetic | the same status with no claimed cause |
| M8 | no index row | image row, no `ImageCacheEntry` | `no-cache-entry` | `no_cache_entry +1` | `scan` | 0 | count | — | |
| M9 | original still on disk | image + pyramid + level file + the original file present | `original-present` | `original_present +1`, not selected | `scan` | 0 | count | — | not a salvage target unless `--include-present` |
| M10 | unclaimed pyramid | level files for a `(uuid, digest)` no image claims | `unmapped` | `unmapped +1` | `scan` | 0 | count | synthetic; ADR 0005 ¶8 | one orphan per pyramid; listed, never written |
| M11 | digest-mismatched file | row `(UUID, digestA)`; file `<UUID>-digestB_1616` | `missing-payload` + `unmapped` | `missing_payload +1`, `unmapped +1` | `scan` | 0 | count | synthetic; ADR 0005 ¶6 | only the pair maps, so the file is a second pyramid, not this one's level |
| M12 | `longDimension` stored REAL | row `longDimension 101.0`, file `_101` at 101×68 | `recovered` | `below_floor +1`, tier `thumbnail`, unselected | `scan` | 0 | count | verified §5.7 | the suffix is the rounded integer, and the floor excludes without failing |
| M13 | payload carrying its own EXIF, XMP and ICC | `jpeg_with(1616×1077, [exif_app1(6), xmp_app1(x), icc_app2(p)])`; catalog orientation `AB`, catalog XMP | `recovered` | `payload_metadata = exif,xmp,icc`, `effective_orientation = 6` | `extract` | 0 | both | unverified §5.8 | ADR 0003's precedence: the payload's orientation and ICC win, our EXIF replaces its EXIF, its XMP packet is untouched |
| M14 | bare payload, catalog claims orientation and profile | bare `jpeg(1616×1077)`; catalog `BC`, `Pyramid.colorProfile = 'AdobeRGB'` | `recovered` | `payload_metadata` empty | `extract` | 0 | bytes | verified §6.4 | `0x0112 = 6` is written from the catalog, our `AdobeRGB` profile is embedded, and `payload_metadata` stays empty: ours is not the payload's |

**Runs on `modern`.**

| Run | Asserts |
|---|---|
| `scan` | the totals above; `layout`, `db_version`, `crs_version` in `--json` |
| `extract` | exit 0, 7 files written; each output starts with the payload's own scan data, ends with EOI, carries exactly one Exif APP1 whose `0x0131` starts with `lrsalvage `; no `.partial` survives |
| `extract` (rerun) | exit 0, 7 `skipped`, no byte rewritten (the marker prefix + completeness + SOF is what verifies) |
| `extract --all-levels` | exit 0; 12 items; `levels 12`, `below_floor 2`, `selected_items 10`; `images` and every status unchanged (M2 alone contributes 5 items) |
| `extract --min-long-edge 99999` | exit 3, `nothing to extract`, nothing written |
| `extract` with `preoccupied_destination(M1, our_output(1616×1077))` | exit 0; M1 `skipped`, the six others written |
| `extract` with `preoccupied_destination(M1, jpeg(1616×1077))` | exit 2; M1 `failed` with `destination exists and is not a verified extraction; use --force`; the foreign file is byte-for-byte untouched; a same-size complete JPEG without the marker is never ours |
| `extract --no-metadata` then `extract` | first run exit 0 and 7 outputs byte-identical to their payloads (no APP1 of ours); second run exit 2, 7 `failed`, every output untouched — a metadata-free run never verifies anything |
| `extract` (report files) | `report.csv` header ends `…,note,payload,payload_metadata`; an unmapped item carries `payload` and `-` for `output` and `original_path`; `manifest.json` carries `stats.superseded` and `stats.deleted` (0 and 1 here) and one `payload` member per unclaimed pyramid; `report.csv` never carries `superseded` or `deleted` |

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
| E2 | nothing selected, nothing unclaimed | the same vault | — | — | `extract` | 3 | count | ADR 0006 ¶7 | no `report.csv`, no `manifest.json`, no JPEG: an empty run is not something to report on |

### `unindexed` — 2 images, 2 pyramids

| # | Variation | Builder produces | Status | Stats | Run | Exit | Assertion | Evidence | Pins |
|---|---|---|---|---|---|---|---|---|---|
| U1 | no `previews.db` | level files for two pyramids (one with two levels); catalog with two images | `no-cache-entry` for both | `images 2`, `no_cache_entry 2`, `unmapped 2`, layout `modern` | `scan` | 3 | count | verified §5.2 + ADR 0006 ¶1 | identity survives without rows: the leaf gives `(uuid, digest)`, the shard must agree; one hint names the missing file |
| U2 | the run still hands over its list | the same vault | — | — | `extract` | 3 | count | ADR 0006 ¶4, ¶7 | `report.csv` and `manifest.json` are written because `unmapped > 0`; the output tree holds no JPEG |
| U3 | a mistyped `--previews` | the same vault, `--previews` pointing at a file | — | — | `scan` | 1 | count | ADR 0006 ¶3 | a path that is not a directory is a usage error, never an empty cache |

### `unrecognised` — 1 image

| # | Variation | Builder produces | Status | Stats | Run | Exit | Assertion | Evidence | Pins |
|---|---|---|---|---|---|---|---|---|---|
| N1 | payload-like files of no known shape | three JPEGs: right shard with no numeric suffix; right leaf in the wrong shard; a leaf whose uuid does not parse | `missing-payload` | `missing_payload 1`, `unmapped 0`, `unrecognised 3`, layout `unknown` | `scan` | 3 | count | synthetic; ADR 0005 ¶6 | unrecognised is not an orphan: no item, no output path, one hint; the pyramid has no file at its own address |
| N2 | a version no release produced | `Adobe_DBVersion 9900001`, `Adobe_CrsVersion 99.0.0` | — | both strings verbatim | `scan --json` | 3 | count | ADR 0004 | no version refuses a run: a newer number reports, it does not error |
| N3 | nothing to report on | the same vault | — | — | `extract` | 3 | count | ADR 0006 ¶7 | no report file: `selected 0` and `unmapped 0` |

`unrecognised 3` and `layout unknown` are asserted in `--json` and
`manifest.json`. Neither is a status, a CSV column or an exit code: the count says
what the cache held, not what any image became (`adr-0004-detect-layout-not-version`).

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
- **Row ids are stable.** A row is referenced by id (`M3`, `X1`, …) from code
  comments, the resolution of the ticket that added it, and later specs.
- **Every row is built, never loaded.** No vault, no payload and no SQLite file is
  committed.
- **Cache-level counts are asserted per vault**, not per row: `superseded` in
  `mixed`, `deleted` in `modern`, `unrecognised` in `unrecognised`.

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
- `plan` — the `deleted` stat, named and counted per `missing-payload` item whose
  pyramid carries a `DeletedPyramid` row.
- `report` — where `deleted` and `unrecognised` surface (a summary line when
  non-zero, `--json`, `manifest.json`, never `report.csv`), beside `superseded`.
- `format-notes` §5.8 — the metadata-bearing-payload TODO becomes a pointer to
  row M13, which pins the order and cannot show what Adobe writes; §5.5 gains a
  cross-reference to rows M3, M4 and X1 as the unmeasured states.
