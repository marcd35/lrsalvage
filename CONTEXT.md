---
id: glossary
type: architecture-design
title: "lrsalvage domain glossary"
status: active
parent: lrsalvage
tags:
  - domain
  - glossary
---

# lrsalvage

The vocabulary of Lightroom preview salvage: what survives when the photographs
are gone, and what we refuse to call it.

## Storage

**Catalog**:
The single record Lightroom Classic keeps of every image it has imported: where
the original lives, what was done to it, and everything it knows about it.
_Avoid_: library, database, lrcat

**Preview cache**:
The directory of rendered copies Lightroom keeps beside a catalog so it never has
to re-render an image just to show it.
_Avoid_: cache folder, preview folder, lrdata

**Pyramid**:
All the cached renders of one image, from thumbnail to full size, kept together
and described as a single unit.
_Avoid_: preview set, preview file, image cache

**Level**:
One size within a pyramid: a complete, independently viewable image.
_Avoid_: tier, size, resolution, rendition

**Payload**:
The stored bytes of one level, exactly as they sit in the cache, before any
metadata of ours is wrapped around them.
_Avoid_: blob, JPEG data, preview bytes

**Preview layout**:
The way a cache arranges its payloads on disk: a legacy pyramid, one file per
level, both at once, nothing at all, or something unrecognised. A property of the
cache, not of an image.
_Avoid_: format, cache version, schema

**Catalog format version**:
The number the catalog carries for its schema generation. It changes when Adobe
rewrites the format and does not follow the marketing release, so it is reported
and never interpreted.
_Avoid_: catalog version, Lightroom version, app version

**Cache entry**:
The catalog's claim that a particular image has a pyramid in the cache, and which
pyramid that is.
_Avoid_: row, mapping, index entry

**Orphan payload**:
A payload that exists in the cache but is claimed by no image in the catalog.
_Avoid_: stray file, unmapped file

## Recovery

**Salvageable image**:
A catalog image whose original file cannot be found and for which at least one
payload survives. An image whose original is still on disk is not a salvage
target unless the caller explicitly asks for it.
_Avoid_: recoverable image, lost photo, missing image

**Best level**:
The largest level present in an image's pyramid, and the one salvage writes by
default.
_Avoid_: highest quality, full size, original

**Tier**:
A size class for a best level, by long edge: `full` (4000 px and above), `large`
(1920–3999), `standard` (1024–1919), `small` (640–1023), `thumbnail` (below 640).
The classes set expectations; they do not change what is in the cache.
_Avoid_: quality, resolution

**Recovered**:
A JPEG now exists at the planned output path for the image.
_Avoid_: restored, rescued

**Restored**:
The original file exists again at its catalogued path, in its original format.
lrsalvage never does this, and no preview can lead to it.
_Avoid_: recovered

**Suspect**:
An image whose payload is a genuine photograph but whose decoded size disagrees
with the size the cache recorded. It is still recovered, and flagged, rather than
silently trusted or thrown away.
_Avoid_: corrupt, damaged

**Original present**:
An image skipped because its original file is still on disk. There is nothing to
salvage; asking for such images explicitly turns them into ordinary recovery
candidates.
_Avoid_: skipped, duplicate, existing photo

## Images

**Original**:
The source file — RAW, JPEG, TIFF, DNG — at the path the catalog records. The one
thing salvage cannot recreate.
_Avoid_: master, source photo

**Colour profile**:
The colour space the catalog records for a pyramid by name, such as `sRGB` or
`AdobeRGB`. A recovered file carries a profile reconstructed from that name when
one can be built, never the profile the original file had.
_Avoid_: embedded profile, colour space

**Virtual copy**:
An additional catalog image that shares one original but has its own edits and
its own cached preview.
_Avoid_: duplicate, version, copy
