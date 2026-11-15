---
id: report
type: module-design
title: "report: the terminal summary, the CSV, the manifest, the contact sheet"
status: draft
parent: architecture
depends-on:
  - plan
  - lrprev
  - jpeg
  - model
  - previews-db
references:
  - adr-0005-discover-payloads-by-index
  - adr-0006-degrade-without-guessing
  - adr-0007-publish-a-verification-report
---

# report

## Responsibility

Say what a plan means, for a human and for a script: the one-block summary, one
line per plan item, `report.csv`, `manifest.json`, a self-contained HTML contact
sheet, and the path-free verification report `scan --report` writes.

## Boundary

Reads a plan and, for the contact sheet, the payload files it needs. Writes to the
path its caller named and invents no path of its own; the numbered-name rule for
report files belongs to `cli`. It decides nothing and adds no metadata.

The verification report is the one artifact here that leaves the machine, so it is
built from a struct of its own — the curated fields below — rather than from a
filtered plan or a filtered `--json` object. Nothing in this module ever hands that
struct a path.

## Decisions

- **A degraded run says what it saw, in one hint.** Every line that talks about a
  payload the run could not name is a fact about this run — counts filled from the
  plan, never hard-coded — and no line calls an unclaimed payload recovered,
  recoverable or salvage. The hint names the missing or unreadable file, says that
  unmapped payloads are never written, and points at the surface that lists their
  paths (`adr-0006-degrade-without-guessing`).
- **An unclaimed payload's path is a field, not prose.** `manifest.json` carries a
  `payload` member for every item, `report.csv` carries `payload` as a trailing
  column after `note`, and the one-line-per-item view shows it where an image's
  relative output path would otherwise print `-`. A script triaging orphans must
  not have to parse an English sentence out of a note. The value is
  `Payload::path()` — a level file, or the container a payload lives in — so the
  report never needs to know which kind it is. `payload_metadata` follows
  `payload` as the last column, so the two trailing facts from
  `adr-0006-degrade-without-guessing` and `adr-0003-preserve-preview-bytes` never
  move an existing column.
- **`missing-payload` names the missing thing, not a suspect.** Its note says which
  pyramid was declared and that no file of it is on disk, and quotes a
  `DeletedPyramid` timestamp when Lightroom recorded one. A stale level row inside
  a surviving pyramid says which level is stale and which level was used instead.
  Neither claims a cause.
- **`superseded` is one summary line, only when it is non-zero.** `superseded  3
  legacy .lrprev ignored; level files used`, mirrored as `superseded` in
  `--json`, `manifest.json` and the verification report's `stats`, and never in
  `report.csv`, which stays per-image. It is a
  property of the cache, not of an item: a superseded container always coexists
  with the level files its pyramid was written from, so the number reports what
  was set aside, never a loss.
- **`deleted` and `unrecognised` are cache counts on the same surface as
  `superseded`.** `deleted` counts the `missing-payload` items whose pyramid
  carries a `DeletedPyramid` row — Lightroom's own record, never a cause — and
  `unrecognised` counts the payload-like files no shape matched, whatever the
  layout turned out to be. Each is a summary line only when non-zero, a field in
  `--json`, `manifest.json` and the verification report's `stats`, and never a
  column in `report.csv`: neither belongs to an image, and neither moves an exit
  code (`adr-0004-detect-layout-not-version`, `adr-0006-degrade-without-guessing`).
- **The summary opens with one line naming the layout, and nothing else.**
  `cache layout: modern` is the whole of it: the layout is the one fact that
  decides how everything below it was read, and it is a word, not a count. The
  two version strings are deliberately absent from the text summary — they are
  for a script and for a contributed verification report, and a number a reader
  might mistake for a compatibility statement belongs in neither (`adr-0004`).
- **The machine surfaces carry the cache's shape flat, and each names where.**
  `--json` is one flat object, so the three cache counts are members of it;
  `manifest.json` groups its image counts under `stats`, so they join them there.
  That nesting is the contract, not an accident of who wrote which file. Both
  carry `layout`, `db_version`, `crs_version` and `shapes` as three numbers —
  `level_file`, `suffix_matches_sof`, `container` — and `shapes` is built by one
  shared helper, the only part of the pair a single function can own because it
  is the only part shaped identically. The nested-per-class form with a grammar
  template in it is the verification report's and belongs there alone. An absent
  version string is `null`, never `""` and never a stand-in.
