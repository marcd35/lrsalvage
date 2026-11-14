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
- **`imageId` is not assumed unique.** No table in the schema declares a primary
  key, so `all_entries` keeps every row. `entries` is the last row wins map used
  for lookup; the full list exists so that a payload belonging to a duplicate row
  is not reported as an orphan payload.
- **`best_level` ranks by pixel area, with the level number as tie-break.**
  Neither the highest level number nor the longest dimension reliably identifies
  the 1616 px preview: real caches store levels in an arbitrary order.
- **Only pyramids, levels and cache entries are read.** `MotionPreview` (a poster
  frame, not a clip), `FacePreview`, and the `Deleted*` tombstones are not
  consulted; stale rows surface later as `missing-payload` instead.
- **A missing `previews.db` is the caller's problem to report.** Opening a cache
  without one yields an empty index, which is exactly what turns every image into
  `no-cache-entry` and every payload into an orphan — the honest answer per
  `adr-0002-map-via-previews-db`.
