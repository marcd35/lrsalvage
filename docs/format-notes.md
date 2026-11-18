---
id: format-notes
type: architecture-design
title: "Lightroom preview formats: engineering notes"
status: active
parent: architecture
tags:
  - format
  - reference
---

# Lightroom preview formats: engineering notes

> Findings below were established by inspecting real catalogs in November 2026.
> Each section states its evidence; nothing here is copied from a vendor
> specification, because no vendor specification exists.

Byte-level notes on what `lrsalvage` reads: the `.lrprev` preview container, the
`previews.db` index, the `.lrcat-data` sidecar, the `.lrcat` catalog itself, and
the colour/orientation rules that decide what a recovered JPEG is worth.

These notes exist because no public source documents all of this at this
fidelity. The upstream tools each know a fragment: the Go one parses chunks, the
Python ones glob for payloads, the Rust one reads only the catalog. This file is
the one place where the fragments are reconciled against real files, with the
evidence stated per claim.

## Evidence standard

Every claim below carries one of these labels:

| Label | Meaning |
|---|---|
| **verified** | Observed directly on real catalog / cache files in November 2026, on this machine. Other agents can re-check it against the same fixtures. |
| **reported** | Asserted by a named public source (section 7). We did not reproduce it. |
| **unverified** | A working hypothesis. Plausible from the bytes, but not tested. Do not build on it without a fixture. |
| **TODO** | Known gap. Nobody has looked yet. Recorded so it is not mistaken for settled. |

Two rules for editing this file: a claim without a label is a bug, and a new
fact gets its label before it gets prose.

## 0. What was inspected

| Fixture | What it is | Used for |
|---|---|---|
| Small cache | 78-file preview cache, Lightroom Classic ≤ 13.2, Windows | `.lrprev` layout, `root-pixels.db` |
| Large catalog | Catalog with 3,416 images and 3,420 cache entries, ≤ 13.2 | `previews.db` schema, statistics, orientation |
| Catalog with AI masks | Catalog whose `<catalog>.lrcat-data` directory is populated | `.lrcat-data` mask decoding |
| Upgraded 78-image cache | The small catalog above, upgraded in place by Lightroom Classic 16.0 on 2026-11-14 | 13.3+ layout, `previews.db` schema delta |
| Freshly rendered 16.0 caches ×3 | Three catalogs built from scratch by the same Lightroom Classic 16.0 on 2026-11-15 — Standard Preview Size `Auto (3840px)`, `1024 pixels`, and 1:1 previews — 10 images each from one import folder | the render half of section 5: 5.5, 5.8, 5.10 |
| A discarded copy of the `Auto (3840px)` render | That cache copied whole, with `Library > Previews > Discard Standard and 1:1 Previews` applied to all ten images by Lightroom Classic 16.0 on 2026-11-16; the cache it was copied from was left untouched, byte for byte | the discard half of section 5: 5.11 |

Until 2026-11-15 no **freshly built** 13.3+ catalog had been inspected: the
modern cache in the second row is a *conversion* of a legacy cache. Three were
then rendered from scratch, and section 5.10 records what they are. What a render
differs in is the level series, the payload's own metadata segments and the
number of pyramids per image; what it keeps is the path rule, `previews.db` and
its `StoreInfo.version` of `13.0`.


---

## 1. `.lrprev` container (Lightroom ≤ 13.2) — VERIFIED

A `.lrprev` file is a flat sequence of length-prefixed chunks. There is no file
header, no trailer, no checksum, and no compression at this level: the declared
lengths are the whole grammar. All integers are **big-endian**.

### 1.1 Chunk header

| Offset | Size | Field | Notes |
|---|---|---|---|
| 0 | 4 | magic | ASCII `AgHg`, bytes `41 67 48 67` |
| 4 | 2 | `header_len` | u16, self-inclusive: counts these 4 magic bytes too |
| 6 | 1 | `version` | TODO: observed values not recorded; the one published hexdump shows `0` |
| 7 | 1 | `kind` | TODO: observed values not recorded; published hexdump shows `0` |
| 8 | 8 | `payload_len` | u64 |
| 16 | 8 | `padding_len` | u64, bytes to skip *after* the payload |
| 24 | `header_len - 24` | `name` | ASCII, NUL padded |
| `header_len` | `payload_len` | payload | the chunk's data |
| `header_len + payload_len` | `padding_len` | pad | skipped by length; contents not inspected (TODO) |

`header_len` is the offset at which the payload starts, so the name field is
`header_len - 24` bytes wide. Since `header_len` includes the magic, the
smallest legal value is 24 (empty name) and anything below that means the file
is corrupt, not a variant.

Chunks repeat until EOF. A chunk whose header runs past EOF, or whose declared
payload does not fit in the remaining bytes, is truncation - fail the file
rather than resynchronising (ADR 0001).

**The final chunk may omit its padding** (**verified**). Measured on 154 of the
3,057 payloads present in the large catalog: the declared `padding_len` runs past
EOF by exactly that chunk's padding, while the payload itself ends exactly at EOF
and is complete. The parser therefore tolerates a shortfall only when the chunk
payload ends exactly at the end of the file; a payload that runs past EOF is
still truncation and still fails the file. No chunk other than the last may be
short.

### 1.2 Worked example

Real file `1418F22B-41B0-4786-9DF0-D5081559CAD1-bd012a8e0e0b5b73ddaf34aeceb8bf48.lrprev`,
428,784 bytes, six chunks, from the 78-file cache (**verified**).

| # | `name` | `payload_len` | `padding_len` | declared size (header `levels`) | chunk bytes |
|---|---|---|---|---|---|
| 1 | `header` | 447 | 1 | — | 480 |
| 2 | `level_1` | 2,791 | 9 | 101 × 68 | 2,832 |
| 3 | `level_2` | 7,536 | 0 | 202 × 135 | 7,568 |
| 4 | `level_3` | 25,989 | 11 | 404 × 270 | 26,032 |
| 5 | `level_4` | 89,230 | 2 | 808 × 539 | 89,264 |
| 6 | `level_5` | 302,569 | 7 | 1,616 × 1,077 | 302,608 |
| | | 428,562 | 30 | | **428,784** |

The `chunk bytes` column is derived, not read: `header_len + payload_len +
padding_len`. With `header_len = 32` the six chunks sum to exactly the file size
(32·6 + 428,562 + 30 = 428,784), which closes the format — there is nothing else
in the file. Solving that equation is also how `header_len = 32` was established,
i.e. an 8-byte name field: `header` plus two NULs, `level_5` plus one NUL.

An independent published hexdump (seachess, 2022) shows the same bytes at offset
4: `00 20`, then `00 00` for version/kind, a `u64` payload length of `0x247`,
padding `9`, and `header\0\0` (**reported**). Two independent observations of the
same 32-byte header is the strongest evidence in these notes.

Observed ordering: `header` is always first, then `level_1` … `level_N` in
ascending size. Chunk count is `N + 1`. `level_1` is the smallest and `level_N`
the largest; for this file, payload byte size and declared dimensions both
increase monotonically with `N` (**verified**).

### 1.3 The `header` payload

A Lua-like table, stored as untrusted text. It is data: parse it, never execute
it. Verbatim from the example file:

```
pyramid = {
	colorProfile = "sRGB",
	croppedHeight = 1077,
	croppedWidth = 1616,
	digest = "bd012a8e0e0b5b73ddaf34aeceb8bf48",
	formatVersion = 3,
	levels = {
		{ height = 68, width = 101, },
		{ height = 135, width = 202, },
		{ height = 270, width = 404, },
		{ height = 539, width = 808, },
		{ height = 1077, width = 1616, },
	},
	quality = "embedded",
	uuid = "1418F22B-41B0-4786-9DF0-D5081559CAD1",
}
```

Notes on the fields (**verified** for this file):

- `uuid` and `digest` match the file name and the `previews.db` row. The digest
  is lowercase hex; the uuid is uppercase with dashes. Both are significant to
  the path on case-sensitive filesystems.
- `formatVersion` is `3` here. Other values exist; do not require `3` (TODO:
  record which Lightroom versions emit which value).
- `levels` is ascending and has one entry per `level_N` chunk. It carries
  dimensions only — no offsets, no byte counts, no per-level quality.
- **quality** is `embedded` here. The value space overlaps `Pyramid.quality`
  (section 2.3), but how the header's value maps onto the database's is
  **unverified**.
- `croppedWidth`/`croppedHeight` are the *stored* (unrotated) render dimensions,
  not the upright image — the same rule as `Pyramid` (section 2.5).
- Indentation is tabs, keys alphabetical-ish but not sorted (`levels` sits before
  `quality`). Do not rely on key order.

Padding bytes sit *after* the payload, so the next chunk's magic is at
`header_len + payload_len + padding_len`. Skip forward by length; do not search
for `AgHg`.

### 1.4 What is in a level

One complete JPEG per level. A level is a self-contained JPEG, so it may carry its
own marker segments, including an embedded EXIF thumbnail: a `FF D8 … FF D9`
scan inside a level can find that thumbnail rather than the level. That is why
marker scanning is unsafe.

It is **not** true that every level carries an `Exif` APP1 segment. Inspection of
390 levels in a real cache found `DQT`, `DRI`, `APP14`, `SOF0` and `DHT`, and no
`Exif` APP1 at all (**verified**). The reason to parse the declared lengths is
that each level is a complete JPEG with its own segment stream, so a marker scan
can stop at something that is not the level or run into the next chunk — not that
every level has EXIF.

