---
id: browser-spike
type: architecture-design
title: "The browser route, measured: spike #126"
status: active
parent: architecture
tags:
  - reference
  - distribution
---

# The browser route, measured: spike #126

> Everything below was measured on one Windows machine in November 2026 against
> this repository's own modules. The labels are
> [app-distribution](app-distribution.md)'s, which owns this project's platform
> facts: **measured** (a run on this machine — the run-shaped form of the evidence
> standard's *verified*), **verified** (a vendor, registry or specification
> statement, with the date it was read), **reported** (a third party said so and
> nobody here confirmed it), **unverified** (a working hypothesis), **TODO** (nobody
> has looked yet). A claim that is none of those is a bug.
>
> [format-notes](format-notes.md) keeps its own meaning for *verified* — observed
> directly on real catalog or cache files here — because a Lightroom claim and a
> platform claim are checked with different instruments. A Lightroom byte claim in
> this page therefore cites format-notes rather than labelling itself.

[ADR 0012](adr/0012-two-release-lines.md) D10 makes the browser-or-desktop
question a rubric, and [app-distribution](app-distribution.md) §4 holds the
constraints it was written against. This page holds what a throwaway harness then
measured: `lrsalvage-core`'s SQLite dependency compiled to `wasm32`, this
repository's `catalog` and `previews_db` modules run against a real catalog in a
browser, payloads written into a folder a person picked, and the storage a page is
offered. The first reading was **No-go**, on §3's sequential write rate and on
§2's dropped `-wal`. The three experiments in §6
were run, and they moved both rows a long way. The `-wal` now recovers **to exact
parity** (§2): 128 of 128 images, every digest equal to a native run. Writes with
8–16 streams in flight reach **27–29 files/s** (§3) — the durable-write speed of
the writer this project ships, and 3.5–8× the sequential rate that produced the
first verdict.

What that does *not* do is turn the pre-registered gate green. The write row was
scored before measuring against per-file latency — median ≤4 ms, p95 ≤20 ms, no
stall >1 s, red above a 20 ms median — and a stream pool deliberately trades
per-file latency for throughput: 34.1 s for 1 000 files with a 542 ms median. By
the letter that is **red**; by the failure it was written to catch — a run that
looks hung — it is not, because no gap between completed files exceeded 570 ms.
The gate is therefore *revised*, explicitly and with its reason, in
[ADR 0012](adr/0012-two-release-lines.md) D10's resolution, and the revision's own
criterion (5 000 files in 120 s) is **not yet met**: 2.8 minutes by projection,
with cancellation and resume never exercised. What remains beyond the writes is
§7's list of costs and unknowns — Chromium-only, quota, a 149.7 s stall after a
reload — not a wall.

The audience test was the release gate when the spike closed. The dated
[ADR 0012](adr/0012-two-release-lines.md) D10 decision now owns the
maintainer recovery gate and the use of the writer timing target. The
measurements below describe the spike.

The harness lived outside the repository, included the modules through `#[path]`
rather than transcribing their SQL, and ran a native binary over the same code so
the two could be compared digit for digit. Nothing in this repository's crates
changed to produce these numbers.

## Environment

