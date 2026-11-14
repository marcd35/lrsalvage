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
  - exif
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

Composes what `catalog`, `previews_db`, `lrprev`, `jpeg` and `exif` report; learns
no byte offset and decides nothing about bytes. It reads payload files but writes
nothing — not one byte, not one directory. Everything that ends up on disk is
`extract`'s work, which is what makes `scan` a true rehearsal of `extract`.

## Decisions

- **One status per catalog image, and the statuses are a partition.** Every image
  is exactly one of `recovered`, `suspect`, `original-present`, `no-cache-entry`,
  `missing-payload`, `failed`; an unclaimed payload is listed as `unmapped`
  alongside them. A payload that is both claimed and broken is `failed`, not two
  rows.
- **A payload is one level, and the item names where it lives.** The item carries
  `Payload::Container { path, level }` — a legacy `.lrprev` and the `level_N`
  chunk that was certified — or `Payload::LevelFile { path }`, where the file *is*
  the level. `level` is `None` only for an unclaimed pyramid, whose payload is
  listed and never written. The kind is decided here, from the index and the
  files; neither `extract` nor `report` sniffs a payload to find out what it is,
  and neither ever re-picks a level: the item's choice is the choice.
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
  per level but must not inflate how many photographs were recovered. Every item
  is judged by the same per-level rule — its own bytes against its own row — and
  the image's one status is its chosen item's, so a good 808 beside a truncated
  1616 is `recovered` and `failed` as two items while the image stays `failed`.
- **A suspect payload is recovered, not discarded.** Width and height may be
  swapped before calling it a disagreement, because the stored render is
  unrotated; a real photograph at the wrong declared size is still worth having,
  and every report flags it.
- **The declared size is the chosen level's own row, and `suspect` is decided per
  level.** `previews_db` hands each level-file candidate the `PyramidLevel` row
  that derived its name, so `PlanItem::declared` is that row and no second
  matcher exists. A file the scoped glob found has no row: there is nothing to
  disagree with, so it cannot be `suspect`, and its size is its own `SOF`. This
  is what makes a stale row a note rather than a status — the surviving smaller
  level is compared against *its* row, not the pyramid's largest
  (`adr-0005-discover-payloads-by-index`). A container keeps its existing rule:
  its whole level set is in the file or the file is truncated, so only the level
  it writes is compared.
- **The chosen level is the largest one present, ranked by the claim before the
  bytes.** A container is measured level by level and sorted by decoded area: a
  small file with a large header is still a small image, and a tie keeps the
  higher level. A pyramid of level files is ranked by each candidate's own row
  area, and by `_<n>` only where a file has no row, then the winner alone is
  certified (`adr-0005-discover-payloads-by-index` ¶5). Either way the dimensions
  the item reports are measured from the winner's own `SOF`, never from the
  name.
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
  for disagreeing** (`adr-0001-parse-dont-carve`): a chunk or level file that
  fails to parse, or does not end at an end-of-image marker, makes the image
  `failed`. There is no quiet fallback to a smaller level when the *chosen* one
  is broken: a truncated 1616 file beside a good 808 is reported, not papered
  over — only a file that is *absent* falls back, and that is a pruned level.
- **Name collisions are resolved, not overwritten.** Two images resolving to one
  output path get ` (2)`, ` (3)` … with case folding on Windows, where `IMG.jpg`
  and `img.jpg` are one file.
- **The plan reads payload bytes once, and keeps facts rather than bytes.** The
  chosen level is read whole and certified through `jpeg`, and the item holds its
  dimensions, its segment inventory and its declared orientation — never the
  bytes. `extract` reads the same file again to write it, which is the only read
  whose bytes leave the machine.
- **The payload's own segment inventory travels with the item.** `exif`, `xmp`,
  `icc` — which of those the chosen payload already carries, empty when it is
  bare — recorded by the certification walk, so the field costs no second read:
  `jpeg::certify` returns the top-level segment list and `exif::inventory` names
  the segments from it. It is what lets a report say a file's profile came from
  the payload rather than from the catalog's name, and it never becomes a status:
  `suspect` stays reserved for bytes that disagree with the row's dimensions.
- **The payload's own orientation outranks the catalog's, and both facts stay on
  the item.** `orientation` keeps the catalog's claim; `effective_orientation` is
  what the payload declares, else the catalog's, and that is the value `extract`
  writes and `report` prints. The precedence is computed here, once, and a
  disagreement is a note, never a status. Pixels are never rotated, so the tag
  cannot be applied twice.
- **A level-file item names the pyramid's smallest level for the contact sheet.**
  `smallest_level` is the smallest of the candidates the same widening already
  enumerated, so the sheet stays recognition-sized instead of inlining the
  payload's hundreds of kilobytes. A container item has no such field: the sheet
  walks its levels itself.
- **The output path is re-validated here.** Even though `model` sanitises every
  component, the join to the output root refuses anything that is not ordinary
  relative components: `starts_with` alone would accept `out/../escaped`.