- **The summary counts images and names the floor actually applied.** The default
  summary assumes 640, so a caller that changed the floor calls
  `summary_with_floor`; a summary that reports a `below_floor` count for a floor
  nobody applied would be a lie about the run.
- **Machine output keeps a fixed shape.** Fields are separated by two spaces and
  an unknown fact prints as `-`, so the five columns stay put for a script;
  `--json` replaces the table with one object rather than mixing the two.
- **CSV escaping lives here once.** `escape_csv` is public so the run's per-file
  CSV is escaped by the same rules instead of growing a second dialect.
- **The contact sheet is recognition, not resolution.** Each image is shown with
  the *smallest* level in its pyramid, and the page is one self-contained HTML
  file with data URIs — a sheet that needs a folder of sidecar files would be a
  worse bug than a small picture. A container payload is walked for its smallest
  level, as today; a level-file payload is inlined from the item's
  `smallest_level`, the smallest file the plan's own level enumeration found,
  after a completeness check — inlining the item's payload instead would put the
  largest level in every figure and multiply the page by two orders of magnitude.
- **A missing or unparseable payload makes the sheet skip the image**, not fail
  the run; the sheet is a convenience over a plan that has already been made.
- **The manifest carries a schema version** so a consumer can tell which facts a
  file is expected to contain.
- **The verification report is a curated field set, frozen here.**
  `scan --report <file>` writes one JSON object: `schema`
  `lrsalvage.verification`, the tool `version` that wrote it, `generated_unix`;
  then `layout`, `db_version` and `crs_version` (ADR 0004's five words and two
  verbatim strings), `options` (`min_long_edge`, `all_levels`,
  `include_present`), `stats`, and `shapes`. `stats` carries exactly `images`,
  `recovered`, `suspect`, `missing_payload`, `no_cache_entry`, `unmapped`,
  `failed`, `original_present`, `below_floor`, `bytes`, `levels`,
  `selected_items`, `selected_images`, `tiers`, `superseded`, `deleted` and
  `unrecognised`. It is not `--json` with fields removed: there is no `catalog`,
  no `previews`, no `items` and no note, so a reader cannot mistake it for a
  summary of one machine. The list is frozen here, which is what makes two
  strangers' reports comparable and what turns a new stat into a decision rather
  than a field that appears in a published file.
- **Nothing in the report identifies a cache, a folder or a payload.** Its only
  names are grammar templates with `<uuid…>`, `<digest>` and `<longedge>`
  placeholders. Two things enforce that instead of intending it: the writer's
  struct has no path, leaf, uuid or digest field to fill, and the fixture matrix
  asserts the written bytes contain none of a vault's absolute paths, uuids,
  digests, shard folder names or leaf names (`adr-0007-publish-a-verification-report`).
- **`shapes` counts the class each payload took, and carries that class's
  grammar.** `level_file` holds its `count` and `suffix_matches_sof` — how many of
  those leaves name the long edge the `SOF` measured, the one claim derived from
  the index that can rot — and `container` holds its `count`. Each class also
  carries the template it stands for, printed verbatim from the constants
  `previews_db` owns: a mixed cache therefore publishes both grammars, which one
  cache-wide string could only get wrong. A class with no file reports `count: 0`
  and no grammar, because a template for a shape the cache never showed is an
  example nobody asked for. `unrecognised` has no class here on purpose: it counts
  files that took no shape, and it already has a `stats` field
  (`adr-0005-discover-payloads-by-index`).
- **`--json` stays the stdout surface; the report is the only path-free one.** A
  redaction switch would leave two ways to produce one object and a flag whose
  whole job is to be remembered, so there is none: `--json` gains only what
  ADR 0004 and ADR 0006 already promise it, keeps its paths, and the two surfaces
  never mix — `--verbose` lines already go to stderr under `--json`, and the
  report goes to a path the caller named.
- **A run that recovered nothing still writes the report it was asked for.** The
  file describes a plan, and the plans most worth handing over are the degraded
  ones: an `unknown` layout, a missing index, an empty cache. When the write
  happens is `cli`'s decision; what it says is this module's.
- **`payload_metadata` is one token list, in a fixed order.** `exif,xmp,icc` for
  the segments the payload itself carried, empty when it is bare: a field on the
  item, an optional manifest member, and a trailing CSV column. `color_profile`
  stays the catalog's claim, so a reconstruction is never implied where the file
  carries the payload's own profile; the prose reasons live in the item's note,
  which is for humans and is never the field a fixture asserts against.
