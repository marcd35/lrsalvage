---
id: plan
type: module-design
title: "plan: decisions without writes"
status: draft
parent: architecture
depends-on:
  - catalog
  - previews-db
  - lrprev
  - jpeg
  - model
references:
  - adr-0001-parse-dont-carve
  - adr-0002-map-via-previews-db
  - glossary
---

# plan

## Responsibility

Turn a catalog, a preview index, the cache root and three options into an explicit
list of decisions: one status for every catalog image, the items `extract` would
write, tier classes, totals, and the payloads no live image claims.

## Boundary

Composes what `catalog`, `previews_db`, `lrprev` and `jpeg` report; learns no byte
offset and decides nothing about bytes. It reads payload files but writes
nothing — not one byte, not one directory. Everything that ends up on disk is
`extract`'s work, which is what makes `scan` a true rehearsal of `extract`.

## Decisions

- **One status per catalog image, and the statuses are a partition.** Every image
  is exactly one of `recovered`, `suspect`, `original-present`, `no-cache-entry`,
  `missing-payload`, `failed`; an unclaimed payload is listed as `unmapped`
  alongside them. A payload that is both claimed and broken is `failed`, not two
  rows.
- **Statistics count images; items count levels.** `--all-levels` emits one item
  per level but must not inflate how many photographs were recovered.
- **A suspect payload is recovered, not discarded.** Width and height may be
  swapped before calling it a disagreement, because the stored render is
  unrotated; a real photograph at the wrong declared size is still worth having,
  and every report flags it.
- **The largest level is chosen by decoded dimensions, not byte count.** A small
  file with a large header is still a small image. Ties keep the higher level.
- **The floor excludes, it does not fail.** Items below `--min-long-edge` (640 by
  default, `0` disables) are counted as `below_floor` and left unselected.
- **`original-present` short-circuits everything else**, and the `stat` behind it
  is skipped when `--include-present` is set, so the opt-in run costs nothing per
  image.
- **The orphan sweep is seeded from live rows only.** A row whose `imageId` no
  longer exists in `Adobe_images` must not suppress the sweep: that stale row's
  payload is exactly what salvage is for (`adr-0002-map-via-previews-db`).
- **A cache payload is never trusted at its declared size, and never discarded
  for disagreeing** (`adr-0001-parse-dont-carve`): a chunk that fails to parse,
  holds no readable level, or does not end at an end-of-image marker makes the
  image `failed`.
- **Name collisions are resolved, not overwritten.** Two images resolving to one
  output path get ` (2)`, ` (3)` … with case folding on Windows, where `IMG.jpg`
  and `img.jpg` are one file.
- **The output path is re-validated here.** Even though `model` sanitises every
  component, the join to the output root refuses anything that is not ordinary
  relative components: `starts_with` alone would accept `out/../escaped`.