Level size is known twice over: the cache records it in `PyramidLevel`, and the
JPEG's SOF marker states it. `lrsalvage` reads the SOF dimensions from the
declared payload and compares them with `previews.db`'s best-level dimensions,
allowing width and height to be swapped; a payload that is not the size the cache
expected is kept and marked `suspect`, not thrown away. The `header` chunk also
lists dimensions, but the planner does not use that table for the check — the
database row is the authority.

`header::PyramidHeader` parses this Lua-like table and has unit tests, but no
production path calls it: the only non-test use is a parser test in `lrprev.rs`
(**verified** in the source). It is a documented capability with no consumer, and
that is deliberate — `previews.db` is authoritative for dimensions, the profile
name and the mapping (ADR 0002), so a second source of the same facts would only
create disagreement to resolve. Do not describe it as part of the read path.

---

## 2. `previews.db` — VERIFIED

Every `* Previews.lrdata` directory contains a SQLite database, `previews.db`,
which is the authoritative map from catalog image to payload file. It contains
**no image bytes at all** — every column in every table is a scalar (text or
integer). The bytes live beside it as `.lrprev` files.

### 2.1 Full schema

Verbatim `CREATE TABLE` statements. The columns are declared **without types**,
which is legal SQLite (typeless affinity) and is not a transcription error: the
types live in Adobe's application code, not in the schema.

```sql
CREATE TABLE StoreInfo (
    version NOT NULL
);

CREATE TABLE Pyramid (
    uuid NOT NULL,
    digest NOT NULL,
    colorProfile NOT NULL,
    fileTimeStamp,
    quality NOT NULL,
    croppedWidth NOT NULL,
    croppedHeight NOT NULL,
    pyramidFileTimeStamp,
    fingerprint,
    fromProxy
);

CREATE TABLE PyramidLevel (
    uuid NOT NULL,
    digest NOT NULL,
    level NOT NULL,
    longDimension NOT NULL,
    lastAccess NOT NULL,
    width NOT NULL,
    height NOT NULL
);

CREATE TABLE DeletedPyramid (
    uuid NOT NULL,
    digest NOT NULL,
    deletedTimeStamp NOT NULL
);

CREATE TABLE ImageCacheEntry (
    imageId NOT NULL,
    uuid NOT NULL,
    digest NOT NULL,
    orientation
);

CREATE TABLE MotionPreview (
    uuid NOT NULL,
    digest NOT NULL,
    frameCount NOT NULL,
    longSide NOT NULL,
    fileTimeStamp NOT NULL,
    motionPreviewFileTimeStamp
);

CREATE TABLE DeletedMotionPreview (
    uuid NOT NULL,
    digest NOT NULL,
    deletedTimeStamp NOT NULL
);

CREATE TABLE MotionPreviewCacheEntry (
    imageId PRIMARY KEY,
    uuid NOT NULL,
    digest NOT NULL
);

CREATE TABLE FacePreview (
    uuid NOT NULL,
    digest NOT NULL,
    fileTimeStamp NOT NULL,
    facePreviewFileTimeStamp
);

CREATE TABLE DeletedFacePreview (
    uuid NOT NULL,
    digest NOT NULL,
    deletedTimeStamp NOT NULL
);

CREATE TABLE FacePreviewCacheEntry (
    imageId PRIMARY KEY,
    uuid NOT NULL,
    digest NOT NULL
);
```

Eleven tables. Note what the schema does *not* say: `Pyramid`, `PyramidLevel`
and `ImageCacheEntry` declare **no primary key and no uniqueness constraint**, so
duplicate `(uuid, digest)` or duplicate `imageId` rows are possible in principle.
Query accordingly; do not assume one row per key. `MotionPreviewCacheEntry` and
`FacePreviewCacheEntry` are the only tables with a declared `PRIMARY KEY`.

**Settled 2026-11-16 — the modern store is this schema plus `PyramidLevel.fileSize`,
and `StoreInfo.version` did not follow the catalog's (§5.4, §5.10).** That is the
whole shape difference the 13.3 change was expected to bring, on every store
measured here; a 14 or 15 catalog was never inspected, and §5.5 keeps that gap.

### 2.2 Table of tables

| Table | Holds | Notes |
|---|---|---|
| `StoreInfo` | schema version | one row; `13.0` in the modern store (§5.4) — `scripts/probe-cache.sh` prints it, and no decision reads it |
| `Pyramid` | one row per cached pyramid | the unit of a preview set |
| `PyramidLevel` | one row per level of a pyramid | dimensions and long edge |
| `ImageCacheEntry` | image → pyramid | the join that matters (`imageId`) |
| `DeletedPyramid` | tombstones | uuid/digest plus deletion time |
| `MotionPreview` | motion-preview entries (video) | `frameCount`, `longSide` |
| `DeletedMotionPreview` | tombstones | |
| `MotionPreviewCacheEntry` | video image → preview | |
| `FacePreview` | face crops used by the People UI | not a photo preview |
| `DeletedFacePreview` | tombstones | |
| `FacePreviewCacheEntry` | image → face preview | |

`DeletedPyramid` is interesting for salvage: it proves a preview once existed for
a `(uuid, digest)`. The one deletion measured here (§5.11) removes the rows and
their payloads together and consumes tombstones rather than writing them, so a
tombstone is evidence of a deletion event and never of a payload that survived
it. Whether the background purge behaves the same way is **unverified**, and that
is the case that matters: if it did not delete the payload, a deleted row beside
a live orphan payload would be a recoverable image. TODO: test by trashing a
preview in a copy of a catalog.

What Lightroom uses `MotionPreview` and `FacePreview` for is **unverified**; only
their columns were read. Neither holds a photograph of the kind we are salvaging.

### 2.3 The mapping, and where the file lives

`ImageCacheEntry.imageId` is `Adobe_images.id_local` in the `.lrcat`. The pair
`(uuid, digest)` names the payload, and the payload path is:

```
<cache>/<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>.lrprev
```

`uuid[0]` is the first character of the uuid, and the second directory is its
first four characters — the whole uuid prefix, not an offset by one. For the
example file:

```
<cache>/1/1418/1418F22B-41B0-4786-9DF0-D5081559CAD1-bd012a8e0e0b5b73ddaf34aeceb8bf48.lrprev
```

UUID uppercase, digest lowercase, joined by one hyphen, extension `.lrprev`
(**verified**). Case matters on case-sensitive filesystems, so reproduce it
exactly rather than normalising.

`Pyramid.quality` observed values: `standard`, `smallRender`, `final`,
`bigThumbnail`, `thumbnail` (**verified**). Their meanings are not documented;
do not assume `final` means full resolution. `Pyramid.colorProfile` observed
values were only `AdobeRGB` and `sRGB` (**verified** in this catalog set). Those
two are the only names `lrsalvage` can turn into an ICC profile; see section 6.4
for the sizes and for what happens to any other name.

`PyramidLevel.longDimension` is `max(width, height)` for that level, stored
alongside the two dimensions. Both are read from Lightroom's own render, so they
are the dimensions of the payload, not a request.

### 2.4 What is not in `previews.db`

- **No image bytes.** No column carries image data: the schema declares no
  blob-typed column, and every value read was a scalar. A "recovered preview
  from `previews.db`" is a contradiction: the database only tells you where the
  file is and what Lightroom thought of it.
- **No original path.** Paths come from the `.lrcat` (section 6.1); the preview
  DB knows nothing about folders on disk.
- **No checksum of the payload.** `digest` is part of the payload's *name*, not
  a hash we can recompute from the bytes. This is why keys are never re-derived
  (ADR 0002).

### 2.5 `croppedWidth`/`croppedHeight` describe the payload

The names suggest the logical image. They do not: they describe the **stored,
unrotated render** (**verified**).

Evidence: three images whose catalog orientation code is `DA` (270°) have
payloads of 1616 × 1077, the same landscape dimensions as their upright
neighbours, and their `Pyramid` rows carry 1616 × 1077 too. A portrait-orientation
image would be 1077 × 1616 once the rotation is applied; the stored payload is
not rotated, so the row reports the stored shape.

"Cropped" is accurate in a different sense: Lightroom's crop **is** baked into
the render, so the payload dimensions are already post-crop. Crop yes, rotation
no. Never infer rotation from width × height (section 4).

### 2.6 `root-pixels.db`

A sibling database, `root-pixels.db`, holds one tiny JPEG per image. Observed
shape, as a column list rather than the literal DDL — capturing the DDL for a
file nothing reads would be a claim with no consumer:

```text
RootPixels (
    uuid,
    digest,
    colorProfile,
    croppedWidth,
    croppedHeight,
    quality,
    jpegData
)
```

Measured on the 78-payload cache (**verified**): 78 rows, roughly 3.5 KB each,
and **all 101 × 68** — one row per image, one fixed size, far too small to be a
recovery. 78 rows for 78 payloads is consistent with a grid thumbnail shown
before a real preview loads; what Lightroom uses it for is still **unverified**,
and that question is now retired rather than answered, because no answer changes
a decision here. Either way it identifies a frame and never substitutes for the
best level.

Observation, and arithmetic on the two measurements above: 101 × 68 is exactly
the smallest level of the example pyramid in section 1.2. Whether the root pixel
is a copy of `level_1` or an independent render is **not needed** and will not be
tested: both answers leave the same decision, and the difference would be
recorded only to satisfy the question.

