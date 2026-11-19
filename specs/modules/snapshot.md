---
id: snapshot
type: module-design
title: "snapshot: a stable, read-only copy of the catalog"
status: draft
parent: architecture
depends-on:
  - fs
---

# snapshot

## Responsibility

Hand every other module a catalog file that cannot change under it: copy
`<catalog>` and its `-wal` / `-shm` sidecars into a private temporary directory
whose lifetime is the snapshot's, keeping the file name identical because SQLite
only discovers sidecars by name. When the caller asks for the proof, hash the
catalog before the copy and the copy after it, and hand back the pair — or fail.

## Boundary

The only module that copies a catalog file, and it copies through `fs` — the
copy, the digest, the scratch directory and the sidecar removals are that seam's
operations. It also owns the read-only opener that `catalog` and `previews_db`
share, so no other module assembles a SQLite URI by hand. It queries nothing —
`catalog` does — and it writes nothing outside its own temporary directory. It
exposes the copy's path; when the snapshot is dropped the copy goes with it, so a
caller that needs bytes beyond the snapshot's life has to read them in time. A
target whose `fs` cannot copy also cannot snapshot, and says so rather than
producing an empty copy.

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
- **A verified capture proves the copy is the catalog, at acquisition.**
  `capture_verified` hashes the catalog, copies it, hashes the copy and compares
  the two; a pair that disagrees is `Error::SourceChanged`, never a warning. The
  window is the acquisition because that is the run's only read of the user's
  file — everything afterwards reads our copy — so this is the failure that
  cannot be caused by a Lightroom session saving the live catalog an hour later.
  Both digests are taken **before** `checkpoint_copy`: the checkpoint folds the
  WAL into our copy and rewrites it, so a digest taken after it would compare our
  own work with the user's file and always disagree.
- **The catalog file is hashed; the sidecars are not.** `-wal` and `-shm` are
  copied because SQLite needs them and a session appending to the WAL is the case
  the snapshot exists to tolerate, so hashing them would refuse runs the rest of
  the module was built to accept. `sha256_file` is public so a surface that shows
  a digest computes it the same way this module does, not against a second
  implementation; the streaming itself is `fs::sha256_file`'s, and this function
  is the name that hands the hex back with `error::Error` context attached.
- **A benign mismatch is possible and is accepted.** A Lightroom checkpoint that
  lands inside the copy window changes the catalog's own bytes, so a copy that is
  perfect can still be refused: the pair is two observations of a file someone
  else may be writing. The window is short, the failure is loud and retryable,
  and the alternative — trusting a copy the run cannot prove is the catalog — is
  the claim this module exists to make. What the comparison never does is fail
  because of work that happened after the acquisition.
