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

No 13.3+ catalog was available. That is why section 5 is reported rather than
verified, and why the tool must not assume the 13.3 layout when it sees one.

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

### 2.2 Table of tables

| Table | Holds | Notes |
|---|---|---|
| `StoreInfo` | schema version | one row; observed `version` value TODO |
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
a `(uuid, digest)`. Whether Lightroom also deletes the payload at the same time
is **unverified**; if it does not, a deleted row plus a live orphan payload is a
recoverable image. TODO: test by trashing a preview in a copy of a catalog.

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
shape (column list, not the literal DDL — that was not captured, TODO):

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
recovery. What Lightroom uses it for is **unverified**; 78 rows for 78 payloads
is consistent with a grid thumbnail shown before a real preview loads. Either
way it identifies a frame and never substitutes for the best level.

Observation, **unverified**: 101 × 68 is exactly the smallest level of the
example pyramid in section 1.2. Whether the root pixel is a copy of `level_1` or
an independent render was not tested (TODO).

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

## 5. Lightroom 13.3 and later — REPORTED, not verified

Lightroom Classic 13.3 changed preview storage. All of the following is
**reported**; we had no 13.3+ catalog to inspect. The tool must therefore detect
the layout rather than assume either one.

| Claim | Source |
|---|---|
| "Changes have been made to how image previews are stored in version 13.3." | Adobe, catalog FAQ |
| Previews are stored as **per-image folders of extensionless plain JPEG files**; the long edge is in the file name. | Ian Lyons, Adobe Community Expert, July 2024 |
| Long-edge series observed: 90 / 180 / 360 / 720 / 1440 / 2880, or 105 … 3360, or 320 … 8192. | Ian Lyons, July 2024 |
| The single-file pyramid is gone; there is no `.lrprev` per image any more. | Ian Lyons, July 2024 |
| `previews.db` still exists after upgrade, but its rows can be stale. | Ian Lyons, September 2024; upgrade threads |
| Extracted filenames now carry the long edge (`DSC06787_6000.jpg`) whereas 13.2 and earlier carried both dimensions (`_MG_0233-360x240.jpg`). | Adobe, "Extract preview for lost images" |
| Extracted previews contain **no ICC profile**, so re-importing them assumes sRGB. This describes Adobe's own extractor; `lrsalvage` rebuilds a profile from the catalog's name (section 6.4). | Adobe, "Extract preview for lost images" |
| "It seems that Lightroom totally changed how they store the preview-cache data, around Lr13.3." | regex.info changelog 20240611.65 |

Consequences we can act on today:

- A stale `previews.db` row pointing at a payload that no longer exists is
  indistinguishable, from the DB alone, from a payload the user deleted. Both
  look like "missing payload". Say so in the report; do not claim corruption.
- If a cache has per-image folders of extensionless JPEGs, `.lrprev` parsing
  will legitimately find nothing. That is a layout detection result, not an empty
  catalog, and must be reported as such.
- Do not write a 13.3+ file-name convention into the extractor until a real
  13.3+ fixture exists. TODO: obtain one, then replace this entire section with
  verified byte-level notes.

Why the 1440 number matters: 1440 is the traditional default standard-preview
long edge, and the measured 1,440 × 960 group (2,696 images) is the bulk of a real
cache (section 2.7). A catalog that never had 1:1 previews built has a hard
ceiling of about 1440 px, no matter what tool reads it.

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

TODO: the table's remaining columns were not recorded; only `xmp` is read.

`extract` writes the rating twice when it can: unconditionally as EXIF tag
`0x4746` (`Rating`, a SHORT in IFD0), and as `xmp:Rating` in a small XMP APP1
packet that it adds only when the payload carries no XMP packet of its own
(**verified** in `exif.rs`, and on the 78-payload cache: no payload there had an
XMP packet, so all 78 outputs received one). A payload with its own packet keeps
it byte-identical; the rating still lands in EXIF and the omission is recorded in
the outcome note. `xmp:Rating = 0` is written like any other value — it is a
recorded rating of zero, not an absent one.

### 6.4 What the catalog does not carry

Nothing we read carries ICC bytes: `previews.db` records only the profile *name*
in `Pyramid.colorProfile` (`sRGB`, `AdobeRGB`), and Adobe reports that extracted
previews embed no profile and are assumed sRGB on re-import (section 5).
Embedding a profile is therefore a reconstruction from a name, and must never be
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
