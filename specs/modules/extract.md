---
id: extract
type: module-design
title: "extract: the only writer of recovered JPEGs"
status: draft
parent: architecture
depends-on:
  - plan
  - exif
  - jpeg
  - lrprev
  - model
  - report
references:
  - adr-0003-preserve-preview-bytes
---

# extract

## Responsibility

Write the items a plan selected: assemble each output (payload bytes, plus the
metadata segments `exif` builds), place it at the planned path atomically, verify
what is already there before touching it, and return one outcome per attempt with
its byte count, SHA-256 and note.

## Boundary

The only module that writes user-visible output, and the only one that refuses a
destination. It decides nothing about which image deserves recovery — that is
`plan` — and knows no format grammar beyond asking `lrprev` / `jpeg` / `exif`.

## Decisions

- **An existing destination is replaced only when it verifies as ours**: a
  complete JPEG whose SOF dimensions match the planned item. Anything else is
  left byte-identical and reported as `failed` unless `--force` explicitly asks
  for the replacement. Resuming a run therefore costs nothing and destroys
  nothing it cannot prove it wrote.
- **Temp-then-rename, never delete-then-rename.** The write goes to a unique
  `.<stem>.<pid>-<n>.partial` beside the destination, is flushed with `sync_all`,
  and is renamed into place; `rename` replaces an existing file on both Windows
  and POSIX, so an interrupted run cannot destroy the file it was replacing and
  cannot leave a half-JPEG wearing the recovered name.
- **Destination safety is checked before the directory is created.** Both the
  already-existing and the not-yet-existing case are handled: the nearest
  existing ancestor is canonicalised and the remaining components applied
  lexically, so `--out catalog/../catalog/out` is refused while the directory
  still does not exist. The same rule covers `.lrcat-data` for `masks`.
- **A dry run writes nothing but does all the rest**: same selection, same
  metadata assembly, same verification, `dry-run` outcomes, no report files.
- **Metadata is a segment-level edit.** The payload's compressed image data is
  copied verbatim; `--no-metadata` writes it with nothing added
  (`adr-0003-preserve-preview-bytes`). A profile name with no built-in profile
  yields a note, not a substitute.
- **Capture time becomes the file modification time** unless `--no-timestamps` is
  given or the date is missing or unparseable — in which case the mtime is left
  as written rather than guessed.
