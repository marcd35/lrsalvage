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
offered. The verdict is one line: **No-go**, on two named mechanisms, and the
reasons are §3 and §2's last row.

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

**`rusqlite` is not a dead end. The demand-paged VFS over a picked file is.**

`rusqlite 0.42` — the workspace's pin — builds for `wasm32-unknown-unknown` with
no change to this repository's code: on that target its **default**
`ffi-sqlite-wasm-rs` feature links `sqlite-wasm-rs 0.6.3`, which compiles the
amalgamation with clang. The VFS is `sqlite-wasm-vfs 0.3`'s `sahpool`: an
origin-private-filesystem store, in a dedicated worker, and the only persistent
VFS that crate ships.

A *synchronous* VFS over a file the user picked cannot exist.
`FileSystemFileHandle.createSyncAccessHandle()` is specified as usable "only …
for files within the origin private file system" (MDN, and the same sentence in
Chrome's own guide; verified 2026-11-18), and `sqlite-wasm-rs` ships no async VFS.
Paging a picked file directly therefore means `wa-sqlite`'s async build and a
JavaScript re-implementation of the data-access layer. That port was not attempted;
§6 says why it is the only route left.

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
that the OPFS route forces is seconds, not minutes.

**The `-wal` is the row that changes the verdict.** The pool's import copies the
main file only and resets the SQLite header's read/write versions to rollback mode
— a deliberate design of the crate, documented in `DbImport`. Measured on the
sidecar variant: **78 images read, 128 present**, no error and no hint. On the
desktop `snapshot` copies `-wal` and `-shm` and checkpoints, which is why the same
file reads 128 there.

## 3. Writing payloads

Two payload sizes, `createWritable()` + `close()` per file, times per file:
417 019 bytes (a large real level file) and 28 059 bytes (the median real level
file).

| Target | Payload | Total | p50 | p95 | Max | Rate | Evidence |
|---|---|---|---|---|---|---|---|
| Picked folder | 417 019 B | 286.0 s | 148.6 ms | 842.5 ms | 3 037 ms | 3.5/s | measured |
| Picked folder | 28 059 B | 545.6 s | 286.7 ms | 1 366.4 ms | **26 269 ms** | 1.8/s | measured |
| Picked folder, fresh folder re-check | 28 059 B | 61.1 s | 265.6 ms | 586.5 ms | 901 ms | 3.3/s | measured |
| OPFS, same call pattern | 417 019 B | 34.2 s | 27.4 ms | 71.1 ms | 565 ms | 29.3/s | measured |
| Native, plain write | 417 019 B | 20.3 s | 9.1 ms | 61.0 ms | 1 651 ms | 49/s | measured |
| Native, write + `fsync` | 417 019 B | 44.2 s | 36.2 ms | 101.9 ms | 278 ms | 23/s | measured |
| Native, plain write | 28 059 B | 9.3 s | 4.3 ms | 29.8 ms | — | 108/s | measured |

**This reads as a hang.** Two thousand files is 10–18 minutes; five thousand is
24–46; the worst single file took 26 seconds. The same page writing the same bytes
into OPFS is 8× faster and a native writer on the same disk is 9–60× faster, so the
cost sits in the browser's user-visible write path rather than in the disk, and
the recovery output is exactly the output that has to land in a folder the user
picked.

One write finding helps: **an aborted stream leaves the file untouched.** Writing
512 KB over an existing 1 048 576-byte file and aborting left exactly
1 048 576 bytes and no leftover directory entry, matching the File System
standard's "the file will either contain its old contents or the new contents".
`extract`'s temp-and-rename is already provided by the platform there.

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

## 6. What was not measured

The `wa-sqlite` async port — a custom asynchronous VFS over a picked file, with the
two queries rewritten in JavaScript — was not built. Its cost is the whole
data-access layer, and the platform facts in §1 are the reason it is the only
candidate left.

One machine, one disk, one anti-malware configuration. The picked-folder rate is a
property of that combination; the OPFS and native controls are reported beside it
precisely so the comparison means something and the single number does not.

Nothing here says a native shell meets the same wall. It writes through the
operating system, like the native control does, and it can replay a `-wal` because
`snapshot` already knows how.

## Sources

- `FileSystemFileHandle.createSyncAccessHandle()` — "only usable inside dedicated
  Web Workers for files within the origin private file system" — MDN, read
  2026-11-18. Verified.
- File System standard, `createWritable()` — "user agents try to ensure that no
  partial writes happen" — read 2026-11-18. Verified.
- `rusqlite 0.42.0` manifest — `sqlite-wasm-rs >=0.6.3,<0.7` for
  `wasm32-unknown-unknown`, with `ffi-sqlite-wasm-rs` in the default features —
  crates.io, read 2026-11-18. Verified in the published tarball.
- `sqlite-wasm-rs 0.6.3` — compiles SQLite through `cc`, ships `memory` and (with
  `sqlite-wasm-vfs 0.3`) `sahpool` VFSes; no async VFS — crate sources, read
  2026-11-18. Verified.
