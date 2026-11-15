---
id: previews-db
type: module-design
title: "previews_db: the authoritative image-to-payload map"
status: draft
parent: architecture
depends-on:
  - snapshot
  - model
  - jpeg
references:
  - adr-0002-map-via-previews-db
  - adr-0005-discover-payloads-by-index
  - adr-0006-degrade-without-guessing
  - adr-0007-publish-a-verification-report
  - format-notes
---

# previews_db

## Responsibility

Load the cache's `previews.db` into an in-memory index — `imageId → CacheEntry`,
`uuid → Pyramid`, `uuid → [PyramidLevel]`, plus the `DeletedPyramid` tombstones —
derive the paths its rows name: the container file of a pyramid, each level file
of one, and the best level of a pyramid; and enumerate the cache tree once into
the pyramids present on disk, the payload-like files that are no pyramid, and
therefore the **layout** the cache holds.

## Boundary

Knows the index schema and the cache's directory layout; knows nothing about
catalog rows beyond the `imageId` it is given, and never writes. It reads
directory entries and names, and opens a payload only through `jpeg`'s two-byte
sniff — never more than its first two bytes, and never to interpret them. The
database is opened through `snapshot`'s read-only opener. Which rows are live,
which are stale, and what to do about either is `plan`'s decision.

## Decisions

- **A payload's address is derived, never globbed**: `<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>.lrprev`
  under the cache root. The exact case of the uuid and digest matters on a
  case-sensitive filesystem, so both are used verbatim. Only a pyramid's *level
  set* is widened by a directory read, and only inside that pyramid's own
  address (`adr-0005-discover-payloads-by-index` ¶3).
- **A level file's path is derived the same way, from its row.**
  `<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>_<round(longDimension)>` — the rounded
  integer, because the columns come back `REAL` (`101.0`). Enumerating a
  pyramid's levels is what `candidates` is for, and it hands each candidate the
  row that derived it — `Some(&PyramidLevel)` for a row-derived path, `None` for
  a file only the bounded read of that pyramid's folder found — together with the
  long edge its leaf names, which is what ranks a file that has no row. The row
  travels with the file because it is the claim `plan` compares `suspect`
  against; that is the one place it can be attached without a second matcher
  (`adr-0005-discover-payloads-by-index` ¶2–3).
- **`imageId` is not assumed unique.** No table in the schema declares a primary
  key, so `all_entries` keeps every row. `entries` is the last row wins map used
  for lookup; the full list exists so that a payload belonging to a duplicate row
  is not reported as an orphan payload.
- **`best_level` ranks by pixel area, with the level number as tie-break.**
  Neither the highest level number nor the longest dimension reliably identifies
  the 1616 px preview: real caches store levels in an arbitrary order.
- **Only pyramids, levels and cache entries are read; tombstones only ever
  narrate.** `MotionPreview` (a poster frame, not a clip) and `FacePreview` are
  not consulted. `DeletedPyramid` rows are read into a set of `(uuid, digest)` and
  deletion times for one purpose: letting a `missing-payload` item be counted as
  `deleted` and its note quote Lightroom's own record of the deletion. A tombstone
  never subtracts from a count, never suppresses an orphan and never changes a
  status (`adr-0005-discover-payloads-by-index`,
  `adr-0006-degrade-without-guessing`). A store without the table reports no
  tombstones rather than refusing the cache: losing a narration is not a reason
  to stop reading payloads that are all there, which is the same tolerance
  `catalog` gives a table it can do without.
- **`layout` is one of five words, and this module owns the vocabulary.**
  `Layout` is `legacy` (`.lrprev` containers only), `modern` (loose payload files
  only), `mixed` (both kinds, anywhere in the cache), `none` (an index and no
  payload files) or `unknown` (payload-like files that matched no shape).
  `CacheSweep::layout()` decides it from the tree alone. Nothing here reads a
  version, and nothing anywhere compares one to a layout: the catalog format and
  the preview layout version independently
  (`adr-0004-detect-layout-not-version`). `mixed` describes the cache, so
  per-pyramid supersession is `plan`'s count and never a value of this enum.
