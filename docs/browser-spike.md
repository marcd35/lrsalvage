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
> this repository's own modules. Each table row carries its evidence: **measured**
> (a run on this machine), **verified** (a vendor, registry or specification
> statement, with the date it was read), or **reported** (a third party said so
> and nobody here confirmed it). A claim that is none of those is a bug.

[ADR 0012](adr/0012-two-release-lines.md) D10 makes the browser-or-desktop
question a rubric, and [app-distribution](app-distribution.md) §4 holds the
constraints it was written against. This page holds what a throwaway harness then
measured: `lrsalvage-core`'s SQLite dependency compiled to `wasm32`, this
repository's `catalog` and `previews_db` modules run against a real catalog in a
browser, payloads written into a folder a person picked, and the storage a page is
offered. The verdict is one line: **No-go**, on the reasons in §3 and §2's last
row. What §1 and §2 also record is that neither reason is a platform wall: the
write rate was measured one file at a time and the `-wal` was lost by the import
this spike chose, so both are worth one more measurement each before the decision
is beyond appeal (§6).

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
images with the sidecar and 78 without.

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

**The `-wal` row was our import, not the platform.** The import used,
`import_db`, has a *checked* mode that "resets the header's read/write versions to
rollback mode", and it carried the main file only. Measured on the sidecar variant:
**78 images read, 128 present**, no error and no hint. Two facts say that is a
choice rather than a wall:

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

So the untested prototype is short: import the main file *unchecked*, then the
`-wal` as its sibling, then let the first statement be
`PRAGMA locking_mode=EXCLUSIVE`. A second-opinion run measured the same idea one
layer down — a synthetic pair copied into ordinary OPFS and opened with the
official `@sqlite.org/sqlite-wasm` non-pooled OPFS VFS under exclusive locking,
with no supplied `-shm` — and read **128 of 128 rows**, `integrity_check` passing,
with the main-only control at 78 (**reported**; its method, the WAL's 4 152 bytes
and the post-reopen SHA-256 are in the review). Our own Rust path over that pair is
still unmeasured, and §6 puts it second.

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

**Sequentially, this reads as a hang.** Every run above awaited each write and its
close before starting the next. Two thousand files is 10–18 minutes by that
arithmetic and five thousand is 24–46 (**projection**, not a run); the worst single
file took 26 seconds. The same page writing the same bytes into OPFS is 8× faster
and a native writer on the same disk is 9–60× faster, so the cost sits in the
browser's external-file path rather than in the disk. Two caveats on that
comparison: the native control is a bare write without `extract`'s destination
checks, resume verification or rename, so it is a floor for the desktop rather than
the shipped writer; and the 26-second outlier is one sample on one machine with an
anti-malware product running, which is a correlation and not an attribution.

**Chromium's write path explains the cost, and names the lever left untested.** In
the tested build the writer stages a swap file and, on close, takes a *safe move*
path that hashes the content and requests after-write checks; for external targets
Windows quarantine can then attach a zone identifier (`154` **verified** from the
157.0.8103.79 sources). That work is per file and per close, which is consistent
with a cost that tracks file count rather than bytes. Nothing in the standard
exposes a commit-many-files call. What it does allow is **many files in flight at
once**: a close need not be awaited before the next stream starts. VS Code for the
web's folder download writes each file "without awaiting the promises" and lets
traversal continue; LinkUnzip's extraction sink permits up to 64 pending closes.
The spike never ran that experiment — every number above is strictly sequential —
so the parallel curve is **unknown**, and §6 puts it first.

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

## 6. What was not measured, and in what order it would be

The No-go rests on a sequential write rate and on an import that dropped a sidecar.
Both are one measured configuration, not a proof. Three experiments would settle
them, in this order:

1. **Bounded-concurrency write into a real picked folder.** The existing page, the
   two payload sizes and a nested mixed-size tree, with 1, 2, 4, 8, 16, 32 and 64
   streams in flight on distinct files and a byte cap; then the promising settings
   three times over fresh 200-, 1 000- and 5 000-file trees in Chrome and Edge,
   charging every write and close to wall time and recording the longest interval
   with no completed file. This is the only lever the spike left untested that
   could plausibly move the number by an order of magnitude. A product gate of
   *5 000 committed files in 120 s, no gap over 1 s, zero wrong or missing outputs,
   safe cancel and resume* would reopen the decision.
2. **The `-wal` pair through the Rust path.** Unchecked main, then the `-wal` as a
   sibling, exclusive locking first; if the pool refuses, recover in a private
   directory with the official OPFS worker, switch to `journal_mode=DELETE`, close,
   and import the clean main. Gate: 128/128 rows and native fingerprints after a
   reopen, 78 in the main-only control, source hashes unchanged, and a truncated
   or stale tail reported rather than swallowed.
3. **Direct paging, only if the copy or the quota still matters.** A custom
   read-only VFS in Rust over `FileReaderSync`, timed on a 1 GiB catalog against
   copy-plus-query. It cannot change §3: reads are not the problem.

One machine, one disk, one anti-malware configuration, one browser profile. The
picked-folder rate is a property of that combination; the OPFS and native controls
are reported beside it precisely so the comparison means something and the single
number does not.

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
