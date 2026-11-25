---
id: adr-0014-recover-legacy-containers-without-an-index
type: architecture-design
title: "ADR 0014: Recover legacy containers without an index"
status: active
parent: architecture
references:
  - adr-0001-parse-dont-carve
  - adr-0002-map-via-previews-db
  - adr-0003-preserve-preview-bytes
  - adr-0006-degrade-without-guessing
  - format-notes
  - glossary
---

# Recover legacy containers without an index

**Status:** accepted · **Date:** 2026-11-24

The [legacy cache spike](../format-notes.md#15-legacy-cache-without-an-index)
reported usable containers in a cache with no preview index.
[ADR 0002](0002-map-via-previews-db.md) reserves image-to-payload associations
for the preview index. [ADR 0006](0006-degrade-without-guessing.md) reserves
export of unclaimed payloads for a separate operation.

The browser app will offer legacy cache recovery as an explicit operation.
It opens no preview index and walks the selected cache read-only for
`.lrprev` files, including containers outside Lightroom's usual shard folders.
The existing modes keep their mapping and index rules. A missing, unreadable
or unsupported index result offers a separate action for the legacy scan and
explains that it recovers previews from legacy containers. Modern standalone
level files are outside the operation's scope.

One candidate represents one container path. Select its certified JPEG with
the largest measured pixel area. An equal-area tie goes to the first certified
chunk in file order. Complete chunks before a broken boundary are eligible.
The review initially selects every usable preview, including thumbnails.
Each enumerated candidate is counted once. A certified JPEG makes it usable.
With no certified JPEG, a candidate whose image payloads are all recognized
non-JPEG formats is unsupported if the container parsed without error. All
other candidates without a certified JPEG are unreadable or invalid. If a
directory cannot be listed, fail the scan with an inventory error. Show an empty result with no
save action when there are no usable previews.

The generated name is `legacy-preview-<sha256>.jpg`, using the full lowercase
SHA-256 of the exact cache-relative path with components joined by `/`.
Moving the whole cache or adding another file preserves the name. Moving the container within the cache changes it.
Separate paths keep separate outputs even when they hold identical JPEG bytes.
A byte-identical destination counts as already recovered. A different file
triggers the shared destination conflict choices. **Keep both** writes a
numbered name and preserves the existing file.
Write the selected JPEG verbatim. Describe it as a preview with a generated
name and no catalog association.