- **A missing or unreadable `previews.db` yields an empty index and a reason, not
  an error.** Absence is a mode — the caller reports every image as
  `no-cache-entry` and every payload pyramid as `unmapped` per
  `adr-0002-map-via-previews-db` — so `open` returns an `Opened`: the index
  (`PreviewIndex::default()` when there was nothing to read) plus an
  `IndexProblem` — `Missing`, or `Unreadable` with SQLite's own error text — for
  the caller's one hint. The rows are read by a private `PreviewIndex::read`, so
  the only public way in degrades and no caller can mistake "no index" for a
  failure. A `previews.db` that exists and a directory by that name both take the
  `Unreadable` arm; only an absent path is `Missing`
  (`adr-0006-degrade-without-guessing` ¶1–¶2).
- **The two name templates live here, beside the rule they describe.**
  `CONTAINER_GRAMMAR` is `<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>.lrprev`, and
  `LEVEL_FILE_GRAMMAR` is that path with `_<longedge>` in place of the extension.
  Both hold placeholders only, never a name, and they exist so the verification
  report can publish the shape of a cache without publishing one of its files:
  `report` prints them verbatim and paraphrases neither. A unit test here
  substitutes a known `(uuid, digest)` and asserts each template agrees with the
  path the derivation returns, so a template cannot drift from the rule it stands
  for (`adr-0007-publish-a-verification-report`).
- **The level set is enumerated here, and it widens inside the row's identity.**
  `adr-0005-discover-payloads-by-index` keeps the identity here (`(uuid, digest)`
  is this module's key) and moves per-file certification into `jpeg`, which owns
  the one walk a payload is believed by. `candidates(cache_root, uuid, digest)`
  returns one leaf per `PyramidLevel` row, carrying the row that derived it, and
  then reads **that pyramid's own** shard folder — `<cache>/<uuid[0]>/<uuid[0..4]>`
  and nothing else — accepting only a file whose name is exactly
  `<UUID>-<digest>_<digits>` for this uuid **and** this digest, with `row: None`.
  A differing digest is not a member (¶6), so the widening finds a level the
  index forgot — a pruned `PyramidLevel` row beside a surviving file — and cannot
  invent a mapping. A path a row already named appears once and keeps its row,
  the order is by path, only names are read (never a payload byte), and a missing
  folder is a pyramid with nothing on disk rather than an error. `payload_path`
  keeps resolving one row's container.
- **The cache tree is enumerated once, and a pyramid is one `(uuid, digest)`.**
  `sweep(cache_root)` walks the cache and returns a `CacheSweep`: one entry per
  `(uuid, digest)` present on disk, grouping that pyramid's level files and any
  coexisting `<UUID>-<digest>.lrprev`, **and** the payload-like files that are no
  pyramid at all. A file is a member only when its leaf parses to a uuid, a
  32-character lower-case hex digest and — for a level file — a numeric suffix,
  **and** both shard folders agree with that uuid, because the folder is an
  address and never an identity (§5.2). `previews.db` and its `-wal`/`-shm`
  neighbours, a leaf in the wrong shard, and a leaf that does not parse are all
  non-members, so each is offered `jpeg`'s two-byte sniff and nothing more: one
  that opens `FF D8` comes back as **unrecognised** — reported, never ignored,
  and never an orphan (`adr-0005-discover-payloads-by-index` ¶6). No member's
  byte is read here, so this module never ranks a pyramid's levels: which level
  of an unclaimed pyramid is its payload is `plan`'s decision, taken from each
  file's own `SOF` and never from its name
  (`adr-0005-discover-payloads-by-index` ¶8–¶9). Which of these pyramids an
  image claims, whether a claimed one is `superseded`, and what an unclaimed one
  becomes, is `plan`'s decision too (`adr-0005-discover-payloads-by-index` ¶8,
  `adr-0006-degrade-without-guessing`).