**Settled 2026-11-16 — no second database is needed, and this one is closed
rather than deferred.** Every level file of every cache read here answered to a
`PyramidLevel` row, and no row was left without its file (§5.4, §5.9, §5.10,
§5.11). `previews.db` alone has therefore named every level this tool needed, and
`previews_db` opens one index file (`INDEX_FILE`) and no other database. And a
101 px grid image is **not a level**: its row lives in `root-pixels.db` with the
JPEG inline in `jpegData`, so it carries no `PyramidLevel` row in `previews.db`
and no `_<longedge>` leaf in the cache tree — the 101 px levels that do carry
them are the index's, counted in §5.2 and §5.9. It also sits far below the 640 px
where `small` begins (`CONTEXT.md`), so a tier built on it would need its own
naming, metadata and floor decision — a different product, not a gap in this one.
If it is ever revisited it starts from that sentence, not from a `TODO`.

**Unmeasured, and nothing depends on it:** whether a 13.3+ upgrade or a render
rewrites `root-pixels.db`, and to what size. A slice that reads the file measures
that first; until then it is a file this tool knows about and does not need.

### 2.7 Measured statistics

Large catalog, **verified**:

| Metric | Value |
|---|---|
| `ImageCacheEntry` rows | 3,420 |
| `Adobe_images` rows | 3,416 |
| Payloads present on disk | 3,057 (1,090.1 MiB) |
| Payloads missing | 363 |
| Orphan payloads | 3 |
| Motion previews | 3 |

`Pyramid.quality` split (3,422 rows total): `standard` 2,716; `smallRender` 423;
`final` 237; `bigThumbnail` 25; `thumbnail` 21.

`Pyramid.colorProfile` split: `AdobeRGB` 3,373; `sRGB` 49.

Best-level long edge, across images: 1,440 × 960 → 2,696 images; 5,760 × 3,840 →
53; 360 × 240 → 311; 160 × 106 → 17.

### 2.8 Reconciliation — the numbers do not all agree

Recorded rather than explained, because guessing here would be exactly the kind
of invented claim this file exists to prevent.

| Identity | Result |
|---|---|
| present + missing = `ImageCacheEntry` rows | 3,057 + 363 = 3,420 ✓ exact |
| `Pyramid` rows vs `ImageCacheEntry` rows | 3,422 vs 3,420 — 2 more pyramids than entries (TODO) |

Since `adr-0008-a-render-companion-is-not-an-orphan`, an unclaimed pyramid's row
facts reach its report row (`quality`, `color_profile`), so whatever those two
stray rows say is now readable from `report.csv` instead of requiring a query. A
`thumbnail`-quality stray is still reported as an orphan here: the companion rule
also needs a claimed pyramid on the same uuid, and this store holds none for them.
| best-level buckets vs payloads present | 3,077 vs 3,057 — 20 more images have level dimensions than have files (TODO) |
| orphan payloads vs motion previews | both 3; whether the same objects is unchecked (TODO) |

The first row is the useful one: every cache entry is accounted for as either a
file we found or a file that is gone. The other gaps are small enough to be
deleted records, smart previews, or stacked virtual copies, and none of them
changes a recovery decision. They do mean `Pyramid` and `ImageCacheEntry` should
be joined, never assumed 1:1.

---

## 3. `.lrcat-data` is AI masks, not previews — VERIFIED

This section appears to be undocumented publicly. It matters because a directory
sitting next to the catalog, full of image-like files, is a plausible-looking
place to hunt for previews — and it is the wrong place. **No preview is
recoverable from `.lrcat-data`.**

### 3.1 What these files are

Layout: `<first char>/<next 4 chars>/<32-hex-uppercase>.dat`.

The 32-hex file name equals the `MaskDigest = "<hex>"` value inside the develop
settings blob stored in `Adobe_imageDevelopSettings` (**verified**). That is the
identification: these are the **alpha channels of AI masks** — Select Subject,
Select Sky, and friends. One file per mask, not per image.

TODO: the exact shard split. Section 2.3's preview sharding uses `uuid[0]` then
`uuid[0..4]`; "first char, next four chars" can be read as characters 0 and 1–5.
No full `.lrcat-data` path was recorded during this pass, so both readings remain
open. Do not rely on either until a tree is walked and logged.

### 3.2 Byte layout

A 16-byte prefix, then a little-endian TIFF whose IFD starts at offset 8 relative
to the TIFF header:

| Offset | Size | Contents |
|---|---|---|
| 0x00 | 4 | `02 00 00 00` |
| 0x04 | 4 | `01 00 00 00` |
| 0x08 | 8 | `00 00 00 00 00 00 00 00` |
| 0x10 | 4 | TIFF magic `II*\0` (`49 49 2A 00`) |
| 0x14 | 4 | first IFD offset, `08 00 00 00` (= 8, relative to 0x10) |
| 0x18 | … | IFD entries, then strip data |

The meaning of the two leading `u32`s (`2`, `1`) is **unverified**; they may be a
version and a mask kind. Do not interpret them.

### 3.3 TIFF tags seen

Tag numbers are from the TIFF 6.0 specification; the values are observed
(**verified**).

| Tag | Name | Observed value |
|---|---|---|
| 254 | NewSubfileType | 0 |
| 256 | ImageWidth | e.g. 2880 |
| 257 | ImageLength | e.g. 741 |
| 258 | BitsPerSample | 8 |
| 259 | Compression | 8 (Adobe Deflate) |
| 262 | PhotometricInterpretation | 1 (BlackIsZero, greyscale) |
| 273 | StripOffsets | array, one per strip |
| 277 | SamplesPerPixel | 1 |
| 278 | RowsPerStrip | 48 or 96 (both seen) |
| 279 | StripByteCounts | array, one per strip |
| 284 | PlanarConfiguration | 1 |
| 317 | Predictor | 2 (horizontal differencing) |

Greyscale, one sample, 8 bits, zlib per strip. That is a weight map, not a
photograph.

### 3.4 The 16-byte rule

**`StripOffsets` are relative to the start of the TIFF header, i.e. add 16.**

This is the detail that costs an afternoon. TIFF normally expresses strip offsets
relative to the start of the TIFF header, and here the TIFF header is not at
offset 0 of the file. Read them as file-absolute and every strip lands 16 bytes
early, which corrupts the first bytes of the zlib stream; read them as TIFF-base
relative and the inflated byte count is exactly `width × rows`.

Worked example (arithmetic on the verified rule, not a separately measured
offset): the TIFF header starts at file offset `0x10`, so a `StripOffsets` entry
of `0x00F2` means the strip starts at file offset `0x0102`. Reading at `0x00F2`
would land 16 bytes early, in whatever precedes the strip.

Despite the "Adobe Deflate" compression tag, the strip payload is a plain **zlib**
stream (inflate, not raw deflate) (**verified**).

### 3.5 Decoding

```
base = 16                          # file offset of "II*\0"
for each strip s in 0..strip_count:
    off  = base + strip_offsets[s]
    raw  = zlib_inflate(file[off .. off + strip_byte_counts[s]])
    rows = rows_in_strip(s)        # last strip is short
    for r in 0..rows:
        acc = 0
        for i in 0..width:
            acc = (acc + raw[r*width + i]) & 0xFF
            out[r*width + i] = acc
```

Predictor 2 (horizontal differencing) is undone per row: each byte is the
difference from the byte to its left, with the accumulator reset to 0 at the
start of every row **and every strip**. Because there is one sample per pixel,
"per row of `width` bytes" is the whole rule; no channel interleaving.

### 3.6 Decoded example

2880 × 741, 8-bit, 2,134,080 pixels (**verified**): 1.33 M at 255, 0.57 M at 0,
and the remainder in between. That is roughly 62% fully selected, 27% fully
unselected, 11% soft edge — the histogram of a selection mask, and nothing like
the histogram of a photograph (derived from the counts above).

### 3.7 Consequence

`.lrcat-data` contains **no previews**. Its files are mask alpha channels: one
per AI mask, greyscale, often a handful of megapixels, frequently mostly black.
A tool that treats them as recoverable images produces a folder of grey
rectangles. `lrsalvage masks` decodes them to PNG on request; `scan` and
`extract` ignore the directory entirely, because nothing in it is a preview.

---

## 4. Orientation — VERIFIED mapping

Previews are stored **unrotated**. The catalog carries the rotation as a
two-letter code; the pixels never do. The mapping, verified against catalog rows
and payload dimensions:

| Catalog code | Clockwise rotation to upright | EXIF `Orientation` |
|---|---|---|
| `AB` | 0° | 1 |
| `BC` | 90° | 6 |
| `CD` | 180° | 3 |
| `DA` | 270° | 8 |

The four codes are consecutive letter pairs (`AB`, `BC`, `CD`, `DA`). Why the
encoding takes that form is **unverified**; the mapping is not. The code appears
in `Adobe_images.orientation` and, when non-null, in
`ImageCacheEntry.orientation`. Which one wins when they disagree is **TODO**;
`ImageCacheEntry.orientation` is nullable, so `Adobe_images` has to be the
fallback at minimum.

Never infer rotation from width × height. Three `DA` images have payloads of
1616 × 1077 — landscape dimensions — because the stored render is unrotated
(section 2.5). The dimensions say nothing about how the picture should be
displayed.

