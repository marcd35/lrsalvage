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
  - adr-0007-publish-a-verification-report
  - adr-0008-a-render-companion-is-not-an-orphan
  - adr-0009-rescue-an-image-from-its-companion
  - adr-0013-a-cap-demotes-the-level
  - glossary
---

# plan

## Responsibility

Turn a catalog, a preview index, the cache root and four options into an explicit
list of decisions: one status for every catalog image, the items `extract` would
write, tier classes, totals, the payloads no live image claims, and one block of
cache-level facts — the layout, the catalog's two version strings and the shape
the cache's files took.

## Boundary

Composes what `catalog`, `previews_db`, `lrprev`, `jpeg` and `exif` report; learns
no byte offset and decides nothing about bytes. It reads payload files but writes
nothing — not one byte, not one directory. Everything that ends up on disk is
`extract`'s work, which is what makes `scan` a true rehearsal of `extract`.

A plan records what the run decided, not the knobs that shaped it: the `Options`
the caller passed stay the caller's, so a surface that echoes them takes them
from the run rather than from the plan.

How a level is ranked, and what declares its size: `adr-0005-discover-payloads-by-index`
¶3–¶5 and `format-notes` §5.2. Read them before changing `level_file_levels`,
`container_levels`, or the cap's winner rule — a level file's `_<n>` leaf can
stand in for a row it lacks, a container's `level_N` chunk cannot, and which one
applies decides whether a disagreement is `suspect`.

## Decisions

- **One status per catalog image, and the statuses are a partition.** Every image
  is exactly one of `recovered`, `suspect`, `original-present`, `no-cache-entry`,
  `missing-payload`, `failed`; an unclaimed payload is listed as `unmapped`
  alongside them. A payload that is both claimed and broken is `failed`, not two
  rows.
- **A payload is one level, and the item names where it lives.** The item carries
  `Payload::Container { path, level }` — a legacy `.lrprev` and the `level_N`
  chunk that was certified — or `Payload::LevelFile { path }`, where the file *is*
  the level. `level` is `None` whenever no level was certified — a file that is
  gone, one that failed to parse, or a pyramid no image claims — and such an item
  is never written. The kind is decided here, from the index and the
  files; neither `extract` nor `report` sniffs a payload to find out what it is,
  and neither ever re-picks a level: the item's choice is the choice.
- **`missing-payload` means no pyramid of that image holds a file at all.** A row
  whose level file is gone while another level survives is a **pruned level**: the
  item recovers the largest level present and its note says the row is stale.
  14.0+ prunes in the background, so a partial pyramid is ordinary, not a fault
  (`adr-0005-discover-payloads-by-index`). When the claimed pyramid has no file
  anywhere, the plan falls back to the image's render companion before it calls
  the payload gone (`adr-0009-rescue-an-image-from-its-companion`). Only when
  neither holds a file is the image `missing-payload`.
