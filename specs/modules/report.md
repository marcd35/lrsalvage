---
id: report
type: module-design
title: "report: the terminal summary, the CSV, the manifest, the contact sheet"
status: draft
parent: architecture
depends-on:
  - plan
  - cache-recovery
  - lrprev
  - jpeg
  - model
  - previews-db
  - snapshot
  - fs
references:
  - adr-0005-discover-payloads-by-index
  - adr-0006-degrade-without-guessing
  - adr-0007-publish-a-verification-report
  - adr-0008-a-render-companion-is-not-an-orphan
  - adr-0013-a-cap-demotes-the-level
---

# report

## Responsibility

Say what a plan means, for a human and for a script: the one-block summary, one
line per plan item, `report.csv`, `manifest.json`, a self-contained HTML contact
sheet, and the path-free verification report `scan --report` writes.

## Boundary

Reads a plan and, for the contact sheet, the payload files it needs — both
through `fs`, and `report.csv` as one whole-file write rather than a stream.
Writes to the path its caller named and invents no path of its own; the
CLI and browser each apply the numbered-name rule in their destination writer. It decides nothing and adds
no metadata. The digest pair a verified run proved is a value handed to it, not a
fact it measures: no module here opens the catalog.
The manifest encoder accepts the host timestamp so the browser and the CLI
can generate the same schema through their own destination writers. `csv` and
`verification_json` are the same encoders `write_csv` and `write_verification`
write: a host that commits whole files itself has no path to hand this module,
and a second renderer for a published file would be a second dialect of it.

The verification report is the one artifact here that leaves the machine, so it is
built from a struct of its own — the curated fields below — rather than from a
filtered plan or a filtered `--json` object. Nothing in this module ever hands that
struct a path.

A cache-only run plans `CacheRecovery` rather than `Plan`, so it gets its own
encoders for the same three files: `cache_csv`, `cache_manifest_json` and
`cache_verification_json`. They take the batch's counts as `CacheBatch` — `saved`,
`already_recovered`, `failed`, `confirmed_on_disk` — because outcomes are
`extract`'s and `extract` renders its CSV through this module, so the counts cross
as values rather than as types that would make a cycle. The member sets below are
frozen per mode and differ, because a cache has no catalog to measure; what they
share is the schema tag and a `mode` member that says which one a file is.