Mirrored orientations (EXIF 2, 4, 5, 7) were not observed in these catalogs
(**TODO**). Treat any code outside the four as unknown and leave the payload
alone rather than guessing.

The payload's own EXIF block may carry an `Orientation` tag, but it describes the
render as stored; the catalog is the authority. When writing a recovered file,
the orientation goes into the output's EXIF so viewers that honour it show the
picture upright without touching a pixel (ADR 0003).

---

## 5. Lightroom 13.3 and later — VERIFIED for an upgrade and for a render

> **Implementation status:** built, and run end to end against this cache
> (section 5.9). The modern layout is read. A level file is derived from its
> `PyramidLevel` row, a level the index forgot is found by widening inside the
> pyramid's own `(uuid, digest)` and never beyond it, the chosen level is
> certified by its own bytes, and the cache's `layout` is reported as one of
> five words. A cache with no readable `previews.db` degrades: every image whose
> original is not on disk is `no-cache-entry`, every payload pyramid is listed as
> `unmapped`, and the run exits 3 with one hint. `scan --report` writes the
> path-free verification artifact a stranger can hand over, and `extract` writes
> one JPEG per image with the same metadata treatment legacy payloads get.
> Delivery: [issue #2](https://github.com/marcd35/lrsalvage/issues/2).

Lightroom Classic 13.3 changed preview storage. Sections 5.1–5.4, 5.7 and 5.9 are
**verified** on an upgraded catalog, 5.10 on three caches rendered from scratch
and 5.11 on a copy of one of them that Lightroom discarded; sections 5.5 and 5.8
say which of their claims the renders settled and which they did not, which is
not the same as untested software. The tool must still detect the layout rather
than assume either one.

### 5.1 What was measured

The 78-image catalog (section 0) opened by Lightroom Classic **16.0**
(`ProductVersion 16.0 (202611041403-7ac14c66)`) on 2026-11-14 and upgraded **in
place**:

| Observation | Value |
|---|---|
| `Adobe_DBVersion`, before → after | `1200001` → `1504001` |
| `Adobe_CrsVersion`, before → after | `15.4.0` → `18.0.0` |
| `.lrprev` files remaining | **0** (every legacy payload was converted) |
| Payload files after | 390 = 78 images × 5 levels |

The catalog version is not the marketing major: a catalog written by Lightroom
16.0 reports `1504001`. Do not derive a version gate from these numbers.

### 5.2 Path and names — VERIFIED

```
<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>_<longedge>
1/1418/1418F22B-41B0-4786-9DF0-D5081559CAD1-bd012a8e0e0b5b73ddaf34aeceb8bf48_1616
```

- **The sharding is the legacy sharding.** `<uuid[0]>` then `<uuid[0..4]>`, the
  same rule `previews_db::payload_path` already implements for `.lrprev`. The
  leaf directory is a per-image folder, but it is named by the first four
  characters of the uuid, **not the whole uuid**: any rule that matches the
  parent directory against a full uuid will find nothing.
- The leaf is the legacy leaf minus the `.lrprev` extension, plus `_<longedge>`.
- `_<longedge>` equals `PyramidLevel.longDimension` **and** the JPEG's measured
  long edge (`_101` is 101 × 68, `_1616` is 1616 × 1077), so the name is
  derivable from the index rather than discovered by globbing.
- Level set in this cache: `101, 202, 404, 808, 1616` — the same doubling series
  as the legacy container it was converted from, not a fresh standard set. A
  render does not repeat it (§5.10): with Standard Preview Size `Auto (3840px)`
  one wrote `320, 960, 1920, 3840`, and at `1024 pixels` it wrote `320, 1024`. The
  ladder belongs to the container the conversion split, not to Adobe's renderer.
- **All 78 pyramids share one digest** (`bd012a8e0e0b5b73ddaf34aeceb8bf48`). In
  this store the digest is store-wide, so it cannot identify a payload; the uuid
  is the only discriminator. A rule that globs on the digest prefix would match
  every image. That is a property of this *conversion*, not of the format
  (§5.10): a rendered store held four digests over twenty pyramids.

### 5.3 Content — VERIFIED

- Each file is a complete baseline JPEG, `FF D8 FF DB` at offset 0, with SOF
  dimensions matching its suffix.
- The largest level of the image checked (1616 × 1077) carries **no `Exif`, no
  XMP packet and no `ICC_PROFILE`**: the bytes are a bare level payload.
- The upgrade **split the legacy container's levels into files; it did not
  re-render them.** The image in section 1.2 now exists as five files of
  2,791 + 7,536 + 25,989 + 89,230 + 302,569 bytes, plus the container's 447-byte
  `header` chunk that makes the documented 428,562-byte payload total. Sizes are
  verified; byte-for-byte identity is inferred from them, not hashed.

### 5.4 `previews.db` — VERIFIED

- The modern store keeps every legacy table. The only schema addition is
  **`PyramidLevel.fileSize`** (the 13.2 schema in section 2.1 has no such column).
- `StoreInfo.version` is `13.0` — so the store format version did **not** follow
  the catalog to 15/16.
- Counts: `Pyramid` 78, `PyramidLevel` 390, `ImageCacheEntry` 78,
  `DeletedPyramid` 0. Every row maps to an existing file: no stale rows and no
  orphan payloads in this cache.
- `PyramidLevel.fileSize` equals the on-disk byte count. Measured across all
  390 rows on 2026-11-15: every row's `fileSize` matched its file exactly
  (delta 0, 390 of 390), and the `1418F22B…` pyramid's level 5 is `302569`
  declared and `302569` on disk. An earlier revision of this note paired
  `311,930`, the `fileSize` of a *different* pyramid in the same cache, with
  that file and concluded the column was unusable. The pairing was wrong, not
  the column. It is still a declaration rather than a measurement: a truncated or
  pruned file can disagree with it, and no such state was measured (§5.5).
- **Settled 2026-11-16: that is the whole difference between the 13.2 and the
  13.3+ store.** One added column and an unchanged `StoreInfo.version`, on the
  upgrade and again on all three renders (§5.10), so §2.1's schema stands with
  that column added and §2.2 points here instead of asking. A 14 or 15 catalog was
  never inspected, and nothing is claimed about one (§5.5).

### 5.5 Reported elsewhere, and what remains unmeasured

| Claim | Source |
|---|---|
| "Changes have been made to how image previews are stored in version 13.3." | Adobe, catalog FAQ |
| Long-edge series observed elsewhere: 90 / 180 / 360 / 720 / 1440 / 2880, or 105 … 3360, or 320 … 8192. | Ian Lyons, July 2024 |
| `previews.db` rows can be stale after an upgrade. | Ian Lyons, September 2024; upgrade threads |
| Extracted filenames now carry the long edge (`DSC06787_6000.jpg`) whereas 13.2 and earlier carried both dimensions (`_MG_0233-360x240.jpg`). | Adobe, "Extract preview for lost images" |
| Extracted previews contain **no ICC profile**, so re-importing them assumes sRGB. This describes Adobe's own extractor; `lrsalvage` rebuilds a profile from the catalog's name (section 6.4). | Adobe, "Extract preview for lost images" |
| "It seems that Lightroom totally changed how they store the preview-cache data, around Lr13.3." | regex.info changelog 20240611.65 |
| Lightroom 14.0 limits preview-cache size and purges in the background; 14.1 can discard standard/1:1 previews. | Ian Lyons, 2024–2025 |

**Measured, 2026-11-15 — the level set a render chooses** (§5.10): it is not a
fixed ladder, and it is not the conversion's. Standard Preview Size `Auto
(3840px)` produced `320, 960, 1920, 3840`; `1024 pixels` produced `320, 1024`;
and a 1:1 build produced a doubling ladder ending at each original's own long
edge. So the setting drives the series and the conversion's `101…1616` was the
legacy container's bytes, not Adobe's renderer. None of the published series
above (90…2880, 105…3360, 320…8192) is what this build wrote; the last is the
closest and still not equal.

**Not measured at all:** a cache with pruned levels or stale rows; a cache holding
both `.lrprev` and modern payloads (this upgrade converted everything in place,
so coexistence was not observed); any conversion from a catalog newer than
`1200001`; whether a render *rewrites* a pyramid it already holds — every render
measured here was of an image the catalog had never seen; and the payload shape
of `MotionPreview` and `FacePreview`, which the index holds here with no rows at
all, so one carrying a payload is counted as an unmapped item on a shape nobody
has measured.

Every state above that a fixture *can* construct has a row in
`specs/fixtures.md`, and each row carries the label that says what it is worth:
`unmeasured` for the five states §5.10 never produced in the field, `verified
§5.10` where a render did produce it, and `synthetic` where the row is a variant
of a measured shape rather than the shape itself. The label matters because a
fixture proves how the reader behaves in a state and never that Adobe produces
it:

| Unmeasured state | Row that pins the reader | What the row cannot show |
|---|---|---|
| Pruned level: the row's file is gone, a smaller level survives | `M3` | that a Lightroom upgrade prunes this way — the discard measured in §5.11 leaves no stale row either |
| Level file with no row (found by the bounded glob) | `M4` | that the index ever forgets a row |
| Both representations for one pyramid (`superseded`) | `X1` | that this upgrade leaves a `.lrprev` behind |
| Coexistence, resolved per pyramid rather than per cache | `X2` | that Adobe produces a mixed cache at all |
| A freshly built render's payload metadata | `M13` (§5.8) | nothing: §5.10 measured it, and every rendered level carried `Exif`, XMP and an ICC profile |
| An unclaimed pyramid beside a claimed one | `M10c`…`M10e` (§5.10) | nothing: every fresh render left one thumbnail pyramid that no cache entry claims, and #42/ADR 0008 decided what it is |
| An image whose whole pyramid is gone while its companion survives | `M10f` (ADR 0009) | that a prune reaches the whole pyramid and leaves the companion behind — no cache on this machine has been caught in that state, and the one deletion that could be triggered here took the companion with it (§5.11) |

So the software's behaviour in those states is tested; the claim that Lightroom
produces them is not — except for the payload metadata and the unclaimed
pyramid, which §5.10 measured in a render. One state has no row at all: a
conversion from a catalog newer than `1200001`, which is a fact about Adobe's
upgrader rather than about the reader.

**Attempted 2026-11-16, and the state did not appear.** The whole-pyramid row was
chased with the one deletion this build can trigger, `Library > Previews >
Discard Standard and 1:1 Previews`, on a copy of the §5.10 cache (§5.11). It
removes the standard-size level and the image's `thumbnail` pyramid, keeps the
levels below the standard size, and leaves the index agreeing with the files. So
no deletion measured here produces the rescue's state, and the background purge
is reported to exempt thumbnails and sub-standard levels as well: `M10f` keeps
`unmeasured`, and ADR 0009 records what that means for the rescue.

### 5.6 Consequences

- A stale `previews.db` **level** row whose file is gone is a **pruned level**,
  not a missing payload: the item recovers the largest level that exists
  (ADR 0005 §Decision 5), and the note says the row is stale. The two are only
  confusable once a whole pyramid has no file at all. Then a pruned pyramid and
  one whose files the user deleted are indistinguishable from the index: the row
  is the only claim and the file is absent either way. A `DeletedPyramid`
  tombstone records that Lightroom deleted the *row*, not the payload (§2 marks
  the payload question unverified), so the report may quote it and must not infer
  a cause from it. Say what was seen; do not claim corruption (ADR 0006).
- `.lrprev` parsing legitimately finds nothing in a modern cache. That is a
  layout detection result, not an empty catalog, and must be reported as such.
- The payload path rule generalises: keep the two shard levels, change the leaf
  and drop the extension. Discovery can be index-driven (derive the leaf from
  `PyramidLevel`), with a glob on `<uuid>-<digest>_*` as the fallback when rows
  are stale. Decided in `adr-0005-discover-payloads-by-index`.

Why the 1440 number matters: 1440 is the traditional default standard-preview
long edge, and the measured 1,440 × 960 group (2,696 images) is the bulk of a real
cache (section 2.7). A catalog that never had 1:1 previews built has a hard
ceiling of about 1440 px, no matter what tool reads it.

### 5.7 What a level file says about itself — VERIFIED

All 390 payloads of the upgraded cache were read (every file, not a sample) on
2026-11-14. Every count below is 390 of 390 unless it says otherwise.

- **The path is shard-consistent.** `<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>_<longedge>`
  held for all 390, so a uuid can be checked against its own address.
- **The leaf grammar is fixed-width**: 36-character uuid, `-`, 32-character
  lowercase hex digest, `_`, digits — 69 characters before the underscore.
  Splitting the leaf on `-` splits the uuid itself; the digest is the 32
  characters after position 36.
- **The top-level segment chain is short and stable**: `SOI`, `DQT`, `DRI`,
  `APP14` (Adobe), `SOF0`, `DHT`, `SOS`. No file carries `APP1`, an XMP packet, or
  `ICC_PROFILE`.
- **`SOI` at offset 0 and `FF D9` exactly at EOF**, with no trailing padding — so
  completeness can be required of a payload that has no length field.
- **`_<longedge>` equals the top-level `SOF` long edge** in all 390, confirming
  section 5.2 across the whole cache instead of one image.
- **Level rows are REAL and unordered.** `longDimension`, `level`, `width` and
  `height` read back as `101.0`, and a pyramid's rows are not in level order. A
  suffix derived from `longDimension` must be the rounded integer.
- **One digest for the whole store** (section 5.2): the digest identifies the
  store, not the image, so the uuid discriminates and the pair is what a row maps.
  **Corrected, 2026-11-15 (§5.10):** a *converted* store held one digest; a
  *rendered* one held four over twenty pyramids — a shared digest per pyramid
  class (`standard`, `thumbnail`) with occasional per-image values. Nothing in
  the reader may assume either, which is why the address is always the row's own
  `(uuid, digest)` pair.
- **A four-character shard is not a unique address.** All 78 uuids here have a
  distinct `<uuid[0..4]>`, but among the large catalog's 3,416 pyramids roughly
  89 colliding pairs are expected (arithmetic, not measurement). A folder can
  therefore hold levels of two pyramids; only the full uuid in the leaf separates
  them.
- **Why the marker walk is safe.** An embedded EXIF thumbnail sits inside APP1's
  declared payload, so a walk of the top-level chain never reaches its `SOF`.
  This is a property of the layout, not something observed in situ: no payload in
  this cache carries an `APP1` segment at all.
- **The reader reproduces every fact above — verified, 2026-11-15.** A `scan`
  with the build from this tree reports `shapes` of `level_file 390` and
  `suffix_matches_sof 390`: §5.7's 390-of-390 measurement read back through the
  tool, because each leaf's `_<n>` matched the long edge its own `SOF` declared,
  on every file in the cache. The histogram is a measurement, not a second
  reading of the name. §5.9 records that run in full.
- **The one shape this section does not cover.** `MotionPreview` and
  `FacePreview` (with their `Deleted*` and `CacheEntry` siblings) exist in this
  store and hold 0 rows, and no video or face-preview cache has been inspected,
  so their modern payload shape is no part of what was read here. Until it is
  measured, one that has the payload shape is counted as an unmapped item —
  section 5.5 records the state as unmeasured.

### 5.8 Payloads that carry their own metadata — VERIFIED for a render

- **Verified, 2026-11-15 (§5.10):** a level file a render wrote is
  `SOI`, `APP1` (`Exif`), `APP13`, `APP2` (`ICC_PROFILE`), `APP1` (XMP), `DQT`,
  `DRI`, `APP14`, `SOF0`, `DHT`, `SOS` — four metadata segments the converted
  levels do not have, before the same band headers. 38 of the 48 level files of
  the `Auto (3840px)` cache carry all four, 20 of 30 at `1024 pixels` and 40 of 40
  in the 1:1 cache; the rest are the thumbnail pyramids of §5.10, which are bare.
- **Verified: what the `Exif` carries is nearly nothing.** Its payload is 114
  bytes: `IFD0` with `XResolution 300/1`, `YResolution 300/1`, `ResolutionUnit 2`
  and an Exif IFD pointer; that IFD with `ExifVersion "0231"` and `ColorSpace`
  65535 (uncalibrated). **No `Orientation` tag, no make, model, date or pixel
  dimensions.** The 42-byte `APP13` is `Photoshop 3.0` with one `8BIM` resource,
  `0x03ED` resolution info.
- **Verified: the `ICC_PROFILE` is a real profile, not a name.** A 574-byte
  segment carrying a 560-byte Adobe RGB (1998) profile — ten tags, D65 white
  point — in every metadata-bearing level. It matches the `Pyramid.colorProfile`
  name `AdobeRGB` those rows hold, including for the three untagged JPEGs
  imported beside them, so a fresh catalog of this build assumes Adobe RGB (1998)
  rather than sRGB. The thumbnail pyramids are `sRGB` and carry no segment.
- **Verified: the XMP packet is an identity stamp and nothing else.** 693 bytes:
  `xmpMM:OriginalDocumentID`, `xmpMM:DocumentID`, `xmpMM:InstanceID` and
  `xmp:MetadataDate`. No `xmp:Rating`, no label, caption or keyword, and no `crs:`
  develop settings — a render does **not** carry the catalog's facts with it. The
  rating set in Lightroom before the 1:1 build (§5.10) appeared nowhere in the
  payloads.
- **Verified: pixels are stored unrotated and the rotation is not in the
  payload.** Two sources whose `Exif` `Orientation` was 8 and 6 (pixels 4096×2731,
  displayed 2731×4096) rendered as 3840×2560 levels — the unrotated matrix —
  while the catalog reported `DA` and `BC`. The payload's own `Exif` has no
  orientation tag, so the leaf's dimensions and the catalog's orientation are the
  only two records of it. This settles the question the reader was built around:
  a payload cannot be trusted to say how it should be turned, and the catalog
  must.
- **Verified, and why the case needs a rule at all:** a payload's own metadata is
  only observable by walking its segments. No table records it — `Pyramid` keeps
  a profile *name* and no bytes (§6.4) — so an inventory read from the file is the
  only evidence available, which is the same walk ADR 0005 already requires for
  certification.
- **Decided, and now exercised in the field:** the precedence order of
  `adr-0003-preserve-preview-bytes`. Every one of §5.10's ten extracted images
  reported `payload_metadata = exif,xmp,icc`, and the output kept the payload's
  ICC profile **byte for byte** (560 bytes, identical digest) and its XMP packet
  unchanged at 693 bytes, while replacing its 114-byte `Exif` with ours (326
  bytes) and saying so in the row's note. The decision written against a
  synthetic shape (`M13` of `specs/fixtures.md`) is what the field state does.
- **Verified, still:** no payload in the converted cache carries metadata
  (§5.9): `payload_metadata` was blank in all 78 items and the 390 legacy levels
  hold no `APP1` and no `APP2`. The metadata-bearing state is what a render
  produces, not what an upgrade preserves — so a cache of either provenance can
  reach `extract`, and the rule above has to hold for both.

### 5.9 The build from this tree, run against this cache — VERIFIED

On 2026-11-15 the binary built from this tree (`0.1.0`) was run against the
upgraded catalog and its cache, read-only, with every write going to a scratch
root outside both trees. No path appears below: the numbers are the runs' own
output and the `--report` file, which is path-free by construction.

| Run | Exit | What it reported |
|---|---|---|
| `scan` | 0 | `cache layout: modern`, `catalog images 78`, `recovered 78 25.1 MiB`, `standard 78` |
| `scan --json` | 0 | `layout modern`, `db_version 1504001`, `crs_version 18.0.0`, `images 78`, `recovered 78`, `bytes 26330281`, `tiers {standard: 78}`, `shapes {level_file: 390, suffix_matches_sof: 390, container: 0}`, and `0` for every other count — `suspect`, `missing_payload`, `no_cache_entry`, `unmapped`, `failed`, `original_present`, `below_floor`, `superseded`, `deleted`, `unrecognised` |
| `scan --report <file>` | 0 | `schema lrsalvage.verification`, `version 0.1.0`, `layout modern`, both version strings, `options {min_long_edge: 640, all_levels: false, include_present: false}`, the same stats plus `levels 78`, `selected_images 78`, `selected_items 78`, `shapes.level_file` as `{count: 390, suffix_matches_sof: 390, template: "<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>_<longedge>"}`, and `shapes.container` as `{count: 0}` with no template |
| `extract --out <root>` | 0 | `78 evaluated, 78 written, 0 skipped, 0 failed, 25.3 MiB` |
| the same `extract` again | 0 | `78 evaluated, 0 written, 78 skipped, 0 failed, 0.0 MiB` — resume proves each output ours by its `Software` marker, completeness and `SOF`, and writes no preview; its three report files are re-emitted as `(2)` copies |
| `extract --all-levels --out <second root>` | 0 | `recovered 78 32.6 MiB`, tiers `standard 78`, `small 78`, `thumbnail 234`, `below floor (640px) 234`, `156 evaluated, 156 written, 0 skipped, 0 failed, 33.0 MiB` |

What those numbers pinned, beyond the sections above:

- **One file per image, and it is the largest level.** All 78 items are
  `standard` at 1616 × 1077, the pyramid's own largest level. `--all-levels`
  lists all 390 level rows — five per image, at 101 / 202 / 404 / 808 / 1616 —
  and selects the 156 at or above the 640-pixel floor.
- **The compressed data is copied byte for byte.** In 78 of 78 outputs the
  payload's bytes after `SOI` appear contiguously at a fixed offset and run to
  exactly EOF at the output's last byte. The carried bytes sum to 26,330,281,
  which is the `bytes` stat both `scan` surfaces report. That is ADR 0003 on the
  modern layout measured rather than inferred.
- **Only metadata segments are added, and always the same three.** Every output
  is its payload plus exactly 2,753 bytes of segments: a 330-byte `Exif` APP1, a
  321-byte rating XMP APP1, and a 2,102-byte `ICC_PROFILE` APP2 whose length
  field is 2,100 and whose payload is the 2,084-byte `sRGB` profile rebuilt from
  `Pyramid.colorProfile` (§6.4). The level file carries none of its own, so
  `payload_metadata` is empty everywhere and the rebuild is the one that lands.
- **The catalog half of the metadata arrives.** 78 of 78 rows carry a capture
  time, a rating (`0`, written as EXIF `0x4746`), an orientation (`AB` 75, `DA`
  3) and `color_profile sRGB`, and all 78 outputs carry the `lrsalvage 0.1.0`
  marker.
- **`scan` wrote nothing** on the runs that named no file, and every file it did
  write — the verification report — obeys the destination rule.

Still §5.5's business: a pruned level, a stale row, and a cache holding both
layouts. The fixture matrix pins the reader's behaviour in those states
synthetically; no run here can claim Adobe produces them. A freshly rendered
cache is no longer on that list — §5.10 measured three.

### 5.10 A fresh render, measured — VERIFIED

Built and rendered on 2026-11-15 by the same Lightroom Classic 16.0
(`ProductVersion 16.0 (202611041403-7ac14c66)`) that upgraded the cache above.
Three catalogs were created from scratch and each imported the same ten local
files — five camera JPEGs (4096 px wide), one `.ARW`, and four generated JPEGs
carrying `Exif` `Orientation` 8, 6, none (a 1200×800) and none (a 6000×4000) — so
every number below is about the renderer rather than about the sources. No paths
appear here; the runs' own output is the source of each figure.

| Catalog | Standard Preview Size | Preview Quality | What was built |
|---|---|---|---|
| A | `Auto (3840px)` | `Medium` | standard previews at import |
| B | `1024 pixels` | `Medium` | standard previews at import |
| C | `Auto (3840px)` | `Medium` | standard at import, then five stars on all ten, then `Library > Previews > Build 1:1 Previews` |

`Auto (3840px)` and `Medium` are what a new catalog holds: the dialog offers
`Auto`, `1024`, `1440`, `1680`, `2048`, `2880` and nothing else, so the
deliberately-odd size this intended to use (2560) does not exist. Both catalogs'
catalogs report `Adobe_DBVersion 1504001` and `Adobe_CrsVersion 18.0.0` — the same
strings the upgraded catalog holds — and every `previews.db` holds
`StoreInfo.version 13.0`.

**What every level file is** (walked by declared segment lengths, every file):

| | A | B | C |
|---|---|---|---|
| level files | 48 | 30 | 40 |
| `SOI` at 0, `FF D9` at EOF | 48 | 30 | 40 |
| walk reached `SOS` by declared lengths | 48 | 30 | 40 |
| `_<longedge>` equals its own `SOF` long edge | 48 | 30 | 40 |
| `PyramidLevel.fileSize` equals the on-disk bytes | 48 | 30 | 40 |
| rows without a file / files without a row | 0 / 0 | 0 / 0 | 0 / 0 |
| carrying `APP1`+`APP13`+`APP2`+`APP1` (§5.8) | 38 | 20 | 40 |
| bare (`SOI DQT DRI APP14 SOF0 DHT SOS`) | 10 | 10 | 0 |

**The series follows the setting, and is not the conversion's.** Long edges, one
line per image, as files (the rows agree in every case):

- A: `320, 960, 1920, 3840` — a 320 thumbnail plus three doubling steps to the
  `Auto (3840px)` ceiling, for the 4096-wide images; the small 1200×800 image
  rendered `320, 1200`, and one image's thumbnail pyramid held `160` instead of
  `320`.
- B: `320, 1024` — the same 320 floor, the setting as the ceiling.
- C: `320` plus a doubling ladder ending at each original's own long edge —
  `1024, 2048, 4096` for the seven 4096-wide images and `750, 1500, 3000, 6000`
  for the two 6000-wide ones.

So `101, 202, 404, 808, 1616` is the legacy container's ladder rather than a
standard series (§5.2), and none of the published series of §5.5 is what this
build wrote.

**A standard render puts two pyramids in the store for one image, and the 1:1
build puts one.** `Pyramid` rows per cache: A 20, B 20, C 10, against 10
`ImageCacheEntry` rows in each. The pair is a preview pyramid and a **thumbnail**
pyramid, the two told apart by `Pyramid.quality`:

| `Pyramid.quality` | A | B | C | what it is |
|---|---|---|---|---|
| `standard` | 9 | 9 | 0 | the levels above 320; `colorProfile AdobeRGB` |
| `final` | 1 | 1 | 10 | the 1200×800 image, whose largest level is its own size (A, B), and all ten of C's |
| `thumbnail` | 10 | 10 | 0 | a single 320 (or 160) level; `colorProfile sRGB`, payload bare |

- **No `ImageCacheEntry` claims the thumbnail pyramid.** Every image maps to its
  preview pyramid, so all ten thumbnail pyramids are payloads no image names.
  `0.1.0` reported them as `unmapped payloads 10` on A; since
  `adr-0008-a-render-companion-is-not-an-orphan` they are counted as
  `thumbnail pyramids` and `unmapped` stays `0` (§5.9's shape vocabulary is
  unchanged; this is a state, not a bug in the reader's vocabulary).
- **Digests are per pyramid class here, and neither one per image nor one per
  store** (§5.2, §5.7): A held four over twenty pyramids — nine preview pyramids
  shared one digest, nine thumbnail pyramids another, and two pyramids had their
  own. C held two over ten.
- **A 1:1 build replaces the standard pyramid** rather than adding to it: C's ten
  rows are all `final` and `AdobeRGB`, with no `standard` or `thumbnail` row and
  no bare level file, so the standard-sized levels are gone once a 1:1 exists.

**The build from this tree, run against A** (`0.1.0`, read-only, every write under
a scratch root, no path recorded here):

| Run | Exit | What it reported |
|---|---|---|
| `scan` | 3 | `cache layout: modern`, `catalog images 10`, `recovered 0`, `original present 10`, `unmapped payloads 10` — nothing to do, because every original is on disk |
| `scan --include-present` | 0 | `recovered 10 8.9 MiB`, `large 9`, `standard 1`, `unmapped payloads 10` |
| `scan --json` | 0 | `layout modern`, `db_version 1504001`, `crs_version 18.0.0`, `images 10`, `recovered 10`, `bytes 9330084`, `tiers {large: 9, standard: 1}`, `shapes {level_file: 48, suffix_matches_sof: 48, container: 0}`, `unmapped 10`, and `0` for `suspect`, `missing_payload`, `no_cache_entry`, `failed`, `superseded`, `deleted`, `unrecognised`, `below_floor` |
| `scan --report` | 0 | `schema lrsalvage.verification`, `layout modern`, both version strings, `options {min_long_edge: 640, all_levels: false, include_present: true}`, `levels 10`, `selected_images 10`, `selected_items 10`, the same shape counts with the leaf template |
| `extract --include-present` | 0 | `10 evaluated, 10 written, 0 skipped, 0 failed, 8.9 MiB`; `report.csv` carried `payload_metadata = exif,xmp,icc` on all ten rows and `color_profile AdobeRGB` |

Two things that run pinned beyond §5.7: the payload's own ICC profile survived
`extract` **byte for byte** (560 bytes, identical SHA-256 in payload and output)
and its XMP packet was untouched at 693 bytes, while our `Exif` APP1 replaced the
payload's — `adr-0003-preserve-preview-bytes` observed rather than asserted.
`PyramidLevel.fileSize` also agreed with the disk in 48 of 48, a second cache
confirming §5.4.

**What the rule makes those runs report — added 2026-11-15.** The counts above
are what `0.1.0` printed, and they stay as printed. With
`adr-0008-a-render-companion-is-not-an-orphan`, A's ten thumbnail pyramids are a
cache fact rather than orphans: the same runs print `unmapped 0` and
`thumbnail pyramids 10`, `scan --json` carries `thumbnail_pyramids: 10`, and the
ten pyramid files still count in `shapes.level_file 48`. This is the rule's
consequence pinned by fixtures `M10c`–`M10e`, not a re-run of the three caches.
The rendered-cache gate `scripts/check.sh real-render` asserts it against the
`Auto (3840px)` cache itself when this machine holds it. `real-1to1` asserts C's
column above against the 1:1 build — ten images with `recovered 10`,
`thumbnail_pyramids 0` and `unmapped 0`, forty metadata-bearing level files, and
no `standard` or `thumbnail` row (`crates/lrsalvage-cli/tests/one_to_one.rs`).

**What this does not measure.** A render of an image the catalog already holds (a
re-render, or a preview rebuilt after pruning), what `14.0+` does when it prunes
in the background, and any catalog written by a release newer than this one. The
deletion a user *can* trigger is measured separately (§5.11), and it is a
different mechanism from the background purge.

### 5.11 A discard, measured — VERIFIED

Measured 2026-11-16 on a copy of the §5.10 `Auto (3840px)` cache, so the gate's own
evidence was never opened by Lightroom: the original pair's catalog and
`previews.db` carry the same SHA-256 before and after the run, and
`sh scripts/check.sh real-render` is green afterwards.

The deletion is the one a user can trigger, `Library > Previews > Discard Standard
and 1:1 Previews` (present in Lightroom Classic 16.0, and it asks "Discard
Standard & full-sized previews for 10 photos?"), applied to all ten images, with
Lightroom quit afterwards so the index settles. The cache was walked by declared
segment lengths before and after:

| | before | after |
|---|---|---|
| `Pyramid` rows | 20 — `standard` 9, `final` 1, `thumbnail` 10 | 10 — `bigThumbnail` 9, `final` 1 |
| `PyramidLevel` rows | 48 | 29 |
| level files | 48 | 29 |
| series, long edge | 320 × 19, 960 × 9, 1920 × 9, 3840 × 9, 160 × 1, 1200 × 1 | 320 × 10, 960 × 9, 1920 × 9, 1200 × 1 |
| rows without a file / files without a row | 0 / 0 | 0 / 0 |
| `DeletedPyramid` rows | 11 | 1 |

- **Nineteen files went and none was written**: the whole `thumbnail` pyramid of
each image (nine 320 px files and one 160 px file) and the 3840 px level of each
preview pyramid. No 1:1 level was involved — this cache held none.
- **A pyramid shrinks rather than emptying.** Levels below the standard size
survive, which is the exemption Adobe describes for the background purge (§5.5,
**reported**), so a claimed pyramid never ends with no file at all. The
whole-pyramid state `M10f` pins is therefore **not** produced by this deletion —
nor, on Adobe's own description, by the purge, which exempts thumbnails and
sub-standard levels too.
- **The companion is not spared.** A render's second pyramid is deleted outright,
and `thumbnail_pyramids` on this cache goes from 10 to 0, so ADR 0008's cache fact
is also a *disposable* fact. Lightroom rebuilds it when the image is next shown,
as its own dialog says: "when full-size data is needed again, it will be retrieved
by reading from the file".
- **`quality` is rewritten, not preserved.** The nine preview pyramids now carry
`Pyramid.quality = bigThumbnail`, so the column describes the levels a pyramid
currently holds rather than the build that wrote it. That is the value ADR 0009
refuses to treat as a companion, and it is a value a **claimed** pyramid can now
carry.
- **No stale row is left behind.** The discard deletes `PyramidLevel` rows and
their files together: 29 rows, 29 files, 0 disagreements. The pruned-level state
(`M3`, a row whose file is gone) is not produced by this deletion either.
- **Tombstones are consumed, not written.** The eleven rows before included one
per deleted companion; the run removed those ten and wrote none for the levels it
deleted. The one left names a pair that is *also* live in `Pyramid` — a state
§5.10's cache already held before this run — so a tombstone is evidence of a
deletion event, never of a payload's absence.

What the reader then reports, on the same cache: `scan --json` gives `layout
modern`, `images 10`, `recovered 10` (tier `large` 9, `standard` 1),
`thumbnail_pyramids 0`, `unmapped 0`, `missing_payload 0`, `below_floor 0`,
`superseded 0`, `deleted 0` and exit 0. `extract --min-long-edge 0
--include-present` writes ten JPEGs totalling 2 731 955 payload bytes, exits 0,
and its manifest reports the demoted `quality`. The same ten write 2 733 187 bytes
on disk — the difference is the ten metadata segments ours adds — so `2.6 MiB` is
the same figure whichever you count. A discard is a cache-size event, not a loss:
the largest surviving level in this cache was 1920 px, and no image needed a
rescue.

**The copy was rebuilt, and a gate holds every count.** The first one was deleted
with the scratch tree it lived in; the replacement was cut from the same
`Auto (3840px)` render on 2026-11-17, by the recipe in [rendering a test
cache](agents/rendering-a-test-cache.md), and kept on an evidence root with its
`EVIDENCE.md` beside it. It reproduces the table above: 48 level files before and
29 after, nineteen gone — nine 3840 px levels, nine 320 px companions and one
160 px one — and all 29 survivors byte-identical to the render. `sh
scripts/check.sh real-discard` (`crates/lrsalvage-cli/tests/discard.rs`) asserts
the object above — the tier split, the six zeroes, the 29-file shape, the demoted
`quality` and the 2 731 955-byte payload total — against a copy named by
`LRSALVAGE_DISCARD_CATALOG` and `LRSALVAGE_DISCARD_PREVIEWS`, and fails rather
than skipping when they are unset. The payload total is the number that
reproduced the `0.1.0` build's own figure exactly, which is what makes it a fact
about Adobe's payloads rather than about this tree.

**The state has a fixture row now.** `M10h` of `specs/fixtures.md` composes a
claimed pyramid holding only 320, 960 and 1920 px, typed `bigThumbnail`, with no
companion, and pins what the reader reports in it: `recovered` at the surviving
1920 × 1280 level, tier `large`, `quality bigThumbnail` on a **claimed** pyramid,
and the same four counters the run above reports, at zero. It is a synthetic
composition of a state this section measured, and its label says both: the reader
is what the row pins, and this section stays the record that Lightroom writes the
state.

**What neither measures — TODO.** What a discard does to the pyramid row's own
`croppedWidth` and `croppedHeight`. The level set before and after is in the
table above; the cropped size was never read, and no reader path consults it for
a level-file pyramid, so `M10h` composes the pre-discard value and claims nothing
about it. Reading those two columns off a discarded copy would settle it, and is
the first thing to check if the container arm ever needs that column for a
demoted pyramid.


---

## 6. Useful queries — VERIFIED

### 6.1 Reconstructing the original path

The path is split across three tables, and both separators are already in the
data:

```sql
SELECT rf.absolutePath || fo.pathFromRoot || fi.baseName || '.' || fi.extension AS path
FROM Adobe_images i
JOIN AgLibraryFile fi ON fi.id_local = i.rootFile
JOIN AgLibraryFolder fo ON fo.id_local = fi.folder
JOIN AgLibraryRootFolder rf ON rf.id_local = fo.rootFolder;
```

- `AgLibraryRootFolder.absolutePath` carries a **trailing separator**. Lightroom
  writes it with POSIX `/` even on Windows (`C:/Users/.../Pictures/`), so
  concatenate the stored strings and let the host accept both separators; do not
  re-split on the host's separator.
- `AgLibraryFolder.pathFromRoot` also carries a trailing separator, and is
  **empty at the root** of a drive. Concatenating without normalising therefore
  works, which is why the query is so short.
- The extension is stored without a dot, hence `'.' || extension`.
- Virtual copies share one `AgLibraryFile` row, so this query returns the master
  path for every copy — the copy name lives elsewhere on `Adobe_images`.

This yields the path of the **original** file, which is usually the file that is
gone. Checking whether it still exists is a filesystem question SQL cannot
answer: an image is only a salvage candidate once that path fails to resolve.

Note that `Adobe_images.id_local` is what `ImageCacheEntry.imageId` joins to, so
the preview join and the path join compose directly.

### 6.2 Counts worth running first

```sql
SELECT COUNT(*) FROM ImageCacheEntry;
SELECT quality, COUNT(*) FROM Pyramid GROUP BY quality ORDER BY 2 DESC;
SELECT colorProfile, COUNT(*) FROM Pyramid GROUP BY colorProfile ORDER BY 2 DESC;
```

These produced the statistics in section 2.7. They are also the fastest way to
tell whether a `previews.db` is populated at all before walking the whole cache.

### 6.3 Metadata from `Adobe_AdditionalMetadata.xmp`

The XMP packet is stored as a **zlib blob with a 4-byte prefix**:

```
xmp_xml = zlib_decompress(blob[4..])
```

Ignore the first four bytes; inflating the whole blob fails. The result is a
normal XMP packet, and it is the richest metadata source in the catalog: camera
make and model, lens, ISO, exposure time, f-number, focal length, rating, and GPS
(**verified**). It is far cheaper than joining the `AgHarvestedExifMetadata` and
interned-string tables, at the cost of parsing XML.

The row is keyed by an `image` column that joins to `Adobe_images.id_local`. The
attribute spellings that carry the fields are fixed by Adobe's schema:

| Field | XMP attribute |
|---|---|
| capture time | `exif:DateTimeOriginal`, else `xmp:CreateDate` |
| camera make / model | `tiff:Make`, `tiff:Model` |
| lens | `aux:Lens`, else `exifEX:LensModel` |
| ISO | `exif:ISOSpeedRatings` (often an `<rdf:li>` wrapper) |
| exposure | `exif:ExposureTime` |
| aperture | `exif:FNumber` |
| focal length | `exif:FocalLength` |
| rating | `xmp:Rating` |
| GPS | `exif:GPSLatitude` + `exif:GPSLatitudeRef`, same for longitude |

**Settled 2026-11-16 — the columns beside `xmp` are not a gap: nothing reads
them.** `catalog::xmp` is the one reader of this table, and its query names two
columns, the row's `image` key and `xmp` itself
(`SELECT xmp FROM Adobe_AdditionalMetadata WHERE image = ?1 LIMIT 1`, **verified**
in `catalog.rs`). A list of the rest would be a claim a reader trusts and no code
acts on, which is what the evidence standard exists to prevent. A query that
needs one records it here, with its label, in the commit that reads it. The path
itself is not a 13.2 result carried forward: all 78 images of the 16.0 in-place
upgrade took a capture time and a rating out of this blob and into their output
(§5.9), so the catalog's text reaches a modern catalog unchanged.

`extract` writes the rating twice when it can: unconditionally as EXIF tag
`0x4746` (`Rating`, a SHORT in IFD0), and as `xmp:Rating` in a small XMP APP1
packet that it adds only when the payload carries no XMP packet of its own
(**verified** in `exif.rs`, and on the 78-payload cache: no payload there had an
XMP packet, so all 78 outputs received one). A payload with its own packet keeps
it byte-identical; the rating still lands in EXIF and the omission is recorded in
the outcome note. `xmp:Rating = 0` is written like any other value — it is a
recorded rating of zero, not an absent one.

### 6.4 What the catalog does not carry

The **catalog** carries no ICC bytes: `previews.db` records only the profile *name*
in `Pyramid.colorProfile` (`sRGB`, `AdobeRGB`), and Adobe reports that extracted
previews embed no profile and are assumed sRGB on re-import (section 5). A
*payload* may carry its own bytes instead — a level a render wrote does, and every
one measured so far carries a 560-byte Adobe RGB (1998) profile (§5.8) — and then
those bytes win and `extract` copies them verbatim. Where the payload carries
none, embedding a profile is a reconstruction from a name, and must never be
presented as the original profile (ADR 0003).

`extract` does embed that reconstruction, and the sizes are fixed because the
bytes are built, not read: `sRGB` → 2,084 bytes, described as
`sRGB IEC61966-2.1`; `AdobeRGB` → 560 bytes, described as `Adobe RGB (1998)`.
Both sizes match the profile's own declared size field (**verified** by calling
the builder and by inspecting the 78 extracted files, all 78 carrying a
2,084-byte `sRGB IEC61966-2.1` profile). They are ICC v2 matrix/TRC profiles
with D50-adapted primaries, a D50 media white point and one tone curve per
channel: sRGB uses a 256-sample curve, Adobe RGB a single gamma of 563/256. Each
fits one APP2 `ICC_PROFILE` chunk; a larger profile would be split and numbered
(ICC.1 Annex B), which no built-in profile needs.

Name matching ignores case, spaces and punctuation, so `AdobeRGB`,
`Adobe RGB (1998)` and `adobergb1998` all resolve. Any other name — including
`ACR`, which appears in some catalogs — produces an outcome note,
`colour profile <name> has no built-in ICC profile; none embedded`, and no
profile is written. sRGB is never substituted for a name we cannot build.

This profile is a statement about the colour space, not a copy of the original
file's profile. Presenting it as the latter would be a claim the catalog cannot
support.

**Unmeasured:** whether the built `AdobeRGB` profile is byte-identical to the one
Lightroom embeds. Their sizes agree at 560 bytes, but no run has had to build it
in a cache whose payloads carry their own, because those bytes won (§5.8).

---

## 7. Sources

| Source | What it contributes | Label |
|---|---|---|
| [seachess.net, "Dive into Lightroom catalogues" (2022)](https://www.seachess.net/notes/dive-into-lightroom-catalogues/) | Best public description of the `AgHg` container; a hexdump showing `header_len = 0x0020`, 8-byte padded name, and the catalog path query | reported |
| [github.com/aalpern/luminosity](https://github.com/aalpern/luminosity) — `previewfile.go`, `previews.go` | Go chunk reader and `previews.db` queries; the closest thing to a reference implementation of the container | reported |
| [github.com/anykolaiszyn/Lightroom-Preview-Recovery](https://github.com/anykolaiszyn/Lightroom-Preview-Recovery) | Python recovery walkthrough, uuid/digest mapping | reported |
| [github.com/ibips/lrprev-extract](https://github.com/ibips/lrprev-extract) | Extracts by scanning for SOI/EOI. The carving bug: a truncated level yields the embedded EXIF thumbnail instead of failing, so the output looks plausible and is thumbnail-sized. The cautionary case for ADR 0001 | reported |
| [github.com/fdenivac/lrprev-extract](https://github.com/fdenivac/lrprev-extract) | **GPL.** Reference for behaviour only; do not copy code into this MIT/Apache project | — (licence) |
| [github.com/hfiguiere/lrcat-extractor](https://github.com/hfiguiere/lrcat-extractor) | Rust; catalog parsing only, no pixel extraction | reported |
| [Adobe, Lightroom Classic catalog FAQ](https://helpx.adobe.com/lightroom-classic/desktop/technical-support/workflow-issues/catalog-issues/catalog-faq-lightroom.html) | Preview storage changed in 13.3 | reported |
| [Adobe, "Extract preview for lost images"](https://helpx.adobe.com/lightroom-classic/desktop/kb/extract-previews-for-lost-images-lightroom.html) | 13.2 vs 13.3 filename conventions; Adobe's extractor writes no ICC profile | reported |
| Ian Lyons (Adobe Community Expert), Adobe Community, July 2024 and September 2024 | Per-image folders, extensionless JPEGs, long-edge series, single-file pyramid gone, stale `previews.db` rows. Specific thread URLs TODO: not captured in this pass | reported |
| [regex.info, "Extract Cached Image Previews", changelog 20240611.65](https://regex.info/blog/lightroom-goodies/preview-extraction) | Confirms the 13.3 change from an independent implementer | reported |

The pattern across these sources is worth naming: every public tool predates 13.3
or handles one half of the problem (container **or** catalog). None of them
documents `.lrcat-data`, and none reconciles the preview index against the
catalog's image table the way section 2.7 does.

---

## Appendix: constants

```
.lrprev magic            "AgHg" (41 67 48 67)
.lrprev fixed header     24 bytes (4 + 2 + 1 + 1 + 8 + 8)
.lrprev observed         header_len = 32, name field 8 bytes
.lrprev final padding    may be omitted by the last chunk (154 of 3,057 files)
byte order               big-endian (container), little-endian (TIFF inside .lrcat-data)
payload name             <UUID uppercase>-<digest lowercase>.lrprev
payload path             <cache>/<uuid[0]>/<uuid[0..4]>/...
.lrcat-data prefix       16 bytes, TIFF starts at 0x10, strip offsets relative to 0x10
.lrcat-data compression  8 ("Adobe Deflate") but the strips are zlib
root-pixels.db           101 x 68 JPEG per image, ~3.5 KB
orientation              AB 0, BC 90, CD 180, DA 270 (unrotated payloads)
```
