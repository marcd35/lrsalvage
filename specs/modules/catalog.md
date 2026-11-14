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
metadata blob inflated into `model::XmpMetadata`.

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
- **The XMP blob is zlib-compressed text** and is parsed by hand for the handful
  of fields worth restoring. A missing field stays `None`; nothing is inferred
  from a neighbouring field, except the documented capture-time fallback to
  `CreateDate`, normalised to `YYYY-MM-DDTHH:MM:SS`.
- **A missing metadata table is tolerated**: `has_metadata` distinguishes a
  catalog whose `Adobe_AdditionalMetadata` table is absent from one whose blob is
  empty, so an older schema reports less metadata rather than failing the run.
- **Purposes are typed, not strings.** Orientation codes map through
  `Orientation::from_catalog_code`; an unknown or NULL code becomes `Unknown`
  rather than a guessed rotation.
