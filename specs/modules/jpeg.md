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
given.

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
