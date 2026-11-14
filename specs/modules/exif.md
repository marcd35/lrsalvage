---
id: exif
type: module-design
title: "exif: metadata segments around untouched payload bytes"
status: draft
parent: architecture
depends-on:
  - model
references:
  - adr-0003-preserve-preview-bytes
  - format-notes
---

# exif

## Responsibility

Put the catalog's facts back around a preview's pixels: an EXIF APP1 segment
(capture time, camera, lens, ISO, exposure time, f-number, focal length, GPS,
0x4746 star rating, orientation), a small XMP packet repeating the rating, and an
ICC APP2 profile reconstructed from the profile name.

## Boundary

Bytes in, bytes out. No file I/O, no pixel work, no catalog access — the facts
arrive as `MetadataInput`, built from `XmpMetadata` and the orientation `plan`
put on the item.

## Decisions

- **The payload is never re-encoded; only metadata segments are added or
  replaced** (`adr-0003-preserve-preview-bytes`). An EXIF or ICC segment already
  in the payload is replaced, never duplicated, and the compressed image data is
  untouched.
- **A payload that carries its own XMP is left byte-identical.** No merge is
  attempted: a partial merge would be worse than an honest gap. The rating still
  lands in EXIF, and the omission goes into the per-file note.
- **Only what the catalog recorded.** Missing fields stay missing — no guessed
  timestamps, no invented camera names. An explicit `xmp:Rating = 0` is a
  recorded rating and is written as such.
- **The ICC profile is rebuilt from a name, never recovered.** The catalog stores
  no profile bytes, so `sRGB` resolves to the 2,084-byte `sRGB IEC61966-2.1`
  profile and `AdobeRGB` to the 560-byte `Adobe RGB (1998)` profile, matched
  case-insensitively and ignoring spaces and punctuation. An unrecognised name
  returns `None` and the caller reports it: a wrong profile is worse than none
  for colour-critical work.
- **Orientation is a tag, not a rotation.** A viewer that ignores EXIF shows the
  picture sideways, and that is accepted: rotating pixels would be a permanent
  loss in exchange for a temporary convenience. Any future pixel-changing
  operation must be opt-in and must say so in the report.
