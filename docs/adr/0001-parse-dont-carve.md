---
id: adr-0001-parse-dont-carve
type: architecture-design
title: "ADR 0001: Parse, don't carve"
status: active
parent: architecture
---

# Parse, don't carve

**Status:** accepted · **Date:** 2026-11-12

## Context

Every level inside a `.lrprev` is a complete JPEG, and a level may carry its own
marker segments, including an embedded EXIF thumbnail; many levels carry none at
all (see `docs/format-notes.md` §1.4). A tool that extracts levels by scanning for
`FF D8 … FF D9` therefore has two ways to be wrong: it can find an embedded
thumbnail instead of the level it wanted, and it can run past a truncated level
into the next chunk's bytes. Both produce a valid JPEG of the wrong size. That
failure is silent — the file opens, looks like a photograph, and is 160 px wide.
Public carving tools do exactly this, which is why one of them is cited in the
notes as a bug rather than a reference.

The container declares where each payload begins and ends; `previews.db` records
the dimensions Lightroom expected; the payload's own SOF marker states what the
bytes actually are. They answer two different questions: is the payload complete,
and is it the image the cache claimed?

## Decision

Walk the chunks by their declared lengths. Read the SOF dimensions from the
declared payload and require the payload to be a complete JPEG within its
declared bounds. Treat any length overrun, header smaller than 24 bytes, missing
SOI, or missing EOI at the declared end as a failure of that file.

A disagreement between the dimensions recorded in `previews.db` and the decoded
SOF dimensions is not a failure. It makes the item `suspect`; the item is still
extracted, and it is flagged in every report. Width and height may be swapped
before calling it a disagreement, because the stored render is unrotated and the
cache may have recorded the upright shape.

The final chunk may omit its padding. Tolerate a shortfall only when the payload
ends exactly at EOF; a payload that runs past EOF is still an error.

We never search for `FF D8` or `FF D9`.

## Consequences

- A truncated or mis-framed file fails loudly with status `failed` instead of
  yielding a plausible thumbnail. That is the point: someone's last copy of a
  photo should not be a 160 px file labelled as the 1440 px preview.
- A dimension disagreement costs nothing but a flag. A real photo at the wrong
  declared size is still worth recovering, and refusing it would turn a metadata
  problem into a second data loss.
- The final-padding tolerance accepts the shape Lightroom writes when it omits
  the last pad — a complete payload that ends at EOF — without accepting a file
  that is genuinely cut short.
- We give up a fallback that could have produced *something* from a damaged
  file. If damaged files turn out to be common, the answer is a separate,
  explicitly labelled recovery mode, not a quiet carve inside the normal path.
- We own a small SOF parser. That is cheaper than trusting a full decoder, and
  it keeps the dimension check independent of pixel decoding.
- Comparing declared and decoded dimensions is what makes the `suspect` status
  possible at all.
- Corrupt files produce no output, so the report has to carry the explanation;
  a bare "0 recovered" is not acceptable.

## Considered options

- **Carve, then pick the largest result.** Still cannot distinguish a large
  level from two adjacent chunks, and still silently returns thumbnails when the
  large level is truncated.
- **Decode every payload fully with a JPEG library.** Correct dimensions, much
  slower, and it hides container corruption behind decoder leniency.
