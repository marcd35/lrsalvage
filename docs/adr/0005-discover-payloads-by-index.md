---
id: adr-0005-discover-payloads-by-index
type: architecture-design
title: "ADR 0005: Discover payloads by index, widen by name, certify by bytes"
status: active
parent: architecture
references:
  - format-notes
  - adr-0001-parse-dont-carve
  - adr-0002-map-via-previews-db
  - adr-0004-detect-layout-not-version
  - adr-0006-degrade-without-guessing
---

# Discover payloads by index, widen by name, certify by bytes

**Status:** accepted · **Date:** 2026-11-14

## Context

Lightroom Classic 13.3 replaced one `.lrprev` container per image with one file per
level (`docs/format-notes.md` §5). The container answered three questions at once —
where the payload is, how big it is, whether it is complete — and the new layout
answers them in three separate places:

- **the path**: `<cache>/<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>_<longedge>` — the
  legacy sharding, the leaf without `.lrprev`, plus `_<longedge>` (§5.2);
- **the index**: `previews.db` still carries `Pyramid`, `PyramidLevel` (with
  `longDimension`) and `ImageCacheEntry`, so both the level set and the image
  mapping are declared;
- **the bytes**: each file is a complete bare JPEG. All 390 payloads of the
  measured cache have `SOI` at offset 0, `FF D9` exactly at EOF, and no APP1, ICC
  or XMP segment anywhere (§5.7).

Two earlier decisions were written for the container and do not transfer
literally. ADR 0001 protected against choosing an embedded EXIF thumbnail by
walking declared chunk lengths; a modern payload has no chunks. ADR 0002 forbids
hashing, globbing and nearest-neighbour fallback when mapping image → payload, and
the level set is not that mapping.

Issue #2's decision 1 proposed the shortcut both ADRs would have caught: any file
whose parent folder name is a `previews.db` uuid and whose first two bytes are
`FF D8`, largest by SOF dimensions wins. The parent folder is `uuid[0..4]`, not
the uuid, so on the measured cache that rule finds nothing at all (§5.2); and
"largest by SOF wins" is the heuristic ADR 0001 rejected.

## Decision

**A pyramid is identified by `(uuid, digest)`, exactly as in the legacy layout; a
level is identified within it by its long edge; a payload is certified by its own
bytes.**

1. **Identity comes from the row.** `(uuid, digest)` from `Pyramid` /
   `ImageCacheEntry` — the key legacy already uses. The shard folders `<uuid[0]>`
   and `<uuid[0..4]>` are an address, never an identity: among 3,416 pyramids
   about 89 pairs are expected to share a four-character shard.
2. **The index names the candidates.** For each `PyramidLevel` row, derive

   ```
   <cache>/<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>_<round(longDimension)>
   ```

   `longDimension`, `level`, `width` and `height` are stored **REAL** (`101.0`),
   so the suffix is the rounded integer — formatting the float gives `_101.0`
   and matches nothing.
3. **The name widens the level set, inside the row's own identity.** For each
   pyramid also read *that pyramid's own* shard folder and accept files matching
   `<UUID>-<digest>_<digits>`. The glob is bounded by the row's uuid **and**
   digest in the row's own folder, so it can discover a level the index forgot (a
   pruned `PyramidLevel` row beside a surviving file) and cannot invent a mapping.
   A file whose digest differs from the row's is not a member of that pyramid.
4. **The bytes certify each file.** `SOI` at 0; walk the **top-level** marker
   segments by their declared `u16` lengths, skipping APPn and COM payloads
   wholesale; the first top-level `SOF` gives the dimensions; require `FF D9` at
   EOF. Nothing is ever scanned for `FF D8`: an embedded EXIF thumbnail lives
   inside APP1's declared payload, so it is never on the chain and can never be a
   candidate. This is ADR 0001's guarantee, restored for a container-less layout.
5. **The row is the claim, the bytes are the verdict.** Rank a pyramid's levels as
   today — by the row's pixel area, and by `_<n>` only when no row exists — then
   certify the winner. A dimension disagreement makes the item `suspect` and it is
   extracted anyway (ADR 0001). One file per image: the largest level that
   exists. `missing-payload` means *no* level file of that pyramid exists, never
   "the advertised top level is absent": 14.0+ prunes in the background, so a
   partial pyramid is normal (§5.5).
6. **Disagreement has three outcomes, kept apart.** A file that parses but whose
   name carries a different digest from its row is an orphan — only the pair maps
   (ADR 0002). A file that is payload-like but not the right shape — wrong shard,
   no numeric suffix, no parseable uuid — is *unrecognised*: ADR 0004's narrow
   `FF D8` sniff, feeding the `unknown` layout, not the orphan count. A file at a
   derived path that fails to parse is `failed`, loudly.
