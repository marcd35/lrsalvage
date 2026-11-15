---
id: adr-0003-preserve-preview-bytes
type: architecture-design
title: "ADR 0003: Preserve preview bytes"
status: active
parent: architecture
---

# Preserve preview bytes

**Status:** accepted · **Date:** 2026-11-12 · **Amended:** 2026-11-15

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

When the payload carries no profile — the only case any measured cache shows
(`docs/format-notes.md` §5.7) — the ICC profile is rebuilt from the catalog's
*name*, never recovered from a source file, because the catalog stores no profile
bytes (§6.4). Two names resolve to a built-in, reproducible
ICC v2 matrix/TRC profile: `sRGB` to the 2,084-byte `sRGB IEC61966-2.1` profile
and `AdobeRGB` to the 560-byte `Adobe RGB (1998)` profile. Matching ignores case,
spaces and punctuation.

Unknown profile names are reported and no profile is embedded. The writer says
`colour profile <name> has no built-in ICC profile; none embedded`; it never
substitutes sRGB for a name it does not recognise. A payload that already has an
XMP packet is left alone, and its rating stays in EXIF only, with a note.

### Precedence when the payload already carries metadata

**Amended 2026-11-15.** The decision above was written when a payload was a level
inside a `.lrprev` container and no inspected level carried an `Exif` APP1
(`docs/format-notes.md` §1.4). The modern layout stores a level file per image,
and a *rendered* JPEG may carry its own EXIF, XMP or ICC profile. Whether
Lightroom writes those today is **unverified** (§5.8), so the order below is
chosen to be safe under either answer: precedence follows **evidence strength**,
not authorship.

| Segment | The payload already carries one | The payload carries none |
|---|---|---|
| APP2 ICC | the payload's profile is copied **verbatim**; none of ours is embedded | ours, reconstructed from `Pyramid.colorProfile`; an unknown name still yields a note and no profile |
| APP1 Exif | **ours replaces it**; the payload's segment is dropped and the displacement is recorded | ours, as decided above |
| APP1 XMP | the payload's packet stays **byte-identical**; ours is not written and the rating stays in EXIF `0x4746` | ours (rating only) |

1. **A real profile beats a reconstructed one.** An embedded ICC profile
describes the pixels in this file; ours describes the colour space the catalog
recorded for the original, built from a name (§6.4). The two are never compared —
reading an ICC description would add format surface that cannot change the
output — and a reconstruction never displaces a real profile.
2. **The catalog beats a render-time snapshot of the catalog's own facts.** EXIF
is the one segment replaced wholesale, because the catalog holds the current
rating, capture time, camera, lens, exposure and GPS, while a payload's segment
holds what was true when the render was made. What the payload carried that we
cannot replace — maker notes, software — is **reported**, and `--no-metadata` is
the per-run escape hatch that writes the payload's bytes unchanged.
3. **A foreign XMP packet is never rewritten.** Unchanged from the decision
above: a partial merge of untrusted XML is worse than the honest gap, and no
information is lost because the rating is in EXIF anyway.
4. **Orientation comes from the payload when the payload states it.** If the
payload's own EXIF carries `Orientation` (IFD0 `0x0112`), that value describes
*these* pixels and wins; the catalog's orientation applies only when the payload
says nothing. Pixels are still never rotated, so honouring the tag a render
carried cannot rotate wrongly. A disagreement is a note, never a status:
`suspect` keeps its meaning, bytes against the row's dimensions
(`adr-0001-parse-dont-carve`). A value this writer cannot restate — a mirrored
orientation (2, 4, 5, 7), which Lightroom's catalog has no code for — states
nothing usable, so the catalog's orientation applies rather than a mirror being
silently restated as a rotation.
5. **Every output names itself.** Whenever metadata is written, our EXIF carries
`Software` (IFD0 `0x0131`) set to `lrsalvage <version>` — even when the catalog
recorded nothing else, so the segment exists for its marker alone. That is the
resume identity: an existing destination is ours only when the marker is present,
the file is a complete JPEG (`SOI` … `FF D9`), and its `SOF` dimensions match the
plan item. A same-size foreign JPEG is therefore *not* ours, which the earlier
"complete JPEG of the right size" check could not tell.
6. **`--no-metadata` output is outside verification.** It writes the payload's
own bytes, so it carries no marker and can never be proven ours: a rerun reports
the destination as not a verified extraction and asks for `--force`. That is the
honest consequence of promising byte-identical output, and the flag's help must
say it rather than let it be discovered.
7. **One metadata set for both layouts.** The modern layout changes where the
payload comes from, never which facts we add: one `MetadataInput`, one segment
builder, the same notes.

The payload's own segment inventory — `exif`, `xmp`, `icc`, empty when bare — is
reported per item as `payload_metadata`, recorded by the same certification walk
`adr-0005-discover-payloads-by-index` already requires, so it costs no second
read. The existing note keeps the prose: what was kept, what was replaced.

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
- An output carries our `Software` marker, so a resume proves authorship instead
  of plausibility. A destination written before the marker shipped, or written
  by `--no-metadata`, or written by another tool, is not recognised as ours: it
  is left byte-identical and reported `failed` unless `--force` replaces it. The
  first resume after the marker ships therefore re-extracts rather than keeps.
- A payload's displaced EXIF is a reported loss, not a silent one: maker notes
  and other render-time tags are gone from that output, the note says so, and
  `--no-metadata` recovers the payload's own bytes.
- When a payload carries its own ICC profile, the recovered file's colour space
  is the payload's. `color_profile` still records what the catalog claimed and
  `payload_metadata` records that the file's profile is the payload's own, so a
  reconstruction is never implied where none happened.
- Metadata that disagrees with the payload never changes a status. `suspect`
  stays reserved for bytes that disagree with the row's dimensions, so one
  status never means two different recoveries.

## Considered options

- **Re-encode at quality 100 to bake in rotation.** Irreversible, larger files,
  and it destroys the only evidence of what Lightroom actually stored.
- **Write a PNG or TIFF.** Lossless wrapper, but it inflates a lossy source and
  breaks the promise that the recovered file is the cached JPEG.
- **Strip metadata to keep output minimal.** Throws away the camera, date, and
  GPS that make a recovered preview identifiable years later.
- **Merge the payload's EXIF into ours, field by field.** Nothing the payload
  carried would be dropped, but it needs a TIFF IFD copier — offsets, maker notes
  — and it would let a stale render-time rating outrank the catalog's current
  one.
- **Let a payload's metadata win wholesale.** Cheap and never destructive, but
  the rating and the orientation fix could vanish from a preview we did not
  render, which is the opposite of what a salvage tool is for.
- **Trust any complete JPEG of the right size as ours.** The rule this amends: a
  foreign file of the same dimensions passes it, so a resume could keep somebody
  else's image and report it recovered.
- **Identify output by re-assembling it and comparing bytes.** No marker to
  write, but a rating edited in the catalog, or any change to the segment writer,
  makes our own past output foreign and impossible to keep without `--force`.