| Item | Value | Evidence |
|---|---|---|
| Operating system | Windows, one machine, Windows Defender active | measured |
| Chrome | 157.0.8103.79 | measured |
| Edge | 157.0.4341.60 | measured |
| Rust | `rustc` 1.100.0, target `wasm32-unknown-unknown` | measured |
| C toolchain | wasi-sdk 34.0, clang 23.1.0 — `sqlite-wasm-rs` compiles the SQLite amalgamation with `cc`, so a wasm build of `rusqlite` needs one | verified (the crate's `build.rs`) |
| Bridge | `wasm-bindgen` 0.2.130, `--target web` | measured |
| Artifact | 1 501 766 bytes for the SQLite link plus the harness | measured |
| Cold load | 188–448 ms Chrome, 1 346 ms Edge — module fetch, instantiate, first response | measured |

Data: the real catalog pair on the machine that produced `format-notes` — a
2 617 344-byte catalog and a 290 816-byte `previews.db`, 78 images — and a
100 078-image catalog (42 770 432 bytes) with an 83 304 480-byte index, built by
appending rows to copies of the same schemas. A third copy carries a 70 072-byte
`-wal` sidecar holding its newest 50 committed images, verified by reading 128
images with the sidecar and 78 without — the byte-level fact is
[format-notes](format-notes.md) §7's, and this page measures only what a browser
does with it.

## 1. The SQLite path to `wasm32`

**`rusqlite` is not a dead end. A *writable* VFS over a picked file is.**

`rusqlite 0.42` — the workspace's pin — builds for `wasm32-unknown-unknown` with
no change to this repository's code: on that target its **default**
`ffi-sqlite-wasm-rs` feature links `sqlite-wasm-rs 0.6.3`, which compiles the
amalgamation with clang. The VFS is `sqlite-wasm-vfs 0.3`'s `sahpool`: an
origin-private-filesystem store, in a dedicated worker, and the only persistent
VFS that crate ships.

**Reading a picked file in place is possible, and is not exotic.** The File API
exposes `FileReaderSync` to workers — "Web Workers allow for the use of
synchronous File or Blob read APIs, since such reads on threads do not block the
main thread", with `ArrayBuffer readAsArrayBuffer(Blob)` and `[Exposed =
(DedicatedWorker, SharedWorker)]` (File API §6.5, read 2026-11-18). A `File` from
a picked directory handle slices and reads synchronously there, which is all
`xRead` needs, and two shipped implementations are exactly that shape: Emscripten's
`WORKERFS`, and SQLite Explorer's blob VFS, ~276 lines covering several backends,
whose author reports opening a 2.5 GB database in under a second (**reported**).

What cannot exist is the **writable** version.
`FileSystemFileHandle.createSyncAccessHandle()` is specified as usable only "for
files within the origin private file system" (MDN and Chrome's own guide; verified
2026-11-18), and `sqlite-wasm-rs` ships no async VFS either. So a page can read a
picked catalog synchronously but must write through the async `createWritable()`
stream, and any VFS that has to write — journal, WAL recovery, checkpoint — cannot
live on a picked file at all.

What did hold: **the rows survive untouched**, on both databases and at both
scales.

| | real pair | 100 078-image pair |
|---|---|---|
| images | 78 | 100 078 |
| pyramids | 78 | 100 078 |
| derived level paths | 390 | 300 390 |
| image fingerprint vs native | identical | identical |
| container and level-path fingerprints vs native | identical once `MAIN_SEPARATOR` is normalised | identical once normalised |

**`previews_db` compiles untouched and does not run untouched.** Its `open()` gate
is `db.exists()`, and `Path::exists` cannot be answered on
`wasm32-unknown-unknown`: the unmodified module reports `Missing`, zero entries and
a tree walk of zero pyramids against a database that is present and readable.
Replacing that one predicate — one line, the rest of the file byte-identical — and
the module reads everything the desktop reads. Its other filesystem touchpoints are
the `walkdir` sweep and `jpeg::is_payload_start`'s `File::open`; `snapshot`'s copy,
hash and checkpoint are all `std::fs` as well. Those four are the seam an app-line
front end has to own.

**The one content difference is the separator**, and it matters:
`std::path::MAIN_SEPARATOR` is `/` on wasm and `\` on Windows, so every derived
path string differs between the two targets. ADR 0012 D12's byte-for-byte app/CLI
parity fails as written on a wasm build unless output naming normalises first.

## 2. Reading a catalog

| Measure | real pair | 100 078-image pair | Evidence |
|---|---|---|---|
| Import into the pool | 20.5 + 5.6 ms Chrome · 49.9 + 9.1 ms Edge | 285 + 748 ms Chrome · 328 + 599 ms Edge | measured |
| Import throughput | — | 111–170 MB/s | measured |
| `catalog`'s query | 4 ms | 739 ms Chrome · 903 ms Edge (native 447 ms) | measured |
| `previews_db`'s query | 9 ms | 1 933 ms Chrome · 3 560 ms Edge (native 664 ms) | measured |
| Whole round trip | 79 ms | 10.0 s Chrome, 13.4 s Edge — and 5.9 / 8.5 s of that is the harness fingerprinting, not the modules | measured |
| OPFS copy of 1 GiB | 6.0–10.4 s Chrome (103–178 MB/s), 12.6 s Edge (85.5 MB/s) | | measured |

**A catalog opens in a time a person tolerates**, and a 100k-image catalog is
under a second of query time for the catalog and under 4 s for the index. The copy
that the OPFS route forces is seconds, not minutes — and §1's `FileReaderSync`
route would delete the copy altogether once the FFI is plumbed, at the cost of one
slice per page read.

**The `-wal` row was the import, not the platform — and the fix is measured.** The
first import, `import_db`, has a *checked* mode that "resets the header's
read/write versions to rollback mode", and it carried the main file only. Measured
that way: **78 images read, 128 present**, no error and no hint. Two facts say that
is a choice rather than a wall:

- **A single owner does not need the `-shm`.** "Beginning in SQLite version 3.7.4
  (2010-12-07), WAL databases can be created, read, and written even if shared
  memory is unavailable as long as the locking_mode is set to EXCLUSIVE before the
  first attempted access" — the wal-index moves to heap memory. Since 3.22.0 a
  read-only WAL database opens too, when the sidecars exist or the parent is
  writable (sqlite.org/wal.html, read 2026-11-18).
- **The bytes can be carried.** `rsqlite-vfs 0.2`'s `import_db_unchecked` "Skips
  header validation" and does not reset versions; the pool recognises `-wal` and
  `-journal` companions, and a *checked* import whose companion is present fails
  with `RecoveryRequired` — an explicit signal, not silence.

So the pool was given the main file and its `-wal` unchecked, opened with
`PRAGMA locking_mode=EXCLUSIVE` as the first statement, checkpointed, switched to
`journal_mode=DELETE`, and closed. **128 of 128 images came back, and every digest
matched a native run of the same pair exactly** — 61 ms of setup, no `-shm`
supplied, and the pool's `-wal` slot gone afterwards.

| | main + `-wal`, recovered | main only, checked import | native reference |
|---|---|---|---|
| images | **128** | 78 | 128 |
| pyramids / level rows / index entries | 78 / 390 / 78 | 78 / 390 / 78 | 78 / 390 / 78 |
| `images_fields_sha256` | `440129e3…` | `87f6b85e…` | `440129e3…` |
| `images_sha256` (separator-normalised) | `6e9ed5e3…` | `9bb1b73e…` | `6e9ed5e3…` |
| level paths / containers (normalised) | `70996c19…` / `c24780a2…` | same | `70996c19…` / `c24780a2…` |
| `xmp_sha256` | `37968eb5…` | same | `37968eb5…` |

Two limits on that result. The recovered database is a private copy that has been
made writable, so this is the desktop's `snapshot` behaviour reproduced in OPFS,
not a read of the user's file. And a *live* pair being written by Lightroom while
it is copied is a separate snapshot-consistency question that neither the desktop
nor the browser answers by itself (ADR 0001, `snapshot`'s verified acquisition).

## 3. Writing payloads

Two payload sizes, `createWritable()` + `close()` per file, times per file:
417 019 bytes (a large real level file) and 28 059 bytes (the median real level
file).

| Target | Payload | Count | Total | p50 | p95 | Max | Rate | Evidence |
|---|---|---|---|---|---|---|---|---|
| Picked folder | 417 019 B | 1 000 | 286.0 s | 148.6 ms | 842.5 ms | 3 037 ms | 3.5/s | measured |
| Picked folder | 28 059 B | 1 000 | 545.6 s | 286.7 ms | 1 366.4 ms | **26 269 ms** | 1.8/s | measured |
| Picked folder, fresh folder re-check | 28 059 B | 200 | 61.1 s | 265.6 ms | 586.5 ms | 901 ms | 3.3/s | measured |
| OPFS, same call pattern | 417 019 B | 1 000 | 34.2 s | 27.4 ms | 71.1 ms | 565 ms | 29.3/s | measured |
| Native, plain write | 417 019 B | 1 000 | 20.3 s | 9.1 ms | 61.0 ms | 1 651 ms | 49/s | measured |
| Native, write + `fsync` | 417 019 B | 1 000 | 44.2 s | 36.2 ms | 101.9 ms | 278 ms | 23/s | measured |
| Native, plain write | 28 059 B | 1 000 | 9.3 s | 4.3 ms | 29.8 ms | — | 108/s | measured |

**Sequential, this reads as a hang; concurrent, it does not.** Every run in the
table above awaited each write and its close before starting the next. Keeping eight
to sixteen streams in flight on distinct files — which the standard allows, and
which VS Code's folder download and LinkUnzip both do — changes the shape:

| Streams in flight | Files | Total | Rate | p50 | p95 | Longest gap with no file |
|---|---|---|---|---|---|---|
| 1 | 128 | 15.8 s | 8.1/s | 122.7 ms | 144.3 ms | 174 ms |
| 2 | 128 | 8.8 s | 14.5/s | 137.0 ms | 158.3 ms | 144 ms |
| 4 | 128 | 5.9 s | 21.7/s | 180.0 ms | 213.7 ms | 165 ms |
| 8 | 128 | 4.7 s | 27.1/s | 288.1 ms | 346.1 ms | 264 ms |
| 16 | 128 | 4.5 s | **28.3/s** | 550.3 ms | 655.0 ms | 507 ms |
| 32 | 128 | 4.6 s | 28.0/s | 1 116.9 ms | 1 284.3 ms | 935 ms |
| 64 | 128 | 4.7 s | 27.3/s | 2 368.4 ms | 2 404.4 ms | 2 144 ms |
| 16 | 1 000 | 34.1 s | **29.4/s** | 542.1 ms | 619.0 ms | **570 ms** |
| 16 | 200 × 417 KB | 7.5 s | 26.6/s | 587.8 ms | 667.7 ms | 595 ms |
| OPFS control, 1 stream | 128 | 1.1 s | 120.3/s | 8.0 ms | 10.2 ms | 14 ms |
| OPFS control, 64 streams | 128 | 0.9 s | 141.7/s | 464.6 ms | 469.0 ms | 369 ms |

Three things fall out. **Throughput saturates at about 28 files/s** between 8 and
16 streams; more streams buy latency, not rate, and at 64 streams the per-file p50
reaches 2.4 s with a 2.1 s gap between completions — the number that would read as
a hang. **The payload size barely matters** (26.6/s at 417 KB against 29.4/s at
28 KB), which is what a per-file, per-close cost predicts. And the same JavaScript
into OPFS is four to five times faster, so the cost is the external commit:
Chromium's safe-move path hashes the content and requests after-write checks on
close, and Windows can then attach a zone identifier (verified in the 157.0.8103.79
sources), none of which the standard lets anyone batch.

**The right native baseline is the one that makes the browser look equal.** The
shipped `extract` writer calls `sync_all` and then `rename` per file; a bare native
write does neither. Against the bare write the browser is 2–4× slower (29.4/s
against 108/s for 28 KB). Against the writer the app actually ships it is at parity
at the payload size that was measured both ways: **26.6/s at 417 KB against 23/s
for a native write plus `fsync`**. The 28 KB native `fsync` rate was not measured,
so no parity claim is made for it. The 5 000-file shape is 2.8 minutes by
projection (34.1 s × 5) against a proposed product gate of 2 minutes — over, not
absurd.

**Scored against the gate agreed before measuring.** The thresholds were written
down first, and they are reported here whether or not the browser survives them:

| Pre-registered threshold | Result | Verdict |
|---|---|---|
| real catalog open and query ≤2 s (red >5 s) | 4 + 9 ms | **green** |
| 100 078-image catalog query ≤10 s (red >30 s) | 0.74 + 1.93 s | **green** |
| OPFS copy ~1 GB ≥50 MB/s (red <10) | 86–178 MB/s | **green** |
| 1 000 writes: median ≤4 ms, p95 ≤20 ms, no stall >1 s | 542 / 619 ms median and p95, no gap over 570 ms | **red on the median** (the rule is red above 20 ms); the stall condition passes |
| quota ≥5× the largest catalog | 10.7 GB against a 2.6 MB catalog | **green** |

So the honest score is one red row and four green, and the red row is the one whose
*shape* the replacement gate changed after the fact. A pre-registered gate that is
revised after seeing the data is the move this repository treats as a bug unless
the revision is recorded with its reason — which is why ADR 0012 D10 now carries
both the original number and the revision, and why the revision's own threshold
(5 000 committed files in ≤120 s, no gap over 1 s, zero wrong or missing outputs,
safe cancel and resume) is stated as **not met** rather than quietly dropped.

**And the sink is not stable across sessions.** The same sequential 1 000 × 417 KB
run took 140.4 s (7.1/s) on the day of the sweep and 286.0 s (3.5/s) in the first
session, the worst single file falling from 3 037 ms to 1 018 ms; the sweep's
one-stream level ran at 8.1/s. Free space, the destination's existing entry count
and the anti-malware scanner's state all changed between the two, and nothing here
separates them. The gain from concurrency is 3.5× over that day's own sequential
rate and 8× over the first session's, so both framings are true and the honest one
is a range.

One write finding helps: **an aborted stream leaves the file untouched.** Writing
512 KB over an existing 1 048 576-byte file and aborting left exactly
1 048 576 bytes and no leftover directory entry, matching the File System
standard's "the file will either contain its old contents or the new contents".
`extract`'s temp-and-rename is already provided by the platform there — for
*replacement*; creating a new destination handle can still leave an empty entry
behind when the stream that follows aborts.

Tiles are affordable: 200 decodes of ~0.6-megapixel payloads took p50 21.5 ms and
p95 27.0 ms (Chrome), 23.6 and 40.7 ms (Edge), with no failures.

## 4. Storage and grants

| Measure | Value | Evidence |
|---|---|---|
| Quota on a fresh profile | 10 745 809 016 bytes | measured |
| Quota after 1.29 GB of usage | 12 023 499 034 bytes | measured |
| Quota after a restart | 11 154 611 675 bytes | measured |
| `persist()` at first load | **refused**, `persisted()` false before and after | measured |
| `persisted()` after a full browser restart | **true**, with the OPFS entries intact | measured |
| Folder grant at pick time | `granted`; no three-way "allow on every visit" prompt appeared | measured |
| Folder grant after a page reload | `granted` | measured |
| Folder grant after a browser restart | **`prompt`** — the user must re-authorise, from a gesture | measured |
| Pool install after a page reload | **149.7 s** while the previous worker released the OPFS directory | measured |

The quota is not 60% of the disk and it moves between sessions, so a plan that
sizes itself against a published percentage would be guessing. Persistence is
granted late, if at all: the data survived a restart while the promise of
protection had been refused when it was written.

## 5. A downloaded folder is not a static export

A page opened from `file://` is a secure context, and `showDirectoryPicker()`
reaches its dialog there. But `navigator.storage.getDirectory()` is refused
("unsafe for access within a Web application"), importing the wasm module is
blocked, and fetching the `.wasm` fails. A downloaded folder therefore cannot use
the OPFS route and cannot load a multi-file export at all. Only a single-file
build with the wasm inlined would run — and then only the direct-paging route §1
rules out for these modules.

