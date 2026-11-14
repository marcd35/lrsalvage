---
id: previews-db
type: module-design
title: "previews_db: the authoritative image-to-payload map"
status: draft
parent: architecture
depends-on:
  - snapshot
  - model
references:
  - adr-0002-map-via-previews-db
  - adr-0005-discover-payloads-by-index
  - adr-0006-degrade-without-guessing
  - format-notes
---

# previews_db

## Responsibility

Load the cache's `previews.db` into an in-memory index — `imageId → CacheEntry`,
`uuid → Pyramid`, `uuid → [PyramidLevel]` — and resolve the payload file a row
names and the best level of a pyramid.

## Boundary

Knows the index schema and the cache's directory layout; knows nothing about
catalog rows beyond the `imageId` it is given, never reads a payload byte, and
never writes. The database is opened through `snapshot`'s read-only opener. Which
rows are live, which are stale, and what to do about either is `plan`'s decision.

## Decisions

- **The path rule is derived, never globbed**: `<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>.lrprev`
  under the cache root. The exact case of the uuid and digest matters on a
  case-sensitive filesystem, so both are used verbatim.
- **A level file's path is derived the same way, from its row.**
  `<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>_<round(longDimension)>` — the rounded
  integer, because the columns come back `REAL` (`101.0`). Enumerating a
  pyramid's levels is what `candidates` is for, and it hands each candidate the
  row that derived it: `Some(&PyramidLevel)` for a row-derived path, `None` for a
  file only the bounded glob found. The row travels with the file because it is
  the claim `plan` compares `suspect` against; that is the one place it can be
  attached without a second matcher (`adr-0005-discover-payloads-by-index` ¶2–3).
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
  deletion times for one purpose: letting a `missing-payload` note quote
  Lightroom's own record of the deletion. A tombstone never subtracts from a
  count, never suppresses an orphan and never changes a status
  (`adr-0005-discover-payloads-by-index`, `adr-0006-degrade-without-guessing`).
- **A missing or unreadable `previews.db` yields an empty index and a reason, not
  an error.** Absence is a mode — the caller reports every image as
  `no-cache-entry` and every payload pyramid as `unmapped` per
  `adr-0002-map-via-previews-db` — so this module returns `PreviewIndex::default()`
  plus the reason (missing, or the SQLite error text) for the caller's hint. The
  running code still fails here; that changes with the indexless mode
  (`adr-0006-degrade-without-guessing`).
- **"Derived, never globbed" is the v1 rule, not the final one.**
  `adr-0005-discover-payloads-by-index` keeps the identity here (`(uuid, digest)`
  is this module's key) but widens the *level set* by a glob bounded to a row's
  own identity and folder, and moves per-file certification into `jpeg`. The
  enumeration is `candidates(cache_root, uuid, digest)` returning each candidate
  file with its row; `payload_path` keeps resolving one row's container. This
  spec describes the decided shape next to the running v1 code, and the code
  catches up with the reader.
