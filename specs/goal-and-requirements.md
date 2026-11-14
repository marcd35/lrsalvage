---
id: lrsalvage
type: goal-and-requirements
title: "lrsalvage: recover Lightroom previews when the originals are gone"
status: draft
---

# lrsalvage

## Goal

When a Lightroom Classic catalog survives and the photographs it catalogues do
not, write out the previews Lightroom had already rendered — as ordinary JPEG
files, named after the original, carrying the metadata the catalog still holds.

## Problem

Lightroom renders a preview of every image it imports so it never has to
re-render one just to show it. That cache outlives a failed drive, a dead laptop
and a wiped backup folder, and it is often the last record of what a photograph
looked like. Nothing in it is an original: a standard preview is 1440 px on the
long edge and a lossy JPEG, and only images whose 1:1 previews were built have
anything larger.

So the value recovered is not pixels, it is a photograph identified: the right
name in the right folder, with its date, camera, rating and orientation, upright,
and with the colour space stated rather than implied. That is why metadata is
restored — and why it is never guessed.

## Audience

Someone holding a `.lrcat` and a `* Previews.lrdata` — from a dead machine, an
external disk, a backup — with no Lightroom install, licence, or running copy of
Lightroom required. Also anyone who wants the previews out of a live catalog
without an Adobe toolchain.

## Capabilities

- **Survey** (`scan`) — read-only. Places every catalog image in exactly one
  status, counts them by size tier, and reports totals; `--verbose` adds one line
  per plan item, `--json` emits one machine-readable object, and `--contact-sheet`
  writes the one artifact `scan` is allowed to write: a self-contained HTML page
  of the images `extract` would produce.
- **Recover** (`extract`) — writes the largest level of each pyramid as a JPEG
  under a destination that mirrors the catalog's own folder structure, plus
  `report.csv`, `manifest.json` and `extract.csv`. Same options as `scan`, plus
  `--all-levels`, `--dry-run`, `--force`, `--no-metadata`, `--no-timestamps`.
- **Decode AI masks** (`masks`) — `.lrcat-data` beside a catalog holds AI mask
  alpha channels, not photographs. Decoded to PNG by a separate command, because
  a mask is a different kind of evidence and must never be mistaken for a preview.

## Durable principles

1. **The payload is the preview.** Compressed image data is copied verbatim;
   only metadata segments are added or replaced. No re-encoding, no pixel
   rotation — see `adr-0003-preserve-preview-bytes`. Where a segment is about to
   be written, the payload's own wins when it is the stronger evidence: a real
   ICC profile or a payload-declared orientation outranks anything rebuilt from
   the catalog, while our EXIF supersedes a render-time snapshot of the same
   catalog facts, and the trade is reported rather than silent.
2. **Parse, don't carve.** Container lengths are authoritative; a marker scan can
   return an embedded thumbnail — see `adr-0001-parse-dont-carve`.
3. **The catalog is the map.** Adobe's `(uuid, digest)` rows are the only
   authority for image → payload — see `adr-0002-map-via-previews-db`.
4. **The catalog and the cache are read-only evidence.** Nothing is ever opened
   for writing, the catalog is queried through a snapshot, and a destination
   inside either tree (or inside `.lrcat-data`) is refused before anything is
   written.
5. **A miss is stated, not guessed.** A wrong image is worse than a missing one,
   so a damaged payload fails loudly with a reason and a partial recovery is
   reported as partial, never as success. A cache whose index is missing or
   unreadable is reported the same way — payloads listed as unmapped, nothing
   written for them — rather than refused, and no mapping is invented to fill the
   gap. See `adr-0006-degrade-without-guessing`. The same rule governs a resume: a
   file we cannot prove we wrote is not ours, and is never kept in place of a
   recovery.
6. **Only what the catalog recorded.** No invented timestamps, no substituted
   colour profile, no merged XMP packet — an absent fact stays absent.
7. **Library first.** The core crate's public API is the supported seam; the CLI
   is one adapter over it, so a GUI or a script can drive the same verified
   pipeline. See `architecture`.

## Verification

Every promise above is proved by a synthetic **vault**: one temporary catalog plus
one preview cache, built at test time by the one shared builder in
`crates/lrsalvage-fixtures`, never a committed binary. The matrix in `fixtures`
names, row by row, the byte shape, the status, the statistic, the exit code and
the regression each row pins, so a status or a statistic arrives with its row.

Real catalogs are evidence, not fixtures: they are reached only from `#[ignore]`d
tests through `LRSALVAGE_TEST_CATALOG` and `LRSALVAGE_TEST_PREVIEWS`, and no real
catalog byte is ever committed. A byte fact learned from a real file is recorded
in `format-notes` with its evidence label before a fixture builds on it; a shape
nobody has observed is labelled `synthetic` in the matrix and never presented as
Adobe's.

## Non-goals

- **Restoring originals.** A preview is a derivative; the RAW is not inside it.
- **Recovering video.** Motion previews are poster frames, not clips.
- **Re-rendering edits at full size.** The adjustments are baked into the cached
  pixels and the develop settings are not exported.
- **Undoing compression.** A JPEG that was re-encoded once cannot be made
  lossless again.
- **Giving the cache more pixels than it has.** 1440 px is the ceiling unless a
  1:1 preview was built.
- **Recovering images that never had previews.** Nothing to salvage, by
  construction.
- **Needing Lightroom at runtime.** No plugin, SDK, COM interface, or Adobe
  module — the tool is a file reader.
- **Being a general JPEG repair tool.** A damaged preview is reported, not
  carved into something plausible.
