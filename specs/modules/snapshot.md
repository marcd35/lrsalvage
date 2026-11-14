---
id: snapshot
type: module-design
title: "snapshot: a stable, read-only copy of the catalog"
status: draft
parent: architecture
---

# snapshot

## Responsibility

Hand every other module a catalog file that cannot change under it: copy
`<catalog>` and its `-wal` / `-shm` sidecars into a private temporary directory
whose lifetime is the snapshot's, keeping the file name identical because SQLite
only discovers sidecars by name.

## Boundary

The only module that copies a catalog file. It also owns the read-only opener
that `catalog` and `previews_db` share, so no other module assembles a SQLite URI
by hand. It queries nothing — `catalog` does — and it writes nothing outside its
own temporary directory. It exposes the copy's path; when the snapshot is dropped
the copy goes with it, so a caller that needs bytes beyond the snapshot's life
has to read them in time.

## Decisions

- **Copy the whole trio.** Lightroom keeps catalogs in WAL mode: a live
  read-only open can observe a half-applied transaction, and `immutable=1` alone
  would ignore the WAL and return stale data. Copying and checkpointing is the
  only option that is both safe and correct.
- **Checkpoint our own copy.** Writing to the copy is safe precisely because it
  is ours. A `wal_checkpoint(TRUNCATE)` folds the WAL in and the sidecars are
  removed; a failed checkpoint is best effort, not an error — a copy that cannot
  be checkpointed is still readable through its WAL, and refusing the snapshot
  would be worse than returning a usable one.
- **`immutable=1` only when it is true.** With a `-wal` still next to the copy,
  `immutable=1` would silently hide committed rows, so the opener falls back to
  a plain read-only URI and replays the WAL. This was measured, not assumed.
- **Read-only, always.** `mode=ro` on every open; the caller's file is never
  handed to SQLite at all, so a Lightroom session left running cannot be affected
  by a salvage run.
