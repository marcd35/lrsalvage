# Preserve preview bytes

**Status:** accepted · **Date:** 2026-11-12

## Context

A cached preview is often the last surviving copy of a photograph, and it is
already a lossy JPEG. Any decode followed by a re-encode spends a generation of
quality to change nothing that matters. But a byte-for-byte copy has two visible
problems: the payload carries no ICC profile at all (Adobe documents that
extracted previews are assumed sRGB when re-imported), and the pixels are stored
unrotated, so viewers that ignore EXIF show the picture sideways.

So the useful information is around the pixels, not in them.

## Decision

Copy the payload's compressed image data verbatim and never re-encode by default.
Metadata is the only thing rewritten: a new EXIF segment is inserted, and an
existing EXIF segment in the payload is replaced rather than kept alongside. Put
the recovered information around those bytes: EXIF (orientation, capture time,
camera, lens, exposure, ISO, f-number, focal length, GPS, rating), a small XMP
packet carrying the rating when the payload does not already carry an XMP packet,
and an ICC profile derived from the colour profile name the catalog recorded.

The ICC profile is rebuilt from the catalog's *name*, never recovered from the
payload, because the payload carries none and the catalog stores no profile bytes
(`docs/format-notes.md` §6.4). Two names resolve to a built-in, reproducible
ICC v2 matrix/TRC profile: `sRGB` to the 2,084-byte `sRGB IEC61966-2.1` profile
and `AdobeRGB` to the 560-byte `Adobe RGB (1998)` profile. Matching ignores case,
spaces and punctuation.

Unknown profile names are reported and no profile is embedded. The writer says
`colour profile <name> has no built-in ICC profile; none embedded`; it never
substitutes sRGB for a name it does not recognise. A payload that already has an
XMP packet is left alone, and its rating stays in EXIF only, with a note.

Any future operation that changes pixels — rotation, conversion to sRGB — must be
an opt-in flag, and the run report must say the pixels were altered.

## Consequences

- The compressed image data is identical to what Lightroom stored, so the quality
  is exactly the cached preview with no second generation of loss and no argument
  about which encoder is better. With metadata on (the default) the *file* is not
  byte-identical: EXIF, XMP and ICC segments are inserted, and a payload EXIF or
  ICC segment is replaced. `--no-metadata` is the way to get the payload's own
  bytes unchanged.
- Orientation lives in EXIF, so a viewer that ignores EXIF still shows the image
  sideways. Accepted: rotating pixels to satisfy a display default is a permanent
  loss in exchange for a temporary convenience.
- The ICC profile is reconstructed from a name (`sRGB`, `AdobeRGB`), not recovered
  from the file, and must be described that way. An unrecognised profile name
  means no profile plus a warning, because a wrong profile is worse than none for
  anyone doing colour-critical work. On the 78-payload cache, all 78 recovered
  files carry `sRGB IEC61966-2.1`; the catalog's name for every one of them is
  `sRGB`.
- Recovered files now carry that ICC profile and, when the payload has no XMP
  packet of its own, the rating packet. The rating is in EXIF at tag `0x4746`
  whenever the catalog records one; the XMP packet repeats it only when merging
  is unnecessary. A payload with its own XMP keeps it unchanged, and the omission
  is recorded in the per-file note — the packet is never rewritten, because a
  partial merge would be worse than the honest gap.
- A rotated or colour-converted output would have different pixels from the
  default one. That difference must be visible in the file name or the report,
  never silent.
- There is no "improve the JPEG on write" option, and there never should be:
  re-encoding cannot add detail and can only remove it.

## Considered options

- **Re-encode at quality 100 to bake in rotation.** Irreversible, larger files,
  and it destroys the only evidence of what Lightroom actually stored.
- **Write a PNG or TIFF.** Lossless wrapper, but it inflates a lossy source and
  breaks the promise that the recovered file is the cached JPEG.
- **Strip metadata to keep output minimal.** Throws away the camera, date, and
  GPS that make a recovered preview identifiable years later.
