---
id: app
type: module-design
title: "app: the browser page's entry and scan"
status: draft
parent: architecture
depends-on: [catalog, previews-db, plan, snapshot, fs, error]
references: [adr-0012-two-release-lines, browser-spike, glossary]
---

# app

`crates/lrsalvage-app` — the shipped page (`0.1.0`, ADR 0012 D10) and the one
entry a run goes through. It is not a module of the pipeline: it owns no format,
no status and no naming rule, and every number it shows was observed by a module
with a spec of its own.

## Responsibility

Take a source — a folder holding a Lightroom catalog and its preview cache, and
any further folder a person hands over — and produce **one report**: the copy's
two digests, the index's condition, the cache's facts, one status per catalog
image, and the root folders the catalog recorded. Show it moving while it works,
let it be stopped, and say which of the ways a scan can come back empty happened.

The page is also the entry's *error surface*: a failure is a report
(`{ok: false, problem: {stage, kind, message}}`), never an exception, so a person
gets a sentence about the stage that failed and a page has one code path.

## Boundary

`src/lib.rs` asks `snapshot`, `catalog`, `previews_db` and `plan` in that order,
and nothing else:

```text
capture_verified ─► from_snapshot ─► index copy + open ─► build_reported ─► report
   (the copy)         (the open)        (the index)          (the scan)
```

- **The private copy is the snapshot.** `snapshot::capture_verified` carries the
  catalog and its `-wal`/`-shm` into the origin-private pool, opens it with
  `PRAGMA locking_mode=EXCLUSIVE` before the first WAL access and checkpoints —
  the measured 61 ms path that read 128 of 128 images where a checked import read
  78 (`browser-spike` §2). So `--verify-source`'s two digests are the same two
  digests the CLI reports, and the page prints them.
- **The index is copied where SQLite can open it.** The pool is the only VFS on
  this target, so `previews.db` is copied into the pool like the catalog is; an
  absent index is *not* copied, and `previews_db::open` answers `Missing`, which
  is a mode rather than a failure (`adr-0006-degrade-without-guessing`).
- **No destination, and therefore no write.** The build is given a placeholder
  output root that no mount answers for, because `plan` needs one to number
  output paths and this slice writes nothing. A picked destination is the save
  slice's, and it re-plans.
- **The scan reports and stops** through `plan::Progress` and `plan::Control`
  (`specs/modules/plan.md`, *Decisions*). The page never re-derives a count.

`src/wasm.rs` is the only file that mentions JavaScript: it declares the two host
calls (the seam's `fs_call` and `scan_progress`) and exports `start_install`,
`poll_install` and `scan`. `web/` holds the page and the seam's page-side
JavaScript. `src/vault.rs` and `src/bin/vault-app.rs` are native-only and compose
the vaults the browser run needs, from `lrsalvage-fixtures`.

`crates/lrsalvage-web` links this crate — harness depends on app, never the
reverse — so the seam is registered one way for both pages.

## Decisions

- **The entry unit is the catalog's folder, not the catalog file.** `FileSystemHandle`
  exposes a name and a kind and no path, so a picked *file* cannot reach its
  siblings: the `-wal` SQLite needs and the cache beside it are both unreachable
  from it. One `showDirectoryPicker` for the backup folder, from which the page
  reads `<name>.lrcat` and `<name> Previews.lrdata`; a split backup is a second
  folder pick. This corrects #117's "pick the catalog with a file picker", and the
  correction is forced by the API rather than by taste (`#149`, DEC-S1).
- **The source is a value, and the picker is one way to make one.** `start(source)`
  takes mount paths and the handles that answer for them; the pickers, and the
  page's own reading of the folder, are what a click makes of them. That is what
  makes the flow testable at all: no automation we have can answer a File System
  Access dialog (`scripts/browser-app.sh`'s header records the measurement), so a
  browser test supplies the value a pick returns — an origin-private
  `FileSystemDirectoryHandle` — and drives everything after it for real.
- **A stop is memory, not a message.** The scan runs inside a dedicated worker
  behind a synchronous wasm frame, so a `postMessage` cannot reach it until it
  ends. The page writes a flag into a four-byte `SharedArrayBuffer` and the
  worker's `scan_progress` host call reads it with `Atomics.load` on every unit —
  the same mechanism the seam's bridge uses, for the same reason
  (`specs/modules/fs.md`, *The bridge*). A stop therefore lands on a unit
  boundary: one cache file, one level file, or one catalog image.
- **Progress is reported per unit and painted at the host's rate.** The Rust half
  calls `scan_progress` for every unit; the worker posts to the page at most ten
  times a second, and always on a stage change and on a stage's last unit. A
  100,000-image catalog crosses the boundary that many times and a DOM update does
  not cost what a `u64` does (`specs/modules/plan.md`).
- **The stages that are one call are named, not pretended to move.** The copy, the
  index copy and the SQLite open have no unit inside them, so the page says which
  stage it is in and offers no stop for it. The measured copy is 6–10 s per GiB
  (`browser-spike` §2), which is why it is named.
- **Every condition is a count, and "nothing found" is never the message.** Absent
  index, unreadable index (with SQLite's own words), an index with no payload
  files, payload-like files matching no shape, images with no index row, rows with
  no payload, damaged payloads, payloads no image claims — each is one sentence
  with the number behind it, and "what was checked" is a list of the facts the
  report carries. A page that said "no previews found" for all of them would have
  failed #117's user stories 4 and 5.
- **The recheck mounts the folder where the catalog says it was.** A recorded root
  is `AgLibraryRootFolder.absolutePath`'s own spelling
  (`format-notes` §6.1: POSIX separators even on Windows), so a picked folder
  mounted at that path makes the seam's namespace answer the question
  `plan` already asks — is the original still there? — and the next scan reports
  those images as `original-present`. The app is stateless between runs, so the
  recheck pays one more copy; that is the trade for an entry that is one function
  in and one report out (`#149`, DEC-S7).
- **No framework and no build step.** Plain HTML, CSS and ES modules, served
  cross-origin isolated because the bridge's `SharedArrayBuffer` needs it. A
  browser without `showDirectoryPicker` is told so, and pointed at the CLI, before
  it is offered anything.
- **The page has no test hook in it.** The harness's page (`web/test.html`,
  `web/test.js`) is a second page that imports the same functions, and the product
  page's own `addEventListener` lines are the only part it does not exercise —
  which is recorded here rather than implied.

## Testing

- `crates/lrsalvage-app/tests/scan.rs` is the gate: one report per condition over
  the seven vaults, a stopped build with no plan, and an indexless cache still
  counting its payloads. It composes `lrsalvage-fixtures` vaults through
  `src/vault.rs`, so no fixture bytes live in this crate.
- `scripts/browser-app.sh` is the measurement: the page's own entry in headless
  Chromium against the same seven vaults, compared field by field with a native
  run — the digests included — plus a stop, the recheck, a cancelled picker driven
  through a real user gesture, and the capability gate. It is not a gate, and
  `specs/modules/fs.md` (*Where the harness lives*) says why.
- What the browser run cannot prove, and does not claim: that a human's accepted
  pick returns a handle. The mechanism is the browser's, one `await` deep, and the
  value it returns is the value the test supplies.
