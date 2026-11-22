---
id: cache-recovery
type: module-design
title: "cache_recovery: a JPEG per indexed pyramid without a catalog"
status: draft
parent: architecture
depends-on: [error, fs, jpeg, lrprev, model, plan, previews-db]
references: [adr-0001-parse-dont-carve, adr-0002-map-via-previews-db, adr-0003-preserve-preview-bytes]
---

# cache_recovery

Require a readable preview index and make one plan item per distinct indexed
pyramid. The positive evidence is a row in Pyramid, PyramidLevel, or
ImageCacheEntry. A tombstone or a file on disk alone adds no item. The index
reader opens a private copy including SQLite sidecars and distinguishes a
missing, unreadable, or unsupported index.

Rank complete JPEGs by measured SOF pixel area. Equal sizes prefer the modern
level file, then the cache-relative path and the legacy chunk order. Only
consider files belonging to the exact indexed identity. Read legacy chunks by
their declared boundaries. The intact prefix of a damaged container can still
supply certified JPEGs. The scan reports one outcome per indexed pyramid:
recoverable, invalid, unsupported, or missing, in that order. A recoverable
pyramid flags a larger unavailable level or a dimension disagreement when the
index or payload header provides that evidence.

The planned filename is the full lowercase SHA-256 of the UUID, a zero byte,
and the digest as stored, with a `preview-` prefix and a `.jpg` extension. The
plan retains a reference to the chosen payload and its digest. The reader
rechecks it before returning the unmodified bytes to a front end. A stopped
scan returns no partial plan. The module never writes recovered files.
