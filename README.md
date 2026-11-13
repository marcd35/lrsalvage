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
against the preview index, and prints totals — recoverable images, their size
tiers, and a count for each other state that occurs. It writes no files unless
you ask for a contact sheet. Give `--contact-sheet` a path to get a
self-contained HTML page of the images `extract` would write, each shown with the
smallest level in its pyramid — a contact sheet wants recognition, not
resolution.

A count is not a verdict. `--verbose` adds one line per plan item — per image at
its best level, plus one per extra level with `--all-levels`, plus one per
unmapped payload:

```text
status  tier  WxH  profile  relative-output
recovered  standard  1616x1077  sRGB  12.31.23/DSC01652.jpg
```

Fields are separated by two spaces, and a fact the plan does not know prints as
`-`, so the five columns stay put for a script. `--json` replaces the table with
one machine-readable object; `--verbose` lines follow it.

`scan --json` emits these fields, and only these:

| Field | Meaning |
|---|---|
| `catalog`, `previews` | The two input paths, with `/` separators. |
| `images` | Catalog images examined. |
| `recovered` | Images whose parsed payload agrees with the size `previews.db` declares. |
| `bytes` | Payload bytes of the items `extract` would write, before metadata is added. |
| `suspect` | Recovered, but the decoded size disagrees with `previews.db`. |
| `missing_payload` | `previews.db` has a row; the payload file is not on disk. |
| `no_cache_entry` | The catalog has the image; `previews.db` has no row. |
| `unmapped` | A payload that no catalog image claims. |
| `failed` | A payload that is truncated, unreadable, or holds no complete JPEG level. |
| `original_present` | Images skipped because the original file is still on disk. Always `0` with `--include-present`. |
| `below_floor` | Items the floor excluded. |
| `tiers` | Object keyed `full`, `large`, `standard`, `small`, `thumbnail`; absent tiers are absent keys. |

`extract` takes the same two paths plus a destination. It shares
`--min-long-edge`, `--all-levels`, and `--include-present` with `scan`; the table
names which command each flag belongs to:

| Flag | Effect |
|---|---|
| `--min-long-edge 640` | Ignore levels below this long edge (default 640; `0` disables the floor). |
| `--all-levels` | Export every intermediate level, not just the best one. |
| `--include-present` | Also include images whose original file still exists on disk. |
| `--verbose` | `scan` only: print one line per plan item instead of only totals. |
| `--json` | `scan` only: emit one machine-readable object instead of the table. |
| `--dry-run` | Do everything except write files, report files included. |
| `--force` | Replace a destination file that does not verify as ours: a complete JPEG whose dimensions match the planned item. Without it, such a file is reported as failed and left byte-identical. |
| `--no-metadata` | Do not add any metadata to the recovered JPEGs: no EXIF (so no orientation or rating), no rating XMP packet, no ICC profile. |
| `--no-timestamps` | Leave the file modification time at the time of writing, instead of setting it to the capture time. |

### What lands on disk

The output tree starts at the **last segment of
`AgLibraryRootFolder.absolutePath`**, then follows `pathFromRoot`, then the file
name:

```
C:/Users/marcc/Pictures/2023/12.31.23/DSC01652.ARW
  → E:\rescued\12.31.23\DSC01652.jpg
```

It does not mirror the whole absolute path, so it never reproduces
`Users\marcc\Pictures\2023\...` under the destination. Real catalogs commonly
point `absolutePath` at the folder that was imported — here `.../12.31.23/` —
and leave `pathFromRoot` empty. Where a catalog instead uses a drive-level root
and puts the subfolders in `pathFromRoot`, the first directory is that root's
last segment: `C:/Users/marcc/Pictures/` plus `pathFromRoot = 2023/12.31.23/`
becomes `Pictures\2023\12.31.23\...`. A bare drive root (`C:/`) has no usable
last segment, so that level is dropped rather than turned into a `C:` folder.

Virtual copies get the copy name appended: `DSC01652 - Black and white.jpg`.
Output JPEGs are never named after a uuid.

The largest level per image wins, and "largest" is decided by decoded JPEG
dimensions, not by byte count — a small file with a big header is still small.
`--all-levels` writes the rest beside it, with ` - 1200x800` added before `.jpg`
so the extra levels do not collide with the best one.

Three report files are written in the output root, beside the recovered tree:
`report.csv` (one row per plan item), `manifest.json` (the same facts with a
schema and version), and `extract.csv` (one row per attempted write, with its
SHA-256 and any per-file note). They are written after extraction, only on a run
that selected something and is not a dry run. None of them ever overwrites an
existing file: if `report.csv` is already there, the run writes `report (2).csv`
and says so. The numbered name applies to all three.

The run ends with a line on stderr such as
`78 evaluated, 78 written, 0 skipped, 0 failed, 25.3 MiB`. Sizes there are
mebibytes (1 MiB = 1,048,576 bytes). The reports keep exact bytes:
`manifest.json` as a total over the selected payloads, `extract.csv` per written
file.

## Safety

The catalog is the only copy of the map. It is treated as read-only evidence:

- **Read-only.** Neither the `.lrcat` nor the cache is ever opened for writing.
- **Snapshot.** The catalog is copied to a temporary directory before querying,
  WAL included, so a Lightroom session left open on the same catalog cannot see
  half a query and a half-written WAL cannot corrupt the read.
- **Refuses dangerous destinations.** `--out` inside the catalog's own directory
  or inside the preview cache is rejected before anything is written. Pointing
  the tool at `My Catalog Previews.lrdata` and telling it to fill that folder is
  how a recovery becomes a second data loss. The same check covers the report
  files in the output root and a requested `--contact-sheet`, whose parent
  directory is validated before the page is written.
