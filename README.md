# lrsalvage

Recover JPEG previews from an Adobe Lightroom Classic catalog when the original
RAW or JPEG files are gone.

You have a `.lrcat` and a `* Previews.lrdata` directory on an old drive. The
photos they point at are not there any more. Lightroom still has a rendered copy
of each one in its preview cache, and this tool writes those copies out as
ordinary JPEG files, with the catalog's metadata attached.

Version 0.1. Early, and deliberately conservative: it would rather tell you it
found nothing than hand you a folder of wrong images.

## The hard truth, before anything else

These are **previews**, not originals.

- A standard preview is 1440 px on the long edge — typically 1440 × 960. That is
  what most catalogs contain, and 1440 px is the ceiling for those images.
- Only images whose 1:1 previews were built have anything larger. In one real
  3,420-entry cache, 53 images had 5,760 × 3,840 previews; the rest topped out at
  1,440 × 960 or smaller.
- Previews are lossy JPEGs. Nothing here can add back detail that was never
  stored.
- A recovered preview is **not** a restored photo. You get a usable picture; you
  do not get your RAW file back.

If that is still worth having — and after a drive failure, it usually is — read
on.

## Quick start

```console
# 1. See what is there. Writes nothing.
lrsalvage scan ^
  --catalog "D:\old\Photos\Photos.lrcat" ^
  --previews "D:\old\Photos\Photos Previews.lrdata"

# 2. Write the recoverable JPEGs.
lrsalvage extract ^
  --catalog "D:\old\Photos\Photos.lrcat" ^
  --previews "D:\old\Photos\Photos Previews.lrdata" ^
  --out "E:\rescued"
```

On macOS or Linux, use a trailing backslash instead of `^` to continue a line.

`scan` is the safe first move: it opens everything read-only, joins the catalog
against the preview index, and prints a per-image verdict plus totals. It writes
no files. Add `--json` for machine-readable output, or give `--contact-sheet` a
path to get a self-contained HTML page of the best available preview per image —
the fastest way to see what you actually still have.

`extract` takes the same two paths plus a destination. Useful flags:

| Flag | Effect |
|---|---|
| `--dry-run` | Do everything except write files. |
| `--min-long-edge 640` | Ignore previews below this long edge (default 640). |
| `--all-levels` | Export every intermediate level, not just the best one. |
| `--json` | Machine-readable report (on `scan`). |

### What lands on disk

The Lightroom folder tree is mirrored under `--out`, so `2019\Iceland\_MG_0233.dng`
becomes `E:\rescued\2019\Iceland\_MG_0233.jpg`. Virtual copies get the copy name
appended: `_MG_0233 - Black and white.jpg`. Nothing else is written, and no file
is ever named after a uuid.

The largest level per image wins, and "largest" is decided by decoded JPEG
dimensions, not by byte count — a small file with a big header is still small.
`--all-levels` writes the rest beside it.

## Safety

The catalog is the only copy of the map. It is treated as read-only evidence:

- **Read-only.** Neither the `.lrcat` nor the cache is ever opened for writing.
- **Snapshot.** The catalog is copied to a temporary directory before querying,
  WAL included, so a Lightroom session left open on the same catalog cannot see
  half a query and a half-written WAL cannot corrupt the read.
- **Refuses dangerous destinations.** `--out` inside the catalog's own directory
  or inside the preview cache is rejected before anything is written. Pointing
  the tool at `My Catalog Previews.lrdata` and telling it to fill that folder is
  how a recovery becomes a second data loss.
- **Atomic writes.** Each JPEG is written to a temporary file and renamed into
  place, so an interrupted run leaves no half-JPEG masquerading as a recovered
  one.
- **Resumable.** Re-running `extract` skips outputs that already exist and picks
  up the rest. Interrupting it costs nothing.

## What the statuses mean

Every image in the scan ends up in exactly one of five states:

| Status | Meaning |
|---|---|
| **recovered** | A payload was found and parsed; `extract` writes it. |
| **unmapped** | A payload exists in the cache but no catalog image claims it. Listed, not renamed into your photo tree. |
| **skipped** | No JPEG was produced, and nothing was corrupt: the catalog has no cache entry (`no-cache-entry`), the entry points at a payload that is gone (`missing-payload`), or the best level is below `--min-long-edge`. |
| **failed** | A payload exists but is truncated, unreadable, or not a JPEG. |
| **suspect** | A payload decoded, but its dimensions disagree with what the catalog recorded. Flagged in the report rather than silently trusted. |

`missing-payload` is worth reading twice. It means Lightroom believed a preview
existed and it is not on disk — either the cache was partially deleted, or you
are looking at a 13.3+ catalog whose `previews.db` rows are stale. Either way it
is a different problem from "no preview was ever built", which is why the report
keeps them apart even though both count as skipped.

## Metadata restored

The preview's pixels are only half of what the catalog knows. `extract` writes
back, into the JPEG's EXIF/XMP:

- capture time, camera make and model, lens
- ISO, exposure time, f-number, focal length
- star rating
- GPS latitude and longitude, when present
- orientation, so viewers show the picture upright
- an ICC profile matching the colour profile the catalog recorded (sRGB or
  AdobeRGB)

Only what the catalog actually recorded is written. Missing fields stay missing —
no guessed timestamps, no invented camera names. The pixels themselves are copied
byte for byte; see [ADR 0003](docs/adr/0003-preserve-preview-bytes.md).

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success; everything recoverable was recovered. |
| 1 | Usage or input error: bad arguments, or the catalog / cache is missing or unreadable. |
| 2 | Partial: some images were recovered, some failed. |
| 3 | Nothing recoverable: the catalog and cache were readable but yielded no usable preview. |
| 4 | Unsafe destination: `--out` is inside the catalog or cache, or is not writable. |

`2` is the one to pay attention to in scripts. It means the run did real work and
also hit something it could not read; the report says which.

## What this cannot do

- **Recover originals.** No tool can. A preview is a derivative, and the RAW is
  not hiding inside it.
- **Beat the cache.** 1440 × 960 is the ceiling unless 1:1 previews were built.
  Expect 5,760 px only for the images whose 1:1 previews were.
- **Re-render your edits at full size.** The preview has Lightroom's adjustments
  baked into its pixels. That is often exactly what you want to look at, but
  there is no full-resolution render hiding in the cache, and this tool does not
  export the develop settings themselves.
- **Recover video.** Motion previews are poster frames, not the clip.
- **Recover images that never had previews.** Discarded previews, images imported
  but never rendered, and catalogs that were only ever used in Library grid view
  may have nothing but grid thumbnails.
- **Undo compression.** A JPEG that was re-encoded once cannot be made lossless
  again. `lrsalvage` never re-encodes, so at least it does not add a second
  generation of loss.

One confusion worth heading off: a `.lrcat-data` directory sitting next to the
catalog is **not** preview data. It holds AI mask alpha channels (Select Subject,
Select Sky). Nothing recoverable lives there. See
[docs/format-notes.md](docs/format-notes.md) section 3.

## Building

Requires Rust 1.85 or newer.

```console
cargo build --release
```

The binary is `target/release/lrsalvage.exe` on Windows, `target/release/lrsalvage`
elsewhere. No Lightroom installation is needed, and no network access.

```console
cargo test
cargo clippy -- -D warnings
```

Tests that need a real catalog are `#[ignore]`d and read their fixtures from
environment variables, so the default `cargo test` runs against synthetic
fixtures only.

## Licence

Dual-licensed under either of

- MIT
- Apache License, Version 2.0

at your option. See [`LICENSE-APACHE`](LICENSE-APACHE) for the Apache text.
