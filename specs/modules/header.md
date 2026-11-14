---
id: header
type: module-design
title: "header: the Lua-like pyramid header, parsed as data"
status: draft
parent: architecture
references:
  - format-notes
---

# header

## Responsibility

Turn the `header` chunk's Lua-like table into typed values: colour profile name,
stored dimensions, per-level dimensions, quality, uuid, digest, format version.

## Boundary

Untrusted disk data is parsed, never executed — a hand-written parser instead of
an embedded Lua runtime. It reads bytes and returns data; it consults no database
and writes nothing.

## Decisions

- **Nothing in production calls it, and that is deliberate.** `previews.db` is
  authoritative for dimensions, the colour profile name and the image → payload
  mapping (`adr-0002-map-via-previews-db`), so parsing a second source of the same
  facts would only manufacture disagreement to resolve. Its only caller is a
  parser test in `lrprev`.
- **If a future change does need the header, settle which source wins first.**
  Adding a consumer here without that decision reintroduces the drift the ADR
  removed; treat the header's values as a cross-check, never as a fallback that
  silently wins.
- **`formatVersion` and `quality` are not gates.** Observed values are partial
  (`formatVersion = 3`, `quality = "embedded"`), and the mapping from the
  header's quality to the database's is unverified, so neither may decide whether
  a file is readable.
