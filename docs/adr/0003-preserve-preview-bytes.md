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

Copy the payload bytes verbatim and never re-encode by default. Put the recovered
information around those bytes: EXIF (orientation, capture time, camera, lens,
exposure), XMP (rating, GPS), and an ICC profile derived from the colour profile
name the catalog recorded.

Operations that change pixels — rotation, conversion to sRGB — are opt-in flags
only. When one is used, the run report says the pixels were altered.

## Consequences

- Default output is byte-identical to the cached JPEG. The quality is exactly
  what Lightroom stored, with no second generation of loss and no argument about
  which encoder is better.
- Orientation lives in EXIF, so a viewer that ignores EXIF still shows the image
  sideways. Accepted: rotating pixels to satisfy a display default is a permanent
  loss in exchange for a temporary convenience.
- The ICC profile is reconstructed from a name (`sRGB`, `AdobeRGB`), not recovered
  from the file, and must be described that way. An unrecognised profile name
  means no profile plus a warning, because a wrong profile is worse than none for
  anyone doing colour-critical work.
- A rotated or colour-converted output has different pixels from the default one.
  That difference must be visible in the file name or the report, never silent.
- There is no "improve the JPEG on write" option, and there never should be:
  re-encoding cannot add detail and can only remove it.

## Considered options

- **Re-encode at quality 100 to bake in rotation.** Irreversible, larger files,
  and it destroys the only evidence of what Lightroom actually stored.
- **Write a PNG or TIFF.** Lossless wrapper, but it inflates a lossy source and
  breaks the promise that the recovered file is the cached JPEG.
- **Strip metadata to keep output minimal.** Throws away the camera, date, and
  GPS that make a recovered preview identifiable years later.
