---
id: jpeg
type: module-design
title: "jpeg: dimensions and completeness, no pixels"
status: draft
parent: architecture
references: [adr-0001-parse-dont-carve, adr-0004-detect-layout-not-version]
---

# jpeg

## Responsibility

Answer two questions about a byte slice without decoding it: what dimensions does
its start-of-frame marker declare, and is it a complete JPEG within the bounds
given. Certify a payload in one pass — the answer `adr-0005-discover-payloads-by-index`
requires before a file is believed — and hand back the top-level segment list so
`exif` can name what the payload carries. Answer one question about a *file* with
its first two bytes: is this payload-like at all.

## Boundary

A walking segment parser over a `&[u8]` and an `error::Error`, plus one two-byte
read of a file. No pixel access, no metadata interpretation. The only file it
ever opens is the payload-like sniff, which reads at most two bytes and scans
nothing; everything else takes bytes, so no caller has to teach this module about
files. Callers decide what a disagreement or an incompleteness means.

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
- **The payload-like test is two bytes, and it is not a scan.**
  `is_payload_start(path)` reads at most the file's first two bytes and answers
  whether they are `FF D8`. That is ADR 0004's rule for what counts as
  payload-like, and its narrowness is the decision: a file that merely *contains*
  a `FF D8` is not payload-like, so nothing is searched for and an embedded
  thumbnail can never make a file claim a shape it does not have. An unreadable
  or absent file is not payload-like — a classification of what was seen.
- **Certification takes bytes, never a path.** `plan` certifies the level it
  chose and `extract` re-certifies the bytes it is about to write; neither has to
  teach this module about files, and a bounded read could never be allowed to
  misjudge a large `APP1` as a broken frame.
