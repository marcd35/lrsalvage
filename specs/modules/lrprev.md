---
id: lrprev
type: module-design
title: "lrprev: the AgHg preview container"
status: draft
parent: architecture
depends-on:
  - jpeg
references:
  - adr-0001-parse-dont-carve
  - format-notes
---

# lrprev

## Responsibility

Parse one `.lrprev` into its chunks by their declared lengths — `header`, then
`level_1 … level_N` — and hand back each payload slice together with its decoded
SOF dimensions, so the caller can choose a level without touching pixels.

## Boundary

Owns the `AgHg` grammar and nothing above it: no statuses, no comparison against
`previews.db` (the caller decides `suspect`), no catalog or database knowledge,
no writes. The `header` chunk is not parsed here; its parser lives in `header`
and has no production consumer.

## Decisions

- **Declared lengths are the whole grammar; marker scanning is never used**
  (`adr-0001-parse-dont-carve`). A payload is a complete JPEG with its own segment
  stream, so a `FF D8 … FF D9` scan can return the level's embedded EXIF
  thumbnail, or run into the next chunk, and both yield a valid JPEG of the wrong
  size.
- **A `header_len` below 24 is corruption, not a variant.** The length includes
  the magic, so 24 is the shortest legal header (empty name).
- **Truncation fails the file; it is not resynchronised.** A header past EOF, or
  a payload that does not fit in the remaining bytes, is an error carrying the
  offset — resynchronising is how a carve starts.
- **The final chunk may omit its padding.** A shortfall is tolerated only when
  that chunk's payload ends exactly at EOF; no chunk other than the last may be
  short, and a payload past EOF is truncation.
- **Measurement is per level.** Each level's dimensions come from `jpeg`, and a
  level whose dimensions cannot be read is dropped from selection — it is not a
  reason to fail a pyramid that has a good level.

## Limits

The `.lrprev` pyramid is the only storage read. Lightroom 13.3 and later write
per-image folders of extensionless JPEGs instead; that format is unimplemented,
so such a cache reports `missing-payload` for every image and the run exits 3.
That is a refusal to guess, not a partial success: a wrong mapping here would be
worse than no mapping (`adr-0002-map-via-previews-db`).