## 6. The two experiments, and what is left

Both experiments §6 named the first time this page was written were run on the day
it was written. What they returned is in §2 and §3; what they did not reach is
here.

1. **Bounded-concurrency writes — done, and it moved the number 3.5–8×.** Chrome
   only, one run per setting, 128 files per level plus 1 000 files and 200 large
   payloads at the winning setting, no byte cap needed at these payload sizes, no
   cancellation or resume exercised, and no nested mixed-size tree. The plateau is
   measured; a second machine, Edge, a repeat run and a mixed tree are not.
2. **The `-wal` pair through this repository's Rust path — done, exact parity.**
   Not yet exercised: a truncated or uncommitted WAL tail, a stale companion whose
   `-shm` disagrees, an already-clean backup, a quota interruption mid-recovery,
   and a pair captured while Lightroom is writing.
3. **Direct paging over `FileReaderSync` — not attempted.** It cannot change §3,
   because reads are not the constraint; it would only remove the copy and the
   quota, and with them the 149.7 s pool-release stall in §4. Worth one afternoon
   if the swarm ever reaches the copy.

The header of this page still holds: one machine, one disk, one anti-malware
configuration, one browser profile. §3's session-to-session variance is the
clearest reminder of how much rides on that.

## 7. What this does not decide

- **The audience.** D10's test — *do users come out of this with previews worth
  keeping?* — is now the deciding question, because the two mechanics that could
  have answered it *no* have been measured back to *yes*.
