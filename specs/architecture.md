---
id: architecture
type: architecture-design
title: "lrsalvage architecture"
status: draft
parent: lrsalvage
references:
  - format-notes
  - glossary
  - adr-0010-what-0-2-promises
  - adr-0011-0-3-is-the-trust-minor
  - adr-0012-two-release-lines
---

# lrsalvage architecture

## Shape

Two crates ship. `lrsalvage-core` is the whole pipeline and the **supported seam**:
its public API is what a GUI, a script or a test drives, so moving a supported
item, changing its signature or hiding a module that carries one is a decision to
record here rather than a refactor. Which items those are is written down in
[The supported seam](#the-supported-seam) below — the seam is a named list, not
everything that is `pub`. `lrsalvage-cli` is one adapter over it — arguments,
presentation, progress, exit codes — and carries no format knowledge.

A third workspace member, `lrsalvage-fixtures`, is test-only: a `publish = false`
dev-dependency of both crates that builds the synthetic vaults `fixtures`
requires. Nothing ships with it, neither crate links it into a binary, and it is
not a module in the table below — it exists so the byte shapes and the catalog
fixtures have exactly one implementation (`fixtures`).

The pipeline is a straight line, and every stage is pure with respect to the one
before it:

```
snapshot ─┬─► catalog ──────┐
          └─► previews.db ──┴─► plan ─┬─► extract ─► output tree
                                      └─► report  ─► summary / CSV / JSON / sheet
                                                  ─► verification report
```

`plan` is the hinge. It is cheap, printable, and write-free — which is why `scan`
can be a full rehearsal of `extract`, and why `extract` never has to decide
anything about an image, only write what the plan selected.

## The supported seam

**This file is `lrsalvage-core`'s crate-level spec.** The spec graph has no
separate node for the crate: `lrsalvage` states the goal, this file states the
crate's shape, and the module specs hang off it one level down. So the seam has
exactly one home, and it is this section.

The seam is **not** the compatibility contract. The contract — the status words,
the exit codes, the machine-readable field sets, the report columns, the output
names, the flags and the marker — is what a script or a person may rely on, and
`adr-0010-what-0-2-promises` owns it. Nothing here restates it. The seam is the
other question: which Rust items a second front-end may build on in order to reach
that contract. The two are not synonyms (`glossary`, *compatibility contract*).

### Why a list, and not "everything `pub`"

The crate exports `pub mod` for all fourteen modules plus a twelve-name re-export
of the data types, and it has no facade wide enough to make a `pub` item
deliberate: visibility is the default here, not a promise. Three consumers are
why: `lrsalvage-cli` drives the pipeline, `lrsalvage-fixtures` builds vaults
through the format modules, and the tests reach the internals of the pipeline they
assert on. Only the first is a front-end under this section; a GUI would be the
second.

### The rule

Three questions, asked in this order. The first one that answers, decides.

1. **Seam** — a front-end receives the item from a supported call, or must build
   it to make one. `Plan::build` takes a `Catalog` and a `PreviewIndex`, so those
   and their own types are seam; `PlanItem` is what comes back, so its fields are
   seam. The seam is closed under the signatures of its own items: a type named
   there is in it.
2. **Convenience** — a shorthand over a seam call, or a helper that composes one.
   It may be added, renamed or dropped without a decision, provided the call it
   delegates to keeps the behaviour the seam gives it.
3. **Not supported** — public because the pipeline is composed across modules and
   the tests and the fixture builder reach in. Anything here may change, or be
   narrowed to `pub(crate)`, without a decision; that is how the surface shrinks.

The seam is the **items**, not the modules: a module is public because an item
lives in it, and the `pub use` block in `lib.rs` re-exports twelve seam types from
`error`, `model` and `plan` rather than adding any. Each item is named once below,
at its own module. A `pub` field travels with its type — a seam type's fields are
seam, because `Options` is built as a literal and a `PlanItem` is read field by
field.

### The inventory

| Item | Tier | Public because |
|---|---|---|
| `snapshot::CatalogSnapshot`, `.capture`, `.path` | seam | the stable copy every other opener takes by reference |
| `catalog::Catalog`, `::from_snapshot`, `::images`, `::xmp`, `::versions` | seam | the catalog queries a run begins with, over a snapshot |
| `catalog::CatalogVersions` (+ `db_version`, `crs_version`) | seam | what `versions` hands back; `CacheFacts` carries the same two strings |
| `catalog::Catalog::open` | convenience | `capture` + `from_snapshot` in one call |
| `catalog::Catalog::image_count` | convenience | a count without materialising `images()`; no in-tree caller |
| `previews_db::open`, `Opened`, `IndexProblem` | seam | the only way in degrades with a reason, and a front-end must hold the index `Plan::build` takes |
| `previews_db::PreviewIndex` (+ fields, `pyramid`, `levels`, `best_level`, `deletion`, `candidates`, `row_for_size`, `payload_path`) | seam | the index a front-end builds and hands to the plan |
| `previews_db::PyramidKey` (+ `new`, `of`, `uuid`, `digest`), `Candidate` (+ `path`, `row`, `long_edge`) | seam | the key and the candidate a seam lookup is expressed in |
| `previews_db::Layout` (+ `as_str`), `INDEX_FILE`, `CONTAINER_GRAMMAR`, `LEVEL_FILE_GRAMMAR` | seam | the layout word reaches `scan --json`, and the two templates are the only form of a payload path that may leave the machine (`adr-0007-publish-a-verification-report`) |
| `previews_db::sweep`, `CacheSweep`, `PyramidOnDisk`, `LevelOnDisk` | not supported | one walk, owed to the plan and called by `Plan::build`; a second caller would be a second answer. The CLI's and the core's tests are the outside callers |
| `plan::Plan` (+ fields, `build`, `selected`, `selected_images`), `PlanItem` (+ fields, `long_edge`), `Options`, `Stats`, `CacheFacts`, `Shapes`, `Status` (+ `as_str`), `Payload` (+ `path`), `Tier` (+ `as_str`) | seam | the plan is the hinge, and a front-end both builds it and reads it |
| `plan::Tier::from_long_edge` | convenience | the mapping behind a tier a front-end only ever reads |
| `extract::extract`, `ExtractOptions`, `Outcome`, `OutcomeStatus` (+ `as_str`), `outcome_summary`, `write_outcomes_csv`, `below_floor` | seam | the writer and what it reports; `extract` is the only module that writes user-visible output |
| `extract::ensure_safe_destination_pending`, `ensure_safe_destination_create`, `ensure_create_outside` | seam | the refusal that must happen before the first write, on the destination and on `.lrcat-data` |
| `extract::ensure_safe_destination` | convenience | its own doc points at `ensure_safe_destination_create`, which supersedes it |
| `extract::DESTINATION_NOT_OURS` | not supported | the note text a refusal writes; no caller outside the module |
| `report::summary`, `item_lines`, `write_csv`, `write_json`, `write_verification`, `write_contact_sheet`, `tiers_json`, `shapes_json` | seam | the surfaces a second front-end prints instead of inventing: the summary, the CSV, the manifest, the verification report and the sheet. The summary takes the run's `Options`, so the bounds it names are the ones the caller applied |
| `report::escape_csv` | not supported | public so `extract`'s per-file CSV quotes by the same rules, as `report`'s own spec says |
| `masks::Mask` (+ fields), `decode_dat`, `to_png`, `list` | seam | the third command's whole surface: decode a layer, encode it, find the layers |
| `model::CatalogImage` (+ fields, `output_stem`, `root_display_name`, `relative_output_path`, `relative_output_path_with_suffix`), `CacheEntry`, `Pyramid`, `PyramidLevel`, `XmpMetadata`, `Orientation` (+ `from_catalog_code`, `from_exif`, `exif_value`, `degrees`, `is_upright`) | seam | the data every other seam type is made of |
| `model::sanitize_component` | not supported | the security boundary `relative_output_path` already applies; the properties test is its only outside caller |
| `error::Error` (+ variants), `error::Result` | seam | one error type, as the invariants below require, and a front-end matches its variants to decide what to tell a user |
| `error::IoContext` | convenience | attaches a path when a caller composes its own `io::Error`; no seam call needs it |
| `jpeg::certify`, `Certified` (+ fields), `Segment` (+ `marker`, `range`, `payload`), `dimensions`, `is_complete`, `is_payload_start` | not supported | the certification the plan already performed; a front-end reads `PlanItem`'s width, `payload_metadata` and `effective_orientation` instead. `lrsalvage-fixtures` and the tests are the outside callers |
| `lrprev::Lrprev` (+ `parse`, `header`, `levels`, `best_level`), `Chunk` (+ `name`, `data`), `chunk_data`, `level_index`, `MAGIC` | not supported | the container walk, done once by the plan; a front-end reads the `Payload` an item names |
| `exif::PayloadMetadata` (+ `is_empty`) | seam | it is a `PlanItem` field, and its three flags are what a report says about a payload |
| `exif::MetadataInput` (+ fields, `with_marker`, `is_empty`), `Injection`, `inject`, `inject_full`, `inject_icc`, `from_catalog`, `from_catalog_with_profile`, `inventory`, `orientation`, `software`, `icc_profile`, `marker`, `SOFTWARE_PREFIX` | not supported | the writer's own parts: `extract` composes them, and the `Software` identity they spell is promised as bytes on disk (`adr-0010-what-0-2-promises`, its item 8), not as a Rust helper |
| `header::PyramidHeader` (+ fields), `parse` | not supported | no production path calls it: `previews.db` is the authority for sizes, profile and mapping, so the module is documented and unused |
| `pub mod` × 14, and the twelve-name `pub use` block in `lib.rs` | — | a module carries the seam items listed above; thirteen do and `header` does not. The re-export block is not a second home |

Two functions are already `pub(crate)` (`snapshot::with_suffix` and
`snapshot::open_readonly`) and so make no claim at all; they are named here only
so the inventory can be checked against a `grep 'pub '` and come out empty.

### What may change without a decision

- **A not-supported item: anything**, including narrowing it to `pub(crate)` or
  deleting it. A front-end that reached into `jpeg`, `lrprev`, `exif` or
  `header` was relying on a module's internals, and `cargo doc` never said
  otherwise.
- **A convenience item: added, renamed or dropped** in any release, so long as
  the seam call it composes keeps the behaviour the seam gives it.
- **A seam item: `adr-0010-what-0-2-promises`'s two tiers govern it.** An
  additive item may land in a minor; removal, renaming or a change of meaning
  needs a new minor with the deprecation stated in the preceding one. A patch
  changes nothing in the seam, and hiding a module that carries a seam item is
  the same event as removing the items in it.

### The gate that does not exist yet

The table above is review-held: nothing compares it to the code. A snapshot — a
checked-in `cargo public-api` listing, or the same list kept by hand — would turn
"every `pub` item is placed" into a gate instead of a reader's grep. **It is a
follow-up slice, not part of 0.2** (`adr-0010-what-0-2-promises` declines a new
policy file in release week), and until it lands the seam has the same standing
as that ADR's exit-code table and `extract.csv`'s columns: a record a reviewer
holds, not a check a machine holds.

## Module dependency edges

Edges the code actually has (non-test). A module may depend only on the ones
listed for it:

| Module | Depends on |
|---|---|
| `snapshot` | `error` |
| `catalog` | `snapshot`, `model`, `error` |
| `previews_db` | `snapshot`, `model`, `error` |
| `lrprev` | `jpeg`, `error` |
| `jpeg` | `error` |
| `header` | `error` |
| `model` | — (no internal dependencies, no I/O) |
| `plan` | `catalog`, `previews_db`, `lrprev`, `jpeg`, `exif`, `model`, `error` |
| `extract` | `plan`, `exif`, `jpeg`, `lrprev`, `model`, `report`, `error` |
| `exif` | `model`, `jpeg`, `error` |
| `report` | `plan`, `lrprev`, `jpeg`, `model`, `previews-db`, `error` |
| `masks` | `error` |
| `error` | — |
| `cli` | any of the above, as a consumer |

Two edges are easy to miss and are real: `lrprev` calls `jpeg` to measure each
declared payload, and `extract` calls `report::escape_csv` so the run CSV is
escaped by the same rules as `report.csv`. `exif` calls `jpeg` for the one
segment walk that both a payload's certification and its metadata reading use;
`plan` calls `exif` for the inventory and the payload's declared orientation, and
`extract` calls it to write the resume marker and to read it back — which is why
both edges appear. `previews_db` borrows `snapshot`'s
read-only opener for the cache's own database, which is why it appears in the
table. `masks` is gated behind the `masks` cargo feature (`png` is optional);
everything else is always compiled. `report` reads its two name templates from
`previews_db` rather than restating a naming rule in a presentation module.

## Invariants the code enforces

- **Ownership of side effects.** `extract` is the only module that writes
  user-visible output. `report` writes only to a path its caller named, and
  `scan` writes only the artifacts the caller named — an explicitly requested
  contact sheet and an explicitly requested verification report — each validated
  and claimed, and never one it invented. The verification report is path-free by
  construction: no module hands it a path, and no field it carries could hold one
  (`adr-0007-publish-a-verification-report`). `snapshot` is the only module that
  copies a catalog file, and it copies into its own temporary directory.
- **Format knowledge stays in the module named after the format.** Nothing else
  parses bytes: `lrprev` owns the `AgHg` container, `previews_db` owns the index
  database, `catalog` owns the `.lrcat` schema, `jpeg` owns marker framing,
  `exif` owns metadata segments, `masks` owns `.lrcat-data`. `plan` composes
  their answers and never learns a byte offset.
- **Decide, then write.** `plan` decides status, selection, tier and output path
  and writes nothing; `extract` writes and decides nothing about which image
  deserves to be recovered. A scan and an extract for the same inputs therefore
  agree by construction.
- **A module's prose states only what it observed or performed.** An item note or
  a report line names the facts the module itself holds and the act the module
  itself takes. `plan` decides and never writes, so it can say a payload carries
  its own EXIF and what writing metadata does with it, while replacing that
  segment is `extract`'s to report. A note that claims another module's act
  becomes a lie the moment a flag — `--no-metadata`, say — changes what that
  module does, and the note is what a stranger reads instead of the cache.
- **One error type.** Every fallible core function returns `error::Error`, with
  the path context that produced it. Exit codes are the CLI's mapping of that
  type, never a status smuggled through the core.
- **Nothing is clobbered on faith.** An existing destination is replaced only
  when it verifies as one of our own outputs; a report file name is claimed, not
  assumed.
- **The artifact carries a suite it can run.** A published tarball compiles its
  test targets and passes its own `cargo test` where it is unpacked, with no
  checkout in reach; `scripts/check.sh package` is what proves it for both
  crates. So a test that needs `lrsalvage-fixtures` lives in `tests/`, never in a
  `#[cfg(test)]` module inside `src/`: the fixtures crate is never published
  (`adr-0011-0-3-is-the-trust-minor`, D6), the published manifest drops the
  dev-dependency, and only `tests/` can be dropped from the package with it.
- **User data is opened read-only.** The `.lrcat`, the preview cache and
  `.lrcat-data` are evidence. A destination inside any of those trees is refused
  before the first write, and `--verify-source` makes the claim checkable: the
  catalog is hashed, copied and hashed again, and a pair that disagrees fails the
  run with its own exit code rather than a warning.

## Where the reasoning lives

The load-bearing decisions are recorded as adopted ADRs and are not restated
here: `adr-0001-parse-dont-carve`, `adr-0002-map-via-previews-db`,
`adr-0003-preserve-preview-bytes`, `adr-0004-detect-layout-not-version`,
`adr-0005-discover-payloads-by-index`, `adr-0006-degrade-without-guessing`,
`adr-0007-publish-a-verification-report`,
`adr-0008-a-render-companion-is-not-an-orphan`,
`adr-0009-rescue-an-image-from-its-companion`,
`adr-0010-what-0-2-promises` (which declares the compatibility contract every
other decision is a promise about),
`adr-0011-0-3-is-the-trust-minor` (what the release after 0.2 is for, and the
six issues it refuses), `adr-0012-two-release-lines` (which artifacts carry a
version number, what `0.4` is for, and the criteria that decide whether the app is
a browser page or an executable), and `adr-0013-a-cap-demotes-the-level` (a bound
chooses the level; it never empties an image).
Byte-level facts about the formats, with an evidence label per claim, live in
`format-notes`; the product vocabulary lives in `glossary`. A module spec links
to those and carries only what is specific to its own boundary.
