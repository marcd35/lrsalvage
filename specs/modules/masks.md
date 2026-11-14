---
id: masks
type: module-design
title: "masks: .lrcat-data AI mask layers, decoded to PNG"
status: draft
parent: architecture
references:
  - format-notes
  - glossary
---

# masks

## Responsibility

Decode `.lrcat-data` mask layers — the alpha channel of a Select Subject or
Select Sky edit — into 8-bit greyscale bitmaps and encode them as PNG.

## Boundary

Reads `.lrcat-data` payload files and nothing else: no catalog query, no
`previews.db`, no `.lrprev`. The layer's file name is its own identity (it equals
the `MaskDigest` in the develop settings), so no lookup is needed. Gated behind
the `masks` cargo feature, which is what makes `png` an optional dependency;
everything else in the crate compiles without it.

## Decisions

- **A separate command, because a mask is not a preview.** `.lrcat-data` sits
  beside the catalog and is easily mistaken for the preview cache; decoding it
  into the recovered photo tree would turn an alpha channel into something that
  looks like a picture. `masks` writes PNGs to a destination of its own.
- **The layout is parsed, not searched**: `<hex[0]>/<hex[..4]>/<32-hex-uppercase>.dat`,
  a 16-byte prefix, then a TIFF whose Adobe-Deflate strips use Predictor 2. Strip
  offsets are relative to the TIFF header, so the prefix is added.
- **None decoded is "nothing to do", not failure.** An empty directory, or every
  file unreadable, exits 3 with the reason on stderr; one bad layer is skipped by
  name while the rest still land.
- **`.lrcat-data` is read-only evidence** like the catalog and the cache: a
  destination inside it is refused before a directory is created.
