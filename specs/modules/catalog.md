---
id: catalog
type: module-design
title: "catalog: read-only queries against the .lrcat"
status: draft
parent: architecture
depends-on:
  - snapshot
  - model
---

# catalog

## Responsibility

Answer, from a snapshot, what the catalog knows about each image: identity, copy
name, orientation, file format, the reconstructed original path, and the XMP
metadata blob inflated into `model::XmpMetadata` — plus the two format-generation
strings it records about itself.

## Boundary

Reads only through `snapshot`'s copy and only the catalog's own tables. It never
writes, never touches `previews.db` or a `.lrprev`, and holds no opinion about
statuses, tiers or output paths — those belong to `plan` and `model`.

## Decisions

- **A database without `Adobe_images` is not a catalog.** That check is the
  difference between exit 1 "not a catalog" and a confusing empty run.
- **Every `Adobe_images` row is returned**, whether or not it has a file row, so
  an image is never silently dropped by a join — a catalog image missing from the
  report would look like "no preview was built".
- **The original path is reconstructed, not trusted.** It is the root folder's
  `absolutePath`, then `pathFromRoot`, then the file name; the output tree later
  takes only the root's last segment (`model`), so the full absolute path is
  never reproduced under the destination.
- **The roots are a query of their own, because a joined path cannot be split
  back.** `root_folders` returns one `RootFolder` per `AgLibraryRootFolder` line
  — the stored `absolutePath` verbatim, trailing separator and all — with the
  image count under it. That is the "last known folder" a person recognises a
  disconnected drive by, and asking it here rather than rebuilding it from
  `original_path` keeps one reading of the schema. The counts are images, virtual
  copies included, so their sum is every image whose file row is intact: an image
  whose row is gone has no root to be counted under and is `plan`'s to report.
- **The XMP blob is zlib-compressed text** and is parsed by hand for the handful
  of fields worth restoring. A missing field stays `None`; nothing is inferred
  from a neighbouring field, except the documented capture-time fallback to
  `CreateDate`, normalised to `YYYY-MM-DDTHH:MM:SS`.
- **The two format-generation strings are reported, never interpreted.**
  `Adobe_DBVersion` and `Adobe_CrsVersion` are read from
  `Adobe_variablesTable` and returned verbatim. A missing table, a missing row, a
  NULL and an empty value are all `None`, the same absent-tolerant shape `xmp`
  has: a catalog that records less reports less. Neither string is mapped to a
  Lightroom release and neither decides whether a cache may be read
  (`adr-0004-detect-layout-not-version`).
- **A missing metadata table is tolerated**: `has_metadata` distinguishes a
  catalog whose `Adobe_AdditionalMetadata` table is absent from one whose blob is
  empty, so an older schema reports less metadata rather than failing the run.
- **Purposes are typed, not strings.** Orientation codes map through
  `Orientation::from_catalog_code`; an unknown or NULL code becomes `Unknown`
  rather than a guessed rotation.
