---
id: jpeg
type: module-design
title: "jpeg: dimensions and completeness, no pixels"
status: draft
parent: architecture
references:
  - adr-0001-parse-dont-carve
---

# jpeg

## Responsibility

Answer two questions about a byte slice without decoding it: what dimensions does
its start-of-frame marker declare, and is it a complete JPEG within the bounds
given. Certify a payload in one pass — the answer `adr-0005-discover-payloads-by-index`
requires before a file is believed — and hand back the top-level segment list so
`exif` can name what the payload carries.

## Boundary

A walking segment parser over a `&[u8]` and an `error::Error`. No pixel access,
no metadata interpretation, no file I/O. Callers decide what a disagreement or an
incompleteness means.

## Decisions

- **A hand-written SOF walk, not a decoder.** Correct dimensions, cheap, and it
  keeps the dimension check independent of decoder leniency — a library that
  repairs a truncated stream would hide exactly the corruption we need to see
  (`adr-0001-parse-dont-carve`).
- **Width and height may be swapped by the caller, not here.** The stored render
  is unrotated and the cache may record the upright shape; comparing them is
  `plan`'s job, so this module reports what the bytes say.
- **Completeness means SOI at the start and EOI at the declared end**, with a
  legal segment walk in between. That is what distinguishes our own earlier
  output from an unrelated file of the same name during a resumed run.
- **Certification is one walk and it is this module's.** `certify` returns the
  frame dimensions, completeness, and the top-level segments before `SOS`
  (marker + range): `SOI` at 0, every segment length declared, `FF D9` at EOF.
  APPn and COM payloads are skipped wholesale by their own length, so an embedded
  EXIF thumbnail is never on the chain — the guarantee `adr-0001-parse-dont-carve`
  gave the container, restored for a payload with no framing of its own.
- **A segment list is framing, not meaning.** Which bytes are segments, and where
  each one starts and ends, is this module's business; whether an `APP1` is
  `Exif`, an XMP packet or neither belongs to `exif`, which consumes the same
  list. One walk per file, two readers of its result.
- **Certification takes bytes, never a path.** `plan` certifies the level it
  chose and `extract` re-certifies the bytes it is about to write; neither has to
  teach this module about files, and a bounded read could never be allowed to
  misjudge a large `APP1` as a broken frame.
