---
id: cache-recovery
type: module-design
title: "cache_recovery: a JPEG per candidate without a catalog"
status: draft
parent: architecture
depends-on: [error, fs, jpeg, lrprev, model, plan, previews-db]
references: [adr-0001-parse-dont-carve, adr-0002-map-via-previews-db, adr-0003-preserve-preview-bytes]
---

# cache_recovery

Answer for a preview cache with no catalog, in the two shapes such a cache comes
in: an index that still names every pyramid, and a legacy cache whose index is
gone. Neither mode writes a file, and neither opens a catalog.

## Responsibility

**Indexed.** Require a readable preview index and make one plan item per distinct
indexed pyramid. The positive evidence is a row in Pyramid, PyramidLevel, or
ImageCacheEntry. A tombstone or a file on disk alone adds no item. The index
reader opens a private copy including SQLite sidecars and distinguishes a
missing, unreadable, or unsupported index.

Rank complete JPEGs by measured SOF pixel area. Equal sizes prefer the modern
level file, then the cache-relative path and the legacy chunk order. Only
open paths whose spelling in the cache walk matches the exact indexed identity.
A traversal failure ends the scan because the file inventory is incomplete. Read legacy chunks by
their declared boundaries. The intact prefix of a damaged container can still
supply certified JPEGs. The scan reports one outcome per indexed pyramid:
recoverable, invalid, unsupported, or missing, in that order. A recoverable
pyramid flags a larger unavailable level or a dimension disagreement when the
index or payload header provides that evidence.

The planned filename is the full lowercase SHA-256 of the UUID, a zero byte,
and the digest as stored, with a `preview-` prefix and a `.jpg` extension. The
plan retains a reference to the chosen payload and its digest. The reader
rechecks it before returning the unmodified bytes to a front end. A stopped
scan returns no partial plan.

**Indexless (#198).** Enumerate every `.lrprev` container under the chosen cache
root and make one plan item per container, wherever it sits. Nothing is opened
to decide candidacy: there is no index to name a payload's place against, so the
shard folders are not an address and a container outside them is a candidate
too. The walk is strict — a folder it cannot read ends the scan as an inventory
error, because an incomplete inventory cannot be an answer — and a file it cannot
read is one candidate's outcome rather than a stopped scan.

Read each container by its declared chunk boundaries, never by scanning for
markers (`adr-0001-parse-dont-carve`), and certify every `level_N` chunk with
`jpeg::certify_recovery`: the JPEG's own framing and SOF dimensions. An item's
outcome is one of three:

- **usable** — at least one chunk is a complete JPEG with a non-zero area. The
  chosen one is the largest by measured pixel area; an equal-area tie keeps the
  chunk that came first in file order. A container whose framing fails part way
  through keeps every chunk whose boundaries were proved before the failure, so
  a truncated file can still yield its preview.
- **unsupported** — the container parsed, holds at least one `level_N` chunk,
  and every such chunk is a recognized non-JPEG image format (`jpeg`'s
  `is_other_image`). This tool cannot read it, which is not the same as a damaged
  file and must not be counted as one.
- **invalid** — everything else: a container that does not parse, one whose
  chunks are neither complete JPEGs nor recognized images, and a file that
  cannot be read at all. "Unreadable" is this outcome, because the inventory is
  complete even when one file was not.

The planned filename is the full lowercase SHA-256 of the container's exact
cache-relative path — components joined with `/`, the cache root excluded —
behind a `legacy-preview-` prefix and a `.jpg` extension. The path is the
identity this mode has, so moving the whole cache or adding another container
beside it leaves every existing name alone, while two containers never share one
however identical their previews are.

Both modes report `plan::Progress` through the same callback: `Cache` per file
walked, and `Payloads` per container or indexed pyramid measured, with the total
known after the walk. A stop lands between two units and returns no partial plan.
The plan retains the chosen payload and its digest, and the reader rechecks the
bytes before returning them unmodified (`adr-0003-preserve-preview-bytes`). The
module never writes recovered files. Review can request the chosen payload
file's modification time via the filesystem seam. The lookup happens on
request and returns no timestamp when the file or its timestamp is unavailable.
All previews in a container use that container's timestamp. The browser review
check exercises the timestamp sort.
Standalone modern level files are not
candidates of the indexless mode: grouping and provenance for them is a separate
design question, and the walk reads containers only.
