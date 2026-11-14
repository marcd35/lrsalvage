---
id: adr-0006-degrade-without-guessing
type: architecture-design
title: "ADR 0006: Degraded runs report, never guess, and never write an unnamed file"
status: active
parent: architecture
references:
  - format-notes
  - adr-0002-map-via-previews-db
  - adr-0005-discover-payloads-by-index
---

# Degraded runs report, never guess, and never write an unnamed file

**Status:** accepted · **Date:** 2026-11-15

## Context

Three cache shapes arrive with no usable mapping, or with only part of one, and
Lightroom's own bookkeeping records nothing about why:

- **no index** — `previews.db` is missing, or present and unreadable;
- **pruned levels** — a row whose level file is gone, sometimes beside levels
  that survive;
- **mixed cache** — both representations of one pyramid on disk at once.

Over that sits the **orphan**: a payload no catalog image claims.

Measured on 2026-11-15, read-only, against the vault and a scratch cache:

- A modern cache reports `78` catalog images, `0` recovered, `78`
  `missing-payload`, **exit 3**. That is ADR 0005's reader not existing yet, not
  this decision.
- A cache with no `previews.db` prints
  `warning: <cache> has no previews.db; this cache holds no previews`, then dies
  with `sqlite error: unable to open database file` and **exit 1**. The warning
  also claims something nothing established: no index means no *mapping*, not no
  payloads.
- An unmapped payload is planned with no output path and `selected: false`, so
  `extract` writes nothing for it while the README says it is "listed with its
  payload path" — a path that only ever appears as prose inside a note.
- `superseded` is named in ADR 0004 and ADR 0005 and counted nowhere, with two
  incompatible readings in circulation: "the larger representation wins", or "any
  legacy file for a claimed pyramid".

## Decision

**The index is optional and never fatal. A degraded run reports what it can see,
names nothing it cannot prove, and writes no file it cannot name.**

1. **A missing `previews.db` is a mode, not an error.** ADR 0002 already says
   what to report (images as having no cache entry, payloads as orphaned, then
   stop); ADR 0005 ¶9 says how identity survives without rows. The run yields
   `no-cache-entry` for every image whose original is not on disk, one `unmapped`
   item per payload pyramid, **exit 3**, and one hint naming the file that is
   missing. The text never says "this cache holds no previews".
2. **An unreadable index degrades identically**, with the `sqlite` reason in the
   hint and the instruction to fix or move the file. Refusing the run instead
   would punish a damaged file sitting beside hundreds of readable payloads, and
   nothing is mapped either way, so ADR 0002's ban on smuggling a mapping in
   still holds.
3. **A `--previews` path that is not an existing directory is a usage error,
   exit 1.** A typo must not read as an empty cache: there is nothing at that
   path to salvage, which is a different fact from an empty one.
4. **An unmapped payload is never written.** It has no catalog image, so no
   original, no capture time, no rating and — with no index — not even a profile
   name; the only name available is the pyramid's own `(uuid, digest)` and the
   level's long edge. Writing that would add a second output namespace, a second
   selection rule, and a run that writes files while claiming nothing was
   selected.
   It is **listed** instead: path in `scan --verbose`, a `payload` member in
   `manifest.json`, and a trailing `payload` column in `report.csv`. No hint ever
   calls an orphan "recovered", "recoverable" or "salvage".
5. **`missing-payload` means no file of that pyramid at all** (ADR 0005 ¶5). A
   row whose level file is gone while another level survives is a **pruned
   level**: the item recovers from the largest level present and its note says
   the row is stale. A pruned level cannot be told from a file the user deleted —
   the row is the only claim and the file is absent either way — so nothing
   claims a cause; where a `DeletedPyramid` row exists the note reports
   Lightroom's own record of the deletion and the summary counts those
   separately. No new status.
6. **`superseded` is one stat, per pyramid, and never moves the exit code.** It
   counts **claimed pyramids with both representations**: at least one level file
   *and* at least one legacy `.lrprev`. The level files are the pyramid and the
   container is not read. A pyramid with only a `.lrprev` is legacy and is
   recovered from the container; one with neither is `missing-payload`; an
   unclaimed `.lrprev` is `unmapped`. Size never decides this (ADR 0005 ¶7). A
   superseded container coexists with the level files its pyramid is written
   from, so it can never be the only thing selected: a stat, in the summary only
   when non-zero, in `--json` and `manifest.json`, never in `report.csv`.
7. **A run that selected nothing may still write its report**, when it found
   orphan payloads: `report.csv` and `manifest.json` are written when
   `selected items > 0` **or** `unmapped > 0`, and a dry run writes none of them.
   Otherwise the one machine-readable list of orphans exists only in the run that
   never produces it.

**The exit-code vocabulary is unchanged, which is the point.** `0` success, `1`
usage or input error, `2` extract with failures, `3` nothing selected to do, `4`
unsafe destination. Every degraded path lands on one of them: `3` for a run that
found payloads it could not name, `1` for a path or catalog that cannot be used
at all. No sixth rule, and a corrupted index is not a usage error.

## Consequences

- An indexless or corrupt-index run produces a **report** rather than a stack
  trace, which is what makes it worth running at all: the payloads are listed,
  counted, and their paths handed over.
- `report.csv` gains a trailing `payload` column and `manifest.json` a `payload`
  member, so a script triaging orphans stops parsing prose. Append-only keeps
  positional readers working, and it is the same shape `payload_metadata` is
  already planned to take (ADR 0003). A reader that asserts exactly fourteen
  fields breaks — that is the cost, accepted deliberately.
- Three statements elsewhere were false and are corrected by this decision:
  `format-notes` §5.6 ("a stale row … looks like missing payload"),
  `previews-db`'s "opening a cache without one yields an empty index" (which the
  code has never done), and the README's promise that an unmapped payload is
  "listed with its payload path" when nothing printed that path.
- `README.md`'s compatibility line ("if the cache has no `previews.db`, it stops
  with a database error") is true of the shipped reader and becomes false the day
  the indexless mode lands. It changes with the code, not with this ADR.
- Coexistence has never been observed on this machine (`format-notes` §5.5): a
  mixed cache exists only as a synthetic fixture. `superseded` is a rule for a
  state, not a measurement of what Adobe writes.
- Exporting orphan payloads under their own identity stays a **separate**
  decision — it needs its own naming, metadata and exit-code answer, and it is
  recorded as fog on the map rather than smuggled in here.

## Considered options

- **Refuse to run without an index** (exit 1). Simple and honest about the
  missing mapping, but it hands a user nothing while hundreds of readable
  payloads sit in the cache, and it makes "no version refuses a run"
  (ADR 0004) a half-promise.
- **Write orphans into `_unmapped/<uuid>-<digest>_<longedge>.jpg`.** The name is
  measured evidence, so ADR 0001's ban on guessing is not broken; but it needs a
  second naming rule, a metadata policy for a payload with no catalog facts, and
  it turns a run that named nothing into exit 0. A feature, not a degraded path.
- **Let the larger representation win.** It reintroduces ADR 0001's silent
  failure in a new place: a re-rendered container that happens to be larger
  would beat the level files the model says are the pyramid.
- **A `superseded` status.** It is not a per-image outcome — the image is
  recovered, from its level files — and adding it would break the partition that
  makes the summary add up.
- **Write report files on every run** (selection or not). Convenient, and it
  makes cli.md's "nothing selected, nothing to report on" rule meaningless; the
  narrow `unmapped > 0` trigger keeps litter out of the ordinary empty run.