- **Chromium only.** `showDirectoryPicker` is absent from Firefox and Safari and
  one vendor has filed a negative standards position. A browser path is a Windows
  and macOS-Chromium promise, and macOS was never run.
- **Storage the page does not own.** The quota moved 10.7 → 12.0 → 11.2 GB across
  sessions; `persist()` was refused at the first load and true only after a restart;
  a folder grant came back as `prompt` after a restart. None of that blocks a
  recovery run, all of it has to be designed around.
- **The 149.7 s stall.** Installing the pool after a reload waited that long for the
  previous worker to release the OPFS directory. It is the worst user-visible
  number in this page and it is not yet explained, let alone fixed.
- **The seams an app front end has to own.** `previews_db` needs `Path::exists`
  answered, `snapshot` needs a browser copy and checkpoint, and `MAIN_SEPARATOR`
  differs. ADR 0012 D12's byte-for-byte parity needs the separator normalised
  before it can hold on a wasm build. None of the three is unowned now: the
  crate's `fs` seam holds every filesystem operation behind one signature per
  target, its `wasm32` half answers from an origin-private store, a picked
  directory and a picked file ([the seam's spec](../specs/modules/fs.md)), and
  `scripts/browser-parity.sh` holds that half to the desktop's counts, statuses
  and JPEG bytes with the separator normalised. What was still owed is the page,
  and the first slice of it has landed: `#149` built the entry and the scan —
  folder picks, the catalog and its `-wal` copied into the store, progress, a stop,
  the conditions that make a scan empty, and a recheck of the originals
  ([the app's spec](../specs/app.md), and `scripts/browser-app.sh` for what it
  measured). What remains of the page is the grid and the saves, which are #117's
  later slices — this page measures the costs, not the plan for them.
- **Which artifact ships.** A native shell writes through the operating system at
  23–108 files/s with no picker, no quota and no `-wal` question. The browser now
  writes at 27–29 files/s with no installer, no signing and a macOS route that does
  not need a Mac to sign on. That is a trade-off between two working options rather
  than a choice between one working option and one that fails.

## Sources

- File API §6.5 "Reading on Threads" and §6.5.1 `FileReaderSync` — synchronous
  `readAsArrayBuffer`, `[Exposed = (DedicatedWorker, SharedWorker)]` — W3C, read
  2026-11-18. Verified.
- `FileSystemFileHandle.createSyncAccessHandle()` — "only usable inside dedicated
  Web Workers for files within the origin private file system" — MDN, read
  2026-11-18. Verified.
- `sqlite.org/wal.html` — "can be created, read, and written even if shared memory
  is unavailable as long as the locking_mode is set to EXCLUSIVE before the first
  attempted access" (3.7.4+); read-only WAL opens when the sidecars exist or the
  parent is writable (3.22.0+) — read 2026-11-18. Verified.
- File System standard, `createWritable()` — "user agents try to ensure that no
  partial writes happen" — read 2026-11-18. Verified.
- `rusqlite 0.42.0` manifest — `sqlite-wasm-rs >=0.6.3,<0.7` for
  `wasm32-unknown-unknown`, with `ffi-sqlite-wasm-rs` in the default features —
  crates.io, read 2026-11-18. Verified in the published tarball.
- `rsqlite-vfs 0.2.0` `transfer.rs` — checked import resets the header to rollback
  mode, `import_db_unchecked` skips header validation; `sqlite-wasm-vfs 0.3.0`
  `sahpool.rs` — companion `-journal`/`-wal` names are tracked and a checked import
  with a companion present fails `RecoveryRequired` — crate sources, read
  2026-11-18. Verified.
- Chromium 157.0.8103.79
  `file_system_access_file_writer_impl.cc`,
  `file_system_access_safe_move_helper.cc` and `components/services/quarantine/quarantine_win.cc`
  — swap-file staging, hashing and after-write checks on close, Windows zone
  identifier — read 2026-11-18. Verified.
- VS Code for the web, `fileImportExport.ts` — folder download writes without
  awaiting; LinkUnzip `extract.js` — up to 64 pending closes — read 2026-11-18.
  Verified (code), with no throughput published by either.
