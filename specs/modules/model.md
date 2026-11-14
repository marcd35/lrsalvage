---
id: model
type: module-design
title: "model: plain data and output-path sanitising"
status: draft
parent: architecture
references:
  - glossary
---

# model

## Responsibility

The types every other module shares — `CatalogImage`, `CacheEntry`, `Pyramid`,
`PyramidLevel`, `XmpMetadata`, `Orientation` — and the one piece of logic that
belongs to none of them: turning catalog text into a safe output path.

## Boundary

No I/O, no formatting, no database. It depends on nothing inside the crate, which
is what lets every other module depend on it freely.

## Decisions

- **The output path mirrors the folder that was imported, not the absolute path.**
  Destination + last segment of `AgLibraryRootFolder.absolutePath` +
  `pathFromRoot` + file name. A bare drive root (`C:/`) has no usable last
  segment, so that level is dropped rather than becoming a `C:` folder; the whole
  absolute path is never reproduced under the destination.
- **Never named after a uuid.** A recovered file is named after the original; a
  virtual copy gets its copy name appended, and extra levels get a `- WxH` suffix
  from `plan`.
- **Sanitising is a security boundary, not cosmetics.** A component that is `.`,
  `..`, empty after cleaning, or carries a separator is rejected, so a hostile or
  merely broken catalog string cannot escape the destination. Components are
  capped at 120 characters and the relative path at 240, counted in characters so
  a multi-byte sequence is never cut in half.
- **Over-long paths lose their shallowest folders, not their deepest.** When the
  budget runs out, the folders nearest the photo are the ones worth keeping, and
  the file name itself is never truncated.
- **An unusable base name falls back to `image-<id_local>`**, so an image with no
  file row still gets a name that identifies the catalog row rather than a blank
  or a uuid.
- **Unknown is a value.** An orientation code the catalog does not define becomes
  `Orientation::Unknown`, never a default rotation.
