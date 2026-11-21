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
  - fs
references:
  - adr-0003-preserve-preview-bytes
  - adr-0006-degrade-without-guessing
---

# extract

## Responsibility

Write the items a plan selected: assemble each output (payload bytes, plus the
metadata segments `exif` builds), place it at the planned path atomically, verify
what is already there before touching it, and return one outcome per attempt with
its byte count, SHA-256 and note. Review uses the same read-only payload
reader without a destination.

## Boundary

The only module that writes user-visible output, and the only one that refuses a
destination. Every read and write it performs is an `fs` operation, so the
staging name, the `fsync` and the `rename` are one call this module cannot
half-perform. It decides nothing about which image deserves recovery — that is
`plan` — and knows no format grammar beyond asking `lrprev` / `jpeg` / `exif`.

## Decisions

- **Only selected items are written.** An unclaimed payload has no output path,
  is never selected, and is never written: a file with no catalog row has no name
  to give it, and inventing one would put an unnamed JPEG in the same tree as the
  named recoveries. A legacy container superseded by a pyramid's level files is
  not read at all (`adr-0006-degrade-without-guessing`). Every payload that *is*
  written goes through this one builder, whatever its layout.
- **The item's payload is written, not re-chosen.** A `LevelFile` payload is the
  level: its bytes are read whole and written verbatim. A `Container` payload is
  written at the `level_N` chunk the plan certified — never "the chunk whose
  dimensions happen to match", and never the largest as a fallback. Both are
  re-certified from the bytes actually read (`SOI` … `FF D9`, `SOF` dimensions
  equal to the item's), so a cache edited between `scan` and `extract` fails
  loudly instead of writing something no plan ever approved. `read_preview`
  returns the certified bytes and their EXIF orientation to both the writer
  and browser review.
- **An existing destination is replaced only when it proves it is ours**: its
  EXIF carries our `Software` marker (`lrsalvage <version>`), the file is a
  complete JPEG, and its `SOF` dimensions match the planned item. Anything else —
  a foreign same-size JPEG, a file from another tool, output written before the
  marker existed — is left byte-identical and reported as `failed` unless
  `--force` explicitly asks for the replacement. Resume proves authorship, not
  plausibility (`adr-0003-preserve-preview-bytes`). The marker is read by `exif`
  from the destination's own `APP1`; the comparison is a prefix — any version of
  `lrsalvage ` counts — so a tool upgrade keeps the files it wrote.
- **`--no-metadata` output carries no marker, so a metadata-free run never
  verifies anything.** Verification is not attempted at all when the run writes
  no metadata: its own earlier output has nowhere to record identity, and a
  metadata-bearing file must not be silently kept by a run that promised the
  payload's bytes. Either way the destination is reported as not a verified
  extraction and asks for `--force`. The flag's help says so.
- **Temp-then-rename, never delete-then-rename.** The write is
  `fs::write_atomic`: a unique sibling, flushed to the device, then renamed into
  place; `rename` replaces an existing file on both Windows and POSIX, so an
  interrupted run cannot destroy the file it was replacing and cannot leave a
  half-JPEG wearing the recovered name. Because it is one operation, a note about
  a failed write names the destination rather than the staging file it never
  handed out.
- **Destination safety is checked before the directory is created.** Both the
  already-existing and the not-yet-existing case are handled: the nearest
  existing ancestor is canonicalised and the remaining components applied
  lexically, so `--out catalog/../catalog/out` is refused while the directory
  still does not exist. The same rule covers `.lrcat-data` for `masks`.
- **The destination's room is checked before the first write (#64).**
  `extract::pending_bytes` is the number: the plan's own total — `plan.stats.bytes`,
  which `--min-long-edge`, `--max-long-edge` and `--all-levels` move because it is
  what this run would write at those options — less every item whose destination
  already exists. An existing destination is skipped when it verifies as ours and
  reported failed when it does not, and neither writes a byte, so those bytes are
  not room this run needs; counting them would make a first pass that filled the
  destination refuse every later resume, for the bytes it had already written. It
  costs one `stat` per selected item and reads no destination. Under `--force`
  nothing is subtracted, because a forced run may replace what is there — an
  over-estimate in the one case a forced item would in fact have verified, which
  errs towards refusing rather than towards filling the disk.
  `extract::ensure_room` compares that number against the free space of the
  nearest existing ancestor. That ancestor is asked because the destination may
  not exist yet: a dry run creates nothing, and a real run asks about the
  directory its own destination check has already created — so both ask a path
  that exists, and neither may create one in order to ask. The comparison has no
  margin — exactly enough proceeds, one byte short is refused — and the refusal is
  `error::Error::Destination`, the crate's existing "this destination cannot be
  used" variant, so exit `4` gains a trigger and no code. The message states the
  shortfall twice, in exact bytes and in `report`'s MiB form. A destination that
  fits but would be left under 1 GiB free is *nearly full*, which the run says
  rather than leaving a reader to subtract two numbers; that constant is a
  statement about what is left behind, never a margin the comparison adds, and a
  run with nothing left to write is never nearly full. A destination that cannot
  be measured — a share whose filesystem will not answer — proceeds with one note
  rather than becoming unextractable: the check is a guard, not the run's premise,
  and `wasm32` answers every operation this way by design (`fs`). `--dry-run` runs
  the same check and prints the same verdict, and it is where the numbers are most
  useful, since it writes nothing.
- **The room it measures is the payload still to be written, and the message says
  so.** The EXIF/XMP/ICC segments this module adds are not counted, so a
  destination holding exactly what is left still lets the last file fail on space,
  as it does today. The shortfall a run states is the plan's, not a promise about
  the finished sizes.
- **A dry run writes nothing but does all the rest**: same selection, same
  metadata assembly, same verification, `dry-run` outcomes, no report files.
- **Metadata is a segment-level edit.** The payload's compressed image data is
  copied verbatim; `--no-metadata` writes it with nothing added
  (`adr-0003-preserve-preview-bytes`). A profile name with no built-in profile
  yields a note, not a substitute. A payload that already carries an ICC profile
  keeps it, and one that carries an XMP packet keeps that too — the outcome says
  which segment came from where. The orientation written is the item's
  `effective_orientation` — the payload's own value when its EXIF declares one,
  else the catalog's — so the file states what describes its own pixels and the
  report prints the value actually written.
- **Capture time becomes the file modification time** unless `--no-timestamps` is
  given or the date is missing, unparseable, or ambiguous in the machine's local
  zone — a DST fold reads the same wall clock twice, a spring-forward gap never —
  in which case the mtime is left as written rather than guessed. The value
  arrives already normalised to `YYYY-MM-DDTHH:MM:SS` by `catalog`, so nothing
  here reads a fraction or an offset out of it.
