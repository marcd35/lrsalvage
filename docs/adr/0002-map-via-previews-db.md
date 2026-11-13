# Map via `previews.db`, never re-derive hashes

**Status:** accepted · **Date:** 2026-11-12

## Context

A payload's file name is `<UUID>-<digest>.lrprev`, and `previews.db` stores that
pair against `Adobe_images.id_local`. Two tempting shortcuts exist: hash the
original path or file name to reconstruct the pair, or scan the cache for UUIDs
and guess which image each one belongs to. Neither is documented by Adobe, both
produce mappings that look right, and neither can be verified — there is nothing
to check the guess against except the guess.

The stakes are asymmetric. A missing mapping costs one recoverable image. A wrong
mapping writes someone else's photograph under a name they will trust, and they
may delete the cache before noticing.

## Decision

The `(uuid, digest)` rows in `previews.db` are the only source of image → payload
mapping. Look the path up from the row and use it verbatim. When the database is
missing or unreadable, report the images as having no cache entry and the payloads
as orphaned, then stop. No hashing, no globbing, no nearest-neighbour fallback.

## Consequences

- Every recovered file corresponds to a row Adobe wrote. False mappings are
  impossible by construction, not by testing.
- A catalog whose `previews.db` is lost cannot be used to name files. The payloads
  can still be listed as orphans, but not claimed.
- Stale rows after a 13.3+ upgrade surface as `missing-payload`. That is honest:
  the catalog promised a file that is not there, and we say so instead of
  inventing a replacement mapping.
- Recovery of a catalog with a corrupt database needs a different, explicitly
  unsupported mode. It must not be smuggled into the normal path.
- Querying the database is cheap. The expensive part of a run is reading and
  writing payload bytes, not resolving names.

## Considered options

- **Reconstruct keys by hashing.** Unverifiable, and it assumes a hash input we
  have never seen documented.
- **Match by UUID prefix only.** Two images can share a prefix, and the digest is
  what disambiguates them; dropping it turns a unique key into a guess.
- **Pick the newest payload in a folder.** Ordering by mtime is a heuristic about
  Lightroom's behaviour, not a fact about the data.