7. **Coexistence resolves per pyramid, not per cache.** A pyramid with both
   representations uses its modern levels; a `.lrprev` for a pyramid that is
   claimed counts as `superseded` (issue #2, decision 2).
   **Amended 2026-11-15 by ADR 0006**, which fixes the count and its name: the
   stat counts **claimed pyramids with both representations** — at least one level
   file *and* at least one legacy `.lrprev` — never whichever payload is larger. A
   pyramid whose only representation is a `.lrprev` is legacy and is read from the
   container, not superseded; one with neither file is `missing-payload`; an
   unclaimed `.lrprev` is `unmapped`. The stat never moves the exit code, because a
   superseded container always coexists with the level files its pyramid is written
   from.
8. **The orphan sweep counts pyramids.** One `unmapped` item per unclaimed
   `(uuid, digest)`, merging that pyramid's modern levels and any coexisting
   `.lrprev`, reporting the largest level's file as the payload. A
   `DeletedPyramid` tombstone does not subtract: a tombstone is not a mapping, and
   its surviving payload is exactly what salvage is for. Motion and face previews
   are counted when they have the payload shape — their modern shape is
   unverified (§5.7 TODO), and subtracting them would commit to an assumption
   instead of measuring one.
9. **Without an index, identity comes from the name.** With no rows, parse
   `(uuid, digest)` from the leaf, require both shard folders to agree with it,
   rank by measured `SOF` area (a legacy `.lrprev` by its declared dimensions),
   report every pyramid as `unmapped`, exit 3, one hint (issue #2, decision 3).
   This is the only mode with no row behind discovery, and it is why it can list
   payloads but never claim a filename.

**ADR 0002 is amended in scope, not in substance.** It governs the image → payload
mapping, which remains the row's alone. It never governed which of a pyramid's own
level files to enumerate, and a glob bounded by the row's own `(uuid, digest)` in
the row's own folder cannot produce a false mapping. Its rejection of "match by
UUID prefix only" is upheld: a differing digest is not a member.

## Consequences

- Legacy and modern share one identity and one level model, so `plan`, `extract`
  and `report` learn "a pyramid has levels, each a file" once instead of twice.
- An upgraded cache that lost level rows still recovers: the file is found by the
  scoped glob and certified by its bytes.
- The reader opens a header's worth of each chosen level instead of trusting the
  index, which is what makes `suspect`, `failed` and truncation visible in a
  layout with no container framing.
- Deleted-but-present payloads are reported as `unmapped` rather than hidden by
  their tombstone, so someone who deleted a photo can still find its preview.
- A modern cache reports honest orphan counts. The sweep costs one `read_dir` per
  pyramid, which is cheaper than the per-image `stat` it replaces on a large cache.
- Reaching into a pyramid's shard folder is a second, bounded filesystem pass,
  allowed only for a row's own identity. Widening it — to `<UUID>-*`, to a folder,
  to the whole tree — is a new decision, not a tweak.
- Rows naming a digest the files no longer carry produce orphans rather than
  guesses, the same stance §5.6 takes on stale rows: say what was seen, do not
  claim corruption.
- `unknown` stays narrow. Payload-like-but-unrecognised files are counted and
  shown, so a new Lightroom layout can be reported before it can be read.
- `specs/modules/previews-db.md` states today's rule ("derived, never globbed").
  It changes when the reader lands, not before.

## Considered options

- **Index only, no widening.** Simplest, and faithful to a literal reading of ADR
  0002, but it loses every level whose `PyramidLevel` row was pruned while the file
  survived — an ordinary state once 14.0+ prunes in the background.
- **Content sniffing the whole tree.** Every `FF D8` file, grouped by folder,
  largest wins. It finds nothing the scoped glob misses, cannot map an image
  without the index (ADR 0002), counts foreign JPEGs as payloads, and its
  folder-as-uuid half is measured false.
- **Group by shard folder.** One directory per image is the layout's appearance,
  not its guarantee: about 89 four-character shard pairs collide at 3,416 images.
- **Trust the name over the row** (`_<n>` ranks the level). A renamed or
  mis-rendered file then wins the pyramid, and the user gets a 404 px file named as
  the 1616 px one — ADR 0001's silent failure in a new place.
- **All-or-nothing on the advertised best level.** Reports `missing-payload` while
  a smaller level of the same image sits on disk: a lost photo stays lost for a
  bookkeeping reason.
- **Extract every level as its own output file.** Multiple files per image changes
  output paths, resume verification, `superseded` and the contact sheet; it is a
  separate feature, not this rule.
- **Full decode instead of a marker walk.** Correct dimensions, slower, and it
  hides framing errors behind decoder leniency — already rejected in ADR 0001.