A run that read no index at all (#198) plans `LegacyRecovery` and gets the third
set: `legacy_csv`, `legacy_manifest_json` and `legacy_verification_json`, with
`mode` `legacy` and the four container counts `containers`, `usable`, `invalid`
and `unsupported`. `extract::legacy_outcomes_csv` marks the batch file the same
way, so three files from one run agree about which run they came from.

## Decisions

- **One encoder per published file, whatever writes it.** `report.csv` and the
  verification report are published formats, so their text and their file are the
  same bytes: `csv` and `verification_json` build them, and `write_csv` and
  `write_verification` write exactly what those return. The browser commits whole
  files through the page's writer, which has no path to give this module, and the
  verification report's `generated_unix` comes from the host for the same reason
  the manifest's does — a `wasm32` page has no clock here. The alternative, a
  second renderer per artifact, is two grammars for one file
  (`specs/app.md`, *Receipt, reports and diagnostics*).
- **Every published report says which mode wrote it, and each mode's member set is
  frozen here.** `mode` is `catalog`, `cache-only` or `legacy`; it is the one
  member all three modes share besides `schema`, `version` and `generated_unix`. A
  cache-only run measured no catalog, so it publishes none of the catalog
  objects' facts rather than a row of nulls a reader has to filter:
  - `manifest.json` (cache-only): `schema` `lrsalvage.manifest`, `version`,
    `generated_unix`, `mode` `cache-only`, `layout`, `scan`, `outputs` (one entry
    per prepared output, as the catalog manifest's `items` are), `batch`
    (`saved`, `already_recovered`, `failed`).
  - `verification.json` (cache-only): `schema` `lrsalvage.verification`,
    `version`, `generated_unix`, `mode` `cache-only`, `layout`, `scan`,
    `writes` (`saved`, `already_recovered`, `failed`, `confirmed_on_disk`; the
    last is the receipt's reconciliation, `null` when the run never made one).
    It carries no item, filename, folder or payload digest: the aggregate
    counts and the layout word are the whole of it.
  - `scan` is the five counts that mode measured — `indexed`, `recoverable`,
    `missing`, `invalid`, `unsupported` — in `report.md`'s order. They are not
    the catalog `stats` under other words: an indexed pyramid with no payload is
    a different fact from an image whose payload is missing, and one vocabulary
    for both would be a lie about whichever it was not measuring.
  - `manifest.json` (legacy): the same members **less `layout`** — `schema`,
    `version`, `generated_unix`, `mode` `legacy`, `scan`, `outputs`, `batch` —
    and `verification.json` (legacy) likewise, with `writes` as above. An
    indexless run classifies no representation: it reads every container the walk
    finds, so the cache's `layout` word would be a fact it never measured.
  - `scan` (legacy) is the four counts that mode measured — `containers`,
    `usable`, `invalid`, `unsupported`. A container is a file, not an indexed
    pyramid, so the indexed words would name different facts; `containers` and
    `usable` are this mode's own.
- **A cache-only `report.csv` has its own frozen columns.** `mode,filename,status,
  width,height,fallback,dimension_mismatch`: one row per indexed pyramid, the
  generated filename rather than an original, and an empty width and height when
  no preview survived. The catalog header's folder, tier, capture-time and
  original-path columns describe a catalog this run does not have, so reusing the
  header would publish sixteen empty cells and call them facts.
- **A legacy `report.csv` carries the container, and `extract.csv` carries the
  mode.** `mode,filename,status,width,height,container`: one row per `.lrprev`
  found, the generated filename, and the cache-relative path that name is derived
  from — the one identity this mode has, and the only way to tell two containers
  holding identical previews apart. A cache-relative path is a folder tree of
  uuids rather than a person's folder names, and this file is local to the run, so
  the shareable artifacts stay the ones that name nothing: the verification
  report has no filename, path or container.
- **The batch's counts arrive as a value, not as a type this module would have to
  know.** `CacheBatch` carries `saved`, `already_recovered`, `failed` and
  `confirmed_on_disk`; `extract` owns the outcomes and renders its own CSV through
  `escape_csv`, so handing this module `Outcome` would close a dependency cycle.
  The three counts in the manifest and the four in the verification report are
  the same value, so the two files cannot disagree about one batch.
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
- **`thumbnail_pyramids` is the one cache count that stops at `--json`.**
  `thumbnail pyramids  10   a render's second pyramid; no image claims it` is
  printed only when non-zero and mirrored as `thumbnail_pyramids` in `--json`; it
  is deliberately absent from `manifest.json` and from the verification report's
  frozen `stats` list, because `shapes.level_file` already counts a companion's
  files and a stranger's report gains nothing from the cache's age. It is never a
  `report.csv` column and never an item: a render's second pyramid is a fact
  about the cache, not a lost photograph
  (`adr-0008-a-render-companion-is-not-an-orphan`).
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
  `level_file`, `suffix_matches_sof`, `container` — and `shapes` and the tier
  histogram are each built by one shared helper: they are the parts a single
  function can own because they are the parts shaped identically on every
  machine surface. The nested-per-class form with a grammar template in it is the
  verification report's and belongs there alone. An absent version string is
  `null`, never `""` and never a stand-in.
- **The summary counts images and names the bounds actually applied.** The caller
  hands the run's options to the summary, so one function prints the floor and the
  cap the run really used; a summary that reported a `below_floor` or `above_cap`
  count for a bound nobody applied would be a lie about the run, and a default
  wrapper around a hard-coded 640 is exactly that lie
  (`adr-0013-a-cap-demotes-the-level`).
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
  `lrsalvage.verification`, the tool `version` that wrote it, `generated_unix`,
  the `mode` that wrote it;
  then `layout`, `db_version` and `crs_version` (ADR 0004's five words and two
  verbatim strings), `source_sha256` and `snapshot_sha256` (the digest pair a
  `--verify-source` run proved, `null` for both when it did not), `options`
  (`min_long_edge`, `max_long_edge`, `all_levels`, `include_present`,
  `verify_source`), `stats`, and `shapes`. `options` echoes the knobs the run
  applied, which the caller hands over with the plan: a plan records what the run
  decided, not the options that shaped it. `stats` carries exactly `images`,
  `recovered`, `suspect`, `missing_payload`, `no_cache_entry`, `unmapped`,
  `failed`, `original_present`, `below_floor`, `above_cap`, `bytes`, `levels`,
  `selected_items`, `selected_images`, `tiers`, `superseded`, `deleted` and
  `unrecognised`. It is not `--json` with fields removed: there is no `catalog`,
  no `previews`, no `items` and no note, so a reader cannot mistake it for a
  summary of one machine. The list is frozen here, which is what makes two
  strangers' reports comparable and what turns a new stat into a decision rather
  than a field that appears in a published file.
- **Nothing in the report identifies a cache, a folder or a payload.** Its only
  names are grammar templates with `<uuid…>`, `<digest>` and `<longedge>`
  placeholders. Two things enforce that instead of intending it: the writer's
  struct has no path, leaf, uuid or payload-digest field to fill, and the fixture
  matrix asserts the written bytes contain none of a vault's absolute paths,
  uuids, digests, shard folder names or leaf names
  (`adr-0007-publish-a-verification-report`).
- **The two digests are the catalog's, never a payload's, and hex is what keeps
  them path-free.** `source_sha256` and `snapshot_sha256` arrive as the snapshot's
  own `SourceDigests` value: this module prints a pair another module measured and
  never derives one from a path. They are the one deliberate exception to
  "nothing here identifies a payload", and they identify the reverse — no cache,
  no uuid, no leaf, and no filename field is derived from them. Both are `null`
  when the flag was not given, so the object is the same eleven members on every
  report and two strangers' files stay comparable; a reader reads `null` as "not
  measured", never as zero. `options.verify_source` is the pair's presence
  rather than a second value, because the flag is the only thing that measures,
  and an echo that could disagree with the evidence beside it is a bug waiting
  to be written.
- **`shapes` counts the class each payload took, and carries that class's
  grammar.** `level_file` is `{"count", "suffix_matches_sof", "template"}`:
  its `count`, how many of those leaves name the long edge the `SOF` measured —
  the one claim derived from the index that can rot — and its grammar.
  `container` is `{"count", "template"}`, because a container's own size is
  nothing the cache publishes. Each class's `template` is printed verbatim from
  the constants `previews_db` owns — `LEVEL_FILE_GRAMMAR` and
  `CONTAINER_GRAMMAR` — so a mixed cache publishes both grammars, which one
  cache-wide string could only get wrong. A class with no file reports
  `count: 0` and **no** `template` key, because a template for a shape the
  cache never showed is an example nobody asked for. `unrecognised` has no class
  here on purpose: it counts files that took no shape, and it already has a
  `stats` field (`adr-0005-discover-payloads-by-index`).
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
