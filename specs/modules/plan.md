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
  - adr-0005-discover-payloads-by-index
  - adr-0006-degrade-without-guessing
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
- **`missing-payload` means no file of that pyramid exists at all.** A row whose
  level file is gone while another level survives is a **pruned level**: the item
  recovers the largest level present and its note says the row is stale. 14.0+
  prunes in the background, so a partial pyramid is ordinary, not a fault
  (`adr-0005-discover-payloads-by-index`). Only a pyramid with no file anywhere is
  `missing-payload`.
- **A stale row is a note, never a status or a claimed cause.** A pruned level and
  a level file the user deleted are indistinguishable — the row is the only claim
  and the file is absent either way — so the item reports what was seen. A
  `DeletedPyramid` tombstone adds one quotable fact (Lightroom recorded deleting
  *this pyramid's row*), never an inference about who removed the file
  (`adr-0006-degrade-without-guessing`).
- **An unclaimed payload is listed, never selected.** It has no catalog image, so
  there is no original path, no capture time, no rating, and no output path to
  plan; `extract` writes nothing for it. Listing is the whole of its fate, which
  is why the report gives its payload path a field rather than burying it in a
  note. Unclaimed payloads are counted **per pyramid**, never per level file: one
  orphan is one lost photograph, whatever its level set looks like.
- **`superseded` is a cache stat, not a status, and it cannot move the exit code.**
  It counts claimed pyramids with **both** representations — at least one level
  file and at least one legacy `.lrprev` — whose level files are the pyramid and
  whose container is never read. A `.lrprev`-only pyramid is legacy and is read
  from the container; one with neither file is `missing-payload`; an unclaimed
  `.lrprev` is `unmapped`. Size decides nothing: the model decides
  (`adr-0005-discover-payloads-by-index`, amended by
  `adr-0006-degrade-without-guessing`). Because a superseded container always
  coexists with the level files its pyramid is written from, it can never be the
  only item selected.
- **An absent or unreadable index yields an empty index, not an error.** Every
  image is then `no-cache-entry` and every payload pyramid `unmapped` — the
  honest answer per `adr-0002-map-via-previews-db` — and the run exits 3 with one
  hint (`adr-0006-degrade-without-guessing`). No mapping is invented to fill the
  gap, and nothing is written for the payloads.
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
- **The orphan sweep is seeded from live rows only, and counts pyramids.** A row
  whose `imageId` no longer exists in `Adobe_images` must not suppress the sweep:
  that stale row's payload is exactly what salvage is for
  (`adr-0002-map-via-previews-db`). One item per unclaimed `(uuid, digest)`,
  merging its level files and any coexisting `.lrprev` and naming the largest
  level as the payload.
- **A cache payload is never trusted at its declared size, and never discarded
  for disagreeing** (`adr-0001-parse-dont-carve`): a chunk that fails to parse,
  holds no readable level, or does not end at an end-of-image marker makes the
  image `failed`.
- **Name collisions are resolved, not overwritten.** Two images resolving to one
  output path get ` (2)`, ` (3)` … with case folding on Windows, where `IMG.jpg`
  and `img.jpg` are one file.
- **The payload's own segment inventory travels with the item.** `exif`, `xmp`,
  `icc` — which of those the chosen payload already carries, empty when it is
  bare — recorded by the certification walk discovery already performs, so the
  field costs no second read. It is what lets a report say a file's profile came
  from the payload rather than from the catalog's name, and it never becomes a
  status: `suspect` stays reserved for bytes that disagree with the row's
  dimensions.
- **The payload's own orientation outranks the catalog's.** The item carries the
  catalog's orientation and the report shows the value actually written, which is
  the payload's when its EXIF declares one. Pixels are never rotated, so the tag
  cannot be applied twice.
- **The output path is re-validated here.** Even though `model` sanitises every
  component, the join to the output root refuses anything that is not ordinary
  relative components: `starts_with` alone would accept `out/../escaped`.
