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
payload survives.
_Avoid_: recoverable image, lost photo, missing image

**Best level**:
The largest level present in an image's pyramid, and the one salvage writes by
default.
_Avoid_: highest quality, full size, original

**Tier**:
A size class that sets expectations about a best level: `best-available`
(whatever this image happens to have), `standard` (about 1440 px on the long
edge), `small` (between thumbnail and standard), `thumbnail` (about 160 px,
enough to recognise the frame).
_Avoid_: quality, resolution

**Recovered**:
A JPEG now exists for the image. It is the only thing lrsalvage ever produces on
disk.
_Avoid_: restored, rescued

**Restored**:
The original file exists again at its catalogued path, in its original format.
lrsalvage never does this, and no preview can lead to it.
_Avoid_: recovered

**Suspect**:
An image whose payload disagrees with what the catalog recorded about it, and is
therefore reported rather than trusted silently.
_Avoid_: corrupt, damaged

## Images

**Original**:
The source file — RAW, JPEG, TIFF, DNG — at the path the catalog records. The one
thing salvage cannot recreate.
_Avoid_: master, source photo

**Virtual copy**:
An additional catalog image that shares one original but has its own edits and
its own cached preview.
_Avoid_: duplicate, version, copy
