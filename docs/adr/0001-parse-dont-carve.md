# Parse, don't carve

**Status:** accepted · **Date:** 2026-11-12

## Context

Every level inside a `.lrprev` is a complete JPEG, and every one of those JPEGs
carries its own EXIF thumbnail. A tool that extracts levels by scanning for
`FF D8 … FF D9` therefore has two ways to be wrong: it can find the embedded
thumbnail inside the level it wanted, and it can run past a truncated level into
the next chunk's bytes. Both produce a valid JPEG of the wrong size. That failure
is silent — the file opens, looks like a photograph, and is 160 px wide. Public
carving tools do exactly this, which is why one of them is cited in the notes as
a bug rather than a reference.

The container already tells us the truth: each chunk declares `header_len`,
`payload_len` and `padding_len`, and the payload's own SOF marker declares its
dimensions. Two independent witnesses for the same fact.

## Decision

Walk the chunks by their declared lengths. Read the SOF dimensions from the
declared payload and require the payload to be a complete JPEG within its
declared bounds. Treat any length overrun, header smaller than 24 bytes, missing
SOI, missing EOI at the declared end, or disagreement between declared and
decoded dimensions as a failure of that file.

We never search for `FF D8` or `FF D9`.

## Consequences

- A truncated or mis-framed file fails loudly with status `failed` instead of
  yielding a plausible thumbnail. That is the point: someone's last copy of a
  photo should not be a 160 px file labelled as the 1440 px preview.
- We give up a fallback that could have produced *something* from a damaged
  file. If damaged files turn out to be common, the answer is a separate,
  explicitly labelled recovery mode, not a quiet carve inside the normal path.
- We own a small SOF parser. That is cheaper than trusting a full decoder, and
  it keeps the dimension check independent of pixel decoding.
- Comparing header and payload dimensions is what makes the `suspect` status
  possible at all.
- Corrupt files produce no output, so the report has to carry the explanation;
  a bare "0 recovered" is not acceptable.

## Considered options

- **Carve, then pick the largest result.** Still cannot distinguish a large
  level from two adjacent chunks, and still silently returns thumbnails when the
  large level is truncated.
- **Decode every payload fully with a JPEG library.** Correct dimensions, much
  slower, and it hides container corruption behind decoder leniency.
