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
0x4746 star rating, orientation), a small XMP packet repeating the rating, an ICC
APP2 profile reconstructed from the profile name, and a `Software` marker naming
the tool and its version.

## Boundary

Bytes in, bytes out. No file I/O, no pixel work, no catalog access — the facts
arrive as `MetadataInput`, built from `XmpMetadata` and the orientation `plan`
put on the item. It reads one fact back out of the payload: the `Orientation` tag
the payload's own EXIF declares, because that value describes the payload's
pixels and the catalog's describes the original.

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
- **The payload's own metadata wins where it is the stronger evidence**
  (`adr-0003-preserve-preview-bytes`, amended): a real embedded ICC profile is
  copied verbatim and ours is not embedded beside it, an existing XMP packet is
  left byte-identical, and the payload's own EXIF `Orientation` outranks the
  catalog's. What the catalog knows better is replaced rather than merged: our
  EXIF supersedes the payload's segment, and the disappearance is reported. The
  two profiles are never compared — reading an ICC description cannot change
  what we write.
- **Every output names itself.** `Software` (IFD0 `0x0131`) is set to
  `lrsalvage <version>` whenever metadata is written, even when the catalog holds
  nothing else, so the segment exists for its marker alone. `extract` verifies
  that marker on resume; without it a file is not ours.
- **A payload's orientation is honoured, never applied.** If the payload's EXIF
  declares an `Orientation`, the output carries that value; the catalog's
  orientation applies only when the payload states none. Pixels are still never
  rotated, so the tagged value cannot be wrong the way a re-derived one could.
- **Orientation is a tag, not a rotation.** A viewer that ignores EXIF shows the
  picture sideways, and that is accepted: rotating pixels would be a permanent
  loss in exchange for a temporary convenience. Any future pixel-changing
  operation must be opt-in and must say so in the report.