- **`scan` writes one thing at most.** Without `--contact-sheet`, `scan` writes
  no file anywhere the user can see; with it, the contact sheet is the only
  output and it obeys the destination rule above.
- **Atomic writes.** Each JPEG is written to a temporary file, flushed, and
  renamed into place, so an interrupted run leaves no half-JPEG masquerading as
  a recovered one.
- **Resumable.** Re-running `extract` skips an output only when it verifies as
  ours: a complete JPEG whose dimensions match the planned item. An existing
  file that does not verify is left byte-identical and reported as failed;
  `--force` is the explicit way to replace it. Interrupting a run costs nothing.

## What the statuses mean

Every catalog image ends up in exactly one of these states; a payload that no
live catalog image claims is listed as `unmapped`.

| Status | Meaning |
|---|---|
| **recovered** | A payload was found and parsed, and its decoded dimensions agree with `previews.db`. `extract` writes it if it clears `--min-long-edge`; a recovered image below the floor is counted but not selected. |
| **suspect** | The payload is a real, complete photo whose decoded size disagrees with the size `previews.db` recorded. It is still extracted, subject to the floor, and flagged in every report: a real photo at the wrong declared size is worth recovering. |
| **original-present** | The original RAW/JPEG/TIFF is still on disk, so there is nothing to salvage. Skipped by default; `--include-present` turns these into ordinary recovery candidates. |
| **no-cache-entry** | The catalog has the image, but `previews.db` has no row for it. Nothing was ever rendered, or the row is gone. |
| **missing-payload** | `previews.db` has a row, but the payload file it names is not on disk. |
| **failed** | A payload exists but is truncated, unreadable, not a `.lrprev`, contains no readable JPEG level, or the level does not end at an end-of-image marker. |
| **unmapped** | A payload exists in the cache but no live catalog image claims it. Listed with its payload path; never renamed into your photo tree. |

`missing-payload` is worth reading twice. It means Lightroom believed a preview
existed and it is not on disk — either the cache was partially deleted, or you
are looking at a 13.3+ catalog whose `previews.db` rows are stale. Either way it
is a different problem from "no preview was ever built", which is why the report
keeps them apart.

## Metadata restored

The preview's pixels are only half of what the catalog knows. With metadata on
(the default), `extract` rebuilds three things around the untouched payload:

- **EXIF** — capture time, camera make and model, lens, ISO, exposure time,
  f-number, focal length, GPS latitude and longitude when present, the orientation
  that makes the picture display upright, and the star rating as EXIF tag
  `0x4746`.
- **XMP** — a small packet carrying the same rating as `xmp:Rating`, but only when
  the payload carries no XMP packet of its own. Merging into a packet the payload
  already has is not attempted: that packet is left byte-identical and the
  omission is recorded in the run's per-file note, while the rating still lands
  in EXIF.
- **ICC** — a profile reconstructed from `Pyramid.colorProfile`. The catalog
  records the profile's name and never its bytes, so `sRGB` becomes a
  2,084-byte `sRGB IEC61966-2.1` profile and `AdobeRGB` a 560-byte
  `Adobe RGB (1998)` profile. Names are matched case-insensitively and ignoring
  spaces and punctuation. Any other name is reported — `colour profile 'ACR' has
  no built-in ICC profile; none embedded` — and none is embedded: a wrong profile
  is worse than none for colour-critical work.

That ICC profile identifies the colour space; it is a reconstruction, not the
profile the original file carried, and should not be presented as the original.

Only what the catalog actually recorded is written. Missing fields stay missing —
no guessed timestamps, no invented camera names. An explicit `xmp:Rating = 0` is
a recorded rating, and is written as such.

The per-file note is the `note` column of `extract.csv`. It is not echoed for a
successful write, and a dry run writes no report at all, so a dry run's notes are
not shown; check a real run's `extract.csv` when colour matters.

Unless `--no-timestamps` is given, the file's modification time is set to the
capture time; if that date is missing or unparseable, the mtime is left alone.

The compressed image data is copied byte for byte. The only bytes added or
replaced are metadata segments: EXIF and XMP APP1, ICC APP2. An EXIF or ICC
segment already in the payload is replaced, never duplicated. With
`--no-metadata`, the payload is written unchanged. See
[ADR 0003](docs/adr/0003-preserve-preview-bytes.md).

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success. Also what `--help` and `--version` return. |
| 1 | Usage or input error: bad arguments, or the catalog / preview cache is missing, unreadable, or not a catalog. |
| 2 | `extract` completed with some failures: at least one selected image could not be written. The run did real work and also hit something it could not do. `scan` never returns 2. |
| 3 | Nothing selected to do: no image was selected to write — for example, every original is still present, or every preview is below the floor. `scan` and `extract` use the same test, so they agree on this code. `masks` returns 3 when it finds no `.dat` file or decodes none. |
| 4 | Unsafe or unwritable destination: `--out` (or the contact-sheet directory) resolves inside the catalog or preview cache — or, for `masks`, inside the `.lrcat-data` directory — or cannot be created or written. |

`2` is the one to pay attention to in scripts: the report says which items failed
and why. `3` is not an error — a catalog where every original is still on disk
legitimately has nothing to salvage.

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
Select Sky). Nothing recoverable lives there. If you want those masks,
`lrsalvage masks --data "...\Photos.lrcat-data" --out "E:\masks"` decodes them to
PNG; it is a separate command precisely because they are not photographs. See
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
