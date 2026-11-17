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
The cache's unit for one image: an index row naming the levels that image's renders consist of, and the payloads those levels live in. A *converted* cache keeps one per image; a *render* keeps two — see below.
_Avoid_: preview set, preview file, image cache

**Preview pyramid**:
The pyramid a cache entry claims: the levels an image is displayed and edited from.
_Avoid_: main pyramid, primary pyramid

**Thumbnail pyramid**:
A second, smaller pyramid a render keeps beside the preview one — typically one
small level, and claimed by no cache entry. It is part of the render, not an
orphan: nothing was lost to produce it. The word *companion* names it in relation
to the image's preview pyramid; it is not a second name for the object. It is
disposable: a discard removes it, and Lightroom rebuilds it when the image is next
shown.
_Avoid_: thumbnail cache, lose pyramid, companion pyramid

**Render**:
A pyramid Lightroom built from the original's own pixels at the catalog's preview settings.
_Avoid_: fresh cache, built previews, generated pyramid

**Conversion**:
A pyramid carried over from a legacy container by an in-place catalog upgrade, its level bytes preserved and only their storage split.
_Avoid_: migrated cache, upgraded previews, converted cache

**Level**:
One size within a pyramid: a complete, independently viewable image.
_Avoid_: tier, size, resolution, rendition

**Payload**:
The stored bytes of one level, exactly as they sit in the cache, before any
metadata of ours is wrapped around them.
_Avoid_: blob, JPEG data, preview bytes

**Level file**:
A payload that is its own file, named by the pyramid it belongs to and the long
edge of the level it holds, as the modern layout stores them — as opposed to a
level inside a legacy container.
_Avoid_: loose JPEG, preview file, payload file

**Container**:
One file holding a whole pyramid: a header describing the levels and every level
beside it, as the legacy layout stores them.
_Avoid_: archive, bundle, lrprev file

**Preview layout**:
The way a cache arranges its payloads on disk. One word, one of five, and the tool
publishes that word because a script reads it: **legacy** (containers only),
**modern** (level files only), **mixed** (both, anywhere in the cache), **none**
(an index and no payloads) or **unknown** (payload-like files that match no
shape). A property of the cache, not of an image.
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

**Prune**:
Lightroom removing the payload bytes of levels it no longer keeps, leaving the
cache's index rows behind as the only claim that those levels existed. Two
triggers do it, and each has its own scope: the background cache-limit purge
(oldest first, thumbnails and levels below the standard size exempt) and the
timed 1:1 discard.
_Avoid_: purge, cleanup, trim — each names a trigger, never the act — and avoid
_prune_ for a discard, which deletes the rows too.

**Discard**:
The explicit command over a selection — Lightroom's *Discard Standard and 1:1
Previews*. It deletes a pyramid's rendered levels **together with their rows**,
takes a render's companion pyramid with it, and leaves levels smaller than the
standard size alone — so a pyramid shrinks rather than emptying, and nothing is
lost that could still be salvaged. It rewrites `quality`, which is why that value
describes the levels a pyramid currently holds and not the build that wrote it. It
is not a prune, because a prune leaves the rows as the only record and a discard
leaves none.
_Avoid_: prune, purge, cleanup, trim — a discard deletes the claim, not only the
bytes

**Orphan payload**:
A payload — or a whole pyramid of level files — that exists in the cache but is
claimed by no image in the catalog, and that the cache does not explain as a
render's second pyramid. A thumbnail pyramid is therefore not an orphan, and a
pyramid nothing can account for is.
_Avoid_: stray file, unmapped file

## Recovery

**Salvageable image**:
A catalog image whose original file cannot be found and for which at least one
payload survives. An image whose original is still on disk is not a salvage
target unless the caller explicitly asks for it.
_Avoid_: recoverable image, lost photo, missing image

**Best level**:
The largest level present across an image's pyramids — its own, or the render
companion beside it when the pyramid the cache entry names has no file at all —
and the one salvage writes by default.
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

## Promises

**Compatibility contract**:
What a script or a person may rely on from one version to the next: the status
words, the exit codes, the machine-readable field sets and their schema names,
the report columns, the output names, the flags, and the marker that identifies
an output as ours. It does not include the version number written inside that
marker.
_Avoid_: public API, output format, interface

**Verification report**:
The object a run can write about the cache it read and the plan it built — the
layout, the version strings, the options applied, the counts and the filename
shapes — with every path replaced by a template, so a result can leave the
machine without carrying one of the user's.
_Avoid_: scan report, summary, log