- **A stale row is a note, never a status or a claimed cause.** A pruned level and
  a level file the user deleted are indistinguishable — the row is the only claim
  and the file is absent either way — so the item reports what was seen. A
  `DeletedPyramid` tombstone adds one quotable fact (Lightroom recorded deleting
  *this pyramid's row*), never an inference about who removed the file. It is also
  the `deleted` stat: one per `missing-payload` item whose pyramid carries a
  tombstone, reported beside `missing_payload`, never a status
  (`adr-0006-degrade-without-guessing`).
- **An unclaimed payload is listed, never selected.** It has no catalog image, so
  there is no original path, no capture time, no rating, and no output path to
  plan; `extract` writes nothing for it. Listing is the whole of its fate, which
  is why the report gives its payload path a field rather than burying it in a
  note. Unclaimed payloads are counted **per pyramid**, never per level file: one
  orphan is one lost photograph, whatever its level set looks like. An orphan
  item carries the `quality` and `color_profile` of the row its pair reaches, so
  the report can say which kind of pyramid nobody claims; with no row it prints
  neither.
- **A render's second pyramid is a cache fact, not an orphan.** A fresh render
  writes a `thumbnail` pyramid beside the one an entry claims (§5.10), and no
  entry ever names it. Such a pyramid is counted as
  `CacheFacts::thumbnail_pyramids` and is never an item, so `unmapped` keeps
  meaning "nobody names this" instead of restating the cache's age
  (`adr-0008-a-render-companion-is-not-an-orphan`). Three clauses must all hold:
  a **claimed pyramid exists on the same uuid** in the live claim set, the row's
  `quality` is **exactly `thumbnail`**, and **every level it holds measures inside
  the `thumbnail` tier** (< 640 px, a fixed constant rather than
  `--min-long-edge`). So a uuid nothing maps keeps every pyramid it holds
  `unmapped`, a `.lrprev`-only pyramid is never a companion, a row that lies
  about its size cannot hide a payload, and an unreadable level keeps its pyramid
  visible. The count never moves an exit code and is never a status.
- **A companion the plan needs becomes the image's payload.** When the pyramid the
  entry names has no file anywhere, the plan reads the classified companion's
  largest level — ranked by its own rows, judged against its own row, reporting
  its own `quality` and `color_profile` — and the item's note says where the
  payload came from. A rescued 320 px level is `recovered`, `below_floor` and
  unselected, exactly like a claimed level of that size (M12), and
  `--min-long-edge 0` writes it under the image's own name
  (`adr-0009-rescue-an-image-from-its-companion`). The cap reaches this path too,
  because a companion's levels are ranked by the same level-file rule: one rule
  for a level, whichever pyramid held it. It binds there only under a floor
  lowered below the thumbnail tier, because a classified companion's every level
  measures under 640 px — so with the default floor no cap reaches one, and with
  `--min-long-edge 0 --max-long-edge 200` a 320 px companion level is `above_cap`
  and unselected, exactly as a claimed level of that size would be. The cache
  count is unchanged: `thumbnail_pyramids` describes the cache, not what the plan
  did with one pyramid.
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
  gap, and nothing is written for the payloads. The plan needs no mode literal
  for this: with no rows the claim set is empty, so every pyramid the sweep finds
  becomes an orphan item, and the layout word comes from that same sweep rather
  than from rows that are not there.
- **Statistics count images; items count levels.** `--all-levels` emits one item
  per level but must not inflate how many photographs were recovered. Every item
  is judged by the same per-level rule — its own bytes against its own row — and
  the image's one status is its chosen item's, so a good 808 beside a truncated
  1616 is `recovered` and `failed` as two items while the image stays `failed`.
- **An unrecognised file is counted, never planned.** A payload-like file that
  matched no shape has no catalog image, no pyramid and no level, so it becomes
  no item at all: `unrecognised` is a cache count beside `superseded`, and it is
  why an `unknown` layout is not an empty one. Wrapping it in an item would give
  it an output path it has no name for (`adr-0005-discover-payloads-by-index` ¶6).
- **The plan counts the shapes the cache's payloads took, as cache counts.** The
  same sweep that finds orphans records `level_file`, how many of those leaves name
  the long edge their `SOF` measured (`suffix_matches_sof`), and `container` — three
  numbers about files, reported beside `superseded` and never a status, an item
  field or an exit code. None of them needs a row: a leaf the glob found and a leaf
  a row named are counted the same way, because the comparison is the measured
  `SOF` against the name rather than a second matcher. A file with no readable
  `SOF` does not match, and an incomplete one still does: a truncated frame that
  declares its size has named its long edge. What they are for is the
  verification report a stranger can hand over
  (`adr-0007-publish-a-verification-report`). That one measured pass is also what
  ranks an unclaimed pyramid, so the histogram and the orphan list come from the
  same bytes, and the cost is the same one: every level file is read whole,
  because a bounded prefix could cut an `APPn` segment in half and turn a frame
  that is really there into a mismatch.
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
  it writes is compared — against the pyramid's best row while that level is the
  container's largest, and otherwise against the row that declares its own size,
  which is what a cap can select (`adr-0013-a-cap-demotes-the-level`).
- **The chosen level is the largest one present that clears the cap, ranked by
  the claim before the bytes.** A container is measured level by level and
  sorted by decoded area: a small file with a large header is still a small
  image, and a tie keeps the higher level. A pyramid of level files is ranked by
  each candidate's own row area, and by `_<n>` only where a file has no row, then
  the winner alone is certified (`adr-0005-discover-payloads-by-index` ¶5).
  Either way the dimensions the item reports are measured from the winner's own
  `SOF`, never from the name.
- **The floor and the cap bound which level is written; neither fails an image.**
  Items below `--min-long-edge` (640 by default, `0` disables) are counted as
  `below_floor`, items above `--max-long-edge` (default `0`, which disables it)
  are counted as `above_cap`, and both are left unselected. A level judged `failed`
  is still an item the run evaluates: it is reported with its reason and writes
  nothing, so a cache with a broken payload exits partial instead of claiming a
  clean success (`goal-and-requirements` ¶5). A cap is a winner rule rather than
  a filter after one: the largest level at or below it owns the image's plain
  name, so a 1:1 cache writes its 1920 level instead of writing nothing. An image
  whose every level is above the cap keeps its largest level as an item —
  `recovered`, unselected, counted — exactly as an image below the floor does,
  and never becomes `failed` (`adr-0013-a-cap-demotes-the-level`).
  `--all-levels` makes every level above the cap an item too; without it the cap
  is invisible unless it binds, which is the floor's behaviour as well. Neither
  bound reaches `superseded`: that counts pyramids held twice, not levels.
- **`original-present` short-circuits everything else**, and the `stat` behind it
  is skipped when `--include-present` is set, so the opt-in run costs nothing per
  image.
- **Every cache-level fact is gathered once, into one struct.** `Plan::cache` is
a `CacheFacts` of `layout`, `db_version`, `crs_version` and `shapes`, filled by
the single `previews_db::sweep` plus `catalog::versions`. One enumeration feeds
the orphan list, the `superseded` count and the `shapes` histogram, so no two
surfaces can print different numbers for the same cache and no caller re-derives
one. The layout word and the version strings are observations, never gates
(`adr-0004-detect-layout-not-version`).
- **The orphan sweep is seeded from live rows only, and counts pyramids.** A row
  whose `imageId` no longer exists in `Adobe_images` must not suppress the sweep:
  that stale row's payload is exactly what salvage is for
  (`adr-0002-map-via-previews-db`). The enumeration is
  `previews_db::sweep`: one entry per `(uuid, digest)` whose leaf
  parses and whose two shard folders agree with it, merging its level files and
  any coexisting `.lrprev`. An unclaimed entry that is not a render companion
  becomes exactly one `unmapped` item — never one per file, because one orphan
  is one lost photograph — whose
  payload is `Payload::LevelFile` of the largest level it holds or
  `Payload::Container { level: None }` when it holds only a container. **Largest
  by the `SOF` its own bytes declare**, with the leaf's `_<n>` only as a
  tie-break and as the rank for a file whose frame cannot be read at all: the
  same rule in every mode, index or none, because a name is its author's word and
  the bytes are the verdict (`adr-0005-discover-payloads-by-index` ¶5, ¶9).
  Nothing is selected or written for it, and the note names its path. A
  **claimed** entry holding both representations is not an item at all: it is the
  `superseded` cache stat.
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
