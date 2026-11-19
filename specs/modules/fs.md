---
id: fs
type: module-design
title: "fs: the one seam onto a filesystem"
status: draft
parent: architecture
---

# fs

## Responsibility

Be the crate's only answer to a filesystem question: read a file, read its first
bytes, ask whether a path exists or is a file, list a directory, walk a tree,
create a directory, write a file, write one atomically, remove one, copy one,
make a scratch directory, hash a file's stream, resolve a path, name the working
directory, stamp a modification time, and ask how much room a filesystem has.

On `wasm32-unknown-unknown` it is also the crate's only answer to **where a path
resolves**. That target has no filesystem, so the module holds a table of mounted
roots and routes an operation to one of three kinds of storage: an
origin-private pool, a picked directory handle, or a picked read-only file.

It owns no format and no decision. `snapshot` decides what a catalog copy is,
`previews_db` decides what a payload path looks like, `extract` decides whether a
destination may be written — this module only does the I/O those decisions need,
in the one place where the answer can differ per target.

## Boundary

Six modules ask this module — `snapshot`, `previews_db`, `jpeg`, `plan`,
`extract` and `report`, and the operation table below records which of them asks
for what. No other module's shipped code in `lrsalvage-core` names `std::fs`,
`tempfile` or `filetime`, and `tests/fs_seam.rs` fails when that stops being
true; its own `#[cfg(test)]` modules do, and the test knows the difference. The
one recorded exception is `masks`, which is native-only and outside this seam
(see below).

No format knowledge, no SQL, no path convention beyond what an operation needs:
this module does not know that `-wal` is a sidecar or that `.partial` is
temporary. Errors are `std::io::Error` and nothing else, so a caller keeps
attaching its own path context (`error::IoContext`) and no error type grows a
filesystem-shaped variant.

It is **not** part of the supported seam, with one recorded exception: on
`wasm32` `crate::fs::wasm` is a public module, because a page has to *install* the
backend before a run. `install_bridge`, `install_store` (and its
`start_install`/`poll_install` pair for a host with no async runtime), `mount`,
`seal`, `reset` and `store_installed` are the crate's browser-facing surface and
the only `fs` API a front end calls. A front-end that never installs one gets
`Unsupported` from every bridged operation, which is the honest answer for a page
that wired nothing.

Three files make up the browser half. `src/fs/wasm.rs` is the routing and the
store; `src/fs/bridge.rs` is the protocol a page answers; `src/fs/mounts.rs` is
the namespace arithmetic. The last two are compiled for the host as well, because
they are arithmetic rather than browser, and their unit tests are the ones that
can run without a page.

The page-side host lives with the harness, not in this crate:
`crates/lrsalvage-web/web/` holds the worker's bridge, the page's host and the
parity page, and `scripts/browser-parity.sh` drives them.

## Operations

The list is what the call sites perform, in the order they ask for it. Each has
one signature and two implementations — `native` for a target with an operating
system, `wasm` for `wasm32-unknown-unknown` — chosen by `cfg` at compile time. The
last column is what the browser backend answers with, and it is the contract the
next slice reads.

| Operation | What it answers | Reached from | On `wasm32-unknown-unknown` |
|---|---|---|---|
| `read` | a whole file's bytes | `plan`, `extract`, `report` | answered: the store's export, or the host's chunked read |
| `read_prefix` | the first `n` bytes, or all of a shorter file | `jpeg` | answered |
| `exists` | is anything at this path; `Err` when the target cannot look | `previews_db`, `extract` | answered; `Err` only when nothing is mounted or no bridge is installed |
| `is_file` | is a regular file at this path | `snapshot`, `plan` | answered |
| `list_files` | the regular files in one directory | `previews_db` | answered: a prefix scan of the store, or a paged directory listing |
| `walk_files` | every regular file under a root | `previews_db` | answered |
| `create_dir_all` | a directory and its parents | `extract` | answered; a no-op in the store, where a directory is a prefix |
| `write` | a whole file, replacing what was there | `report`, `extract` | answered |
| `create_new` | an empty file, refusing a path that is taken | `extract`'s write probe | answered |
| `write_atomic` | stage, flush, rename: all of the new bytes or none | `extract` | answered: one `createWritable` … `close`, which is the platform's own atomic commit |
| `remove_file` | remove one file | `snapshot`, `extract` | answered |
| `copy` | copy a file's bytes and permissions | `snapshot` | answered, across mounts; permissions are not a thing a page has |
| `scratch_dir` | a private directory that removes itself on drop | `snapshot` | answered: a prefix in the store, removed with its handle |
| `sha256_file` | SHA-256 of a file's stream, lowercase hex | `snapshot` | answered; the same hex as the native run's for the same bytes |
| `canonicalize` | the absolute, symlink-free spelling of a path | `extract` | answered by normalising the components: this namespace has no symlinks |
| `current_dir` | the working directory | `extract` | `/` — a page has no working directory |
| `set_mtime` | a file's modification time, in Unix seconds | `extract` | **refused**, `Unsupported`: the File System API cannot set one |
| `available_space` | how many bytes a filesystem can still take, for a non-privileged writer | `extract` | **refused**, `Unsupported`: a page cannot know the destination's free space (`#117`) |

Two refusals are the design rather than a gap: a modification time, and the
destination's free space. `#117` records the second as a page's limit, and
`extract` already treats an unmeasured destination as one it proceeds with
(`extract::ensure_room`'s `Preflight::Unmeasured`), so a browser run degrades the
way a desktop run on an unmeasurable filesystem does.

## The browser half

### The namespace

A mount is a path prefix and a kind, and the longest matching root answers:

| Kind | What it is | What answers it |
|---|---|---|
| store | the origin-private pool SQLite opens through | this process, synchronously |
| directory | a `FileSystemDirectoryHandle`, from `showDirectoryPicker` or from the origin-private file system | the page, over the bridge |
| file | one picker's `File`, read-only | the worker, synchronously (`FileReaderSync`) |

A path under no mount is `NotFound` — not a guess and not "absent". Mounts are
registered once, and a session that forgets `seal` still behaves, because an
unmounted path was always going to be that answer.

### The store

The store is `sqlite-wasm-vfs`'s `sahpool` over the origin-private file system:
the only persistent SQLite VFS in the measured route (`docs/browser-spike.md`
§1–§2), and therefore the only place on this target where a catalog copy can be
opened at all. It is a flat namespace of names, and a virtual path *is* the name
— which is what makes the file the seam's caller built the path for the same file
SQLite opens.

Two properties follow and are load-bearing:

- **A directory is a prefix, not an entry.** `create_dir_all` makes nothing,
  `exists` answers for a directory when something is under it, and a listing is a
  prefix scan. A scratch directory is therefore not "there" until its first file
  is, which no caller observes: `snapshot` copies into it immediately.
- **Removal is synchronous.** The pool is in this process, so `ScratchDir`'s
  `Drop` removes the copy with its handle, exactly as the native guard did. That
  is the decision the ticket asked for: **no async removal is deferred anywhere**,
  because the one thing a `Drop` cannot do is await.

Capacity is a count of files the pool may hold at once, not a size: a slot grows
with what is written to it. `install_store` takes it from the caller and is
awaited once; a store that has not been installed is `Unsupported`, never an
empty namespace.

### The bridge

Every operation on a directory or file mount is a promise, and the seam is
synchronous. One import bridges them:

```text
call(op: u32, request_ptr, request_len, out_ptr, out_cap) -> i32
```

The request is UTF-8 JSON, with a body base64-encoded where there is one; the
answer is `[kind: u8][bytes …]` written into the caller's buffer, and the return
value is how many bytes of it are meaningful. A negative return is the bridge
itself failing, with a message in the same buffer. One answer may not exceed
512 KiB, and `read` is the operation that keeps it that small: the caller asks for
at most 256 KiB at an offset and loops, so a catalog of a few hundred megabytes
costs the chunk and never the file. `list_files` and `walk_files` page the same
way, and the host caches an enumeration until the next write.

The host may make that call synchronous however it likes. The measured route is
`crates/lrsalvage-web/web/lrsalvage-fs.js`: the run happens in a dedicated
worker, the worker posts the operation to the page and parks on a
`SharedArrayBuffer` (`Atomics.wait`), and the page — which owns the handles and an
event loop — awaits the operation and notifies it. Two consequences are the
page's contract, not a preference:

- **The run has to be in a dedicated worker.** The pool is specified to live in
  one, and `Atomics.wait` is not allowed on a main thread.
- **The page has to be cross-origin isolated** (`Cross-Origin-Opener-Policy:
  same-origin` and `Cross-Origin-Embedder-Policy: require-corp`), because a
  `SharedArrayBuffer` cannot be constructed otherwise. `#117`'s hosting decision
  inherits this.

The registration point, in order: `install_bridge` (the host's one call), `mount`
per root, `install_store` (awaited, or `start_install` then `poll_install` from a
macrotask for a host that cannot await), `seal`. Mounting is before the install
because a `scratch_dir` needs a store mount to allocate under, and `seal` is after
it because a run may only start once SQLite has a VFS. `reset` forgets all of it.
`crates/lrsalvage-web` wraps that for a page and is the worked example — that
order, in `crates/lrsalvage-web/src/wasm.rs`.

### What the browser does differently, and why

- **`write_atomic` on a bridged destination is the platform's atomic close, not a
  staged sibling.** "The file will either contain its old contents or the new
  contents" is what `createWritable` promises, so the promise is kept by a
  different mechanism rather than weakened (`docs/browser-spike.md` §3). A
  **store** destination is weaker: the pool publishes a whole image or none, but
  the name is removed before the new one is imported, so a failed import leaves
  neither. That is honest about a store and unreachable in a run — `extract` and
  `report` write to a picked folder, and the store's own writers are `snapshot`'s
  copies, which are `copy` and not `write_atomic`.
- **One `write_atomic` call is one stream, awaited.** The 8–16-streams-in-flight
  writer `#117` measured is that slice's work: the seam has one whole-file write
  and no batching operation, and inventing one here would be a signature the
  native side does not need.
- **`copy` onto a store name whose `-wal` companion exists is refused**, with
  `io::ErrorKind::Other` and the pool's own message, because no `ErrorKind` says
  "a pending WAL describes this image". Natively a copy would overwrite the main
  file and leave the stale sidecar behind. No caller in this tree copies onto
  such a name — `snapshot` copies *into* a fresh scratch directory — so the
  refusal is a safety property rather than a difference a run can meet, and the
  parity probe asserts it as a declared divergence
  (`store_copy_refuses_a_sidecar_name`).
- **A `copy` to a bridged destination holds the file's bytes**, because the
  seam's copy is whole-file. The one large copy in a run is the catalog, and a
  catalog copy always lands in the store, where the import streams it.
- **`Path::is_absolute` is `false` for every path on this target**, measured by
  the parity probe (`path_spelling`). `Path::join` still replaces an absolute
  base, so `extract::resolve_pending`'s `current_dir().join(path)` produces `/`
  plus the path and nothing else goes wrong; the fact is recorded here because it
  is the kind of thing a reader would otherwise assume.
- **The store's install is polled, not awaited.** It is a chain of browser
  promises — acquiring the pool's slots is one per slot — and this crate will not
  take an async runtime to await it: `deny.toml` bans one and a file reader does
  not need one. `start_install` parks the future and `poll_install` advances it
  from a macrotask, which is where a settled promise's callback runs anyway. The
  cost is a host-side loop (`web/worker.js` schedules it on a `MessageChannel`,
  because a zero timeout is clamped); the alternative was `wasm-bindgen-futures`,
  which resolves `tokio` into the lockfile on the host side of its manifest.
- **`std::process::id` panics on this target**, so `extract`'s write-probe name
  uses a `cfg`-selected tag rather than the process id. That is one of the two
  caller lines this slice had to move; neither has a signature.

### Where the harness lives

`crates/lrsalvage-web` is a `publish = false` workspace member: a `cdylib` a page
loads, one driver that compiles for both targets, and a native binary that builds
the vault and runs the driver on the host. `scripts/browser-parity.sh` builds it,
serves it, drives headless Chromium over the DevTools Protocol and compares the
two reports with `MAIN_SEPARATOR` normalised.

**It is not a gate.** `scripts/check.sh` is the gate list and a missing tool there
is a failure rather than a skip, so a gate here would make every CI runner install
a browser to prove a claim about one. It is the same shape as
`scripts/probe-cache.sh`: a real measurement, run by a person or a release check,
that names the tree it measured. What the gate list does hold is that the crate
and the seam's browser half *compile and lint* for the target — `scripts/check.sh
wasm` does both, because before this slice that code was compiled by no gate at
all.

The harness lives in the tree rather than beside it (#126's lived outside, and
`#142`'s brief was that nothing from outside goes in): a harness nobody can run
from a checkout is a claim nobody can reproduce.

What a machine needs to run it — named here because the script fails rather than
skipping, and because only one of these is installed by default: `cargo` and
`node`; the `wasm32-unknown-unknown` target; a C compiler for that target, in
`CC_wasm32_unknown_unknown`/`AR_wasm32_unknown_unknown` (the same two names
`scripts/check.sh wasm` reads, documented in `.env.example`); **`wasm-bindgen-cli`
at the version `Cargo.lock` pins** for the `wasm-bindgen` crate — the CLI and the
crate refuse each other across a version, so a stale one fails at the first build
with a schema error; and Chromium or Chrome (or `CHROME` pointing at one).

## Decisions

- **A `cfg`-selected concrete module, not a trait object and not a generic
  parameter.** Both alternatives keep one signature, and both cost something this
  does not: a trait object puts a vtable behind the payload read at the centre of
  every run, and a generic parameter would put a type parameter on every module
  that asks for an operation, to express a choice made once, at compile time, by
  the target. The cost of `cfg` is that the two implementations are kept in step
  by the compiler instead of by a test: a function present in one and missing in
  the other does not build, so the wasm gate is what holds that.
- **The seam's signatures did not change, and no caller module moved.** The
  browser backend was written against the eighteen existing signatures, which is
  the test #142's choice existed for. Two *lines* in callers moved and neither is
  a signature: `snapshot::checkpoint_copy` sets `PRAGMA locking_mode=EXCLUSIVE`
  before the checkpoint (the measured path in both targets, and the only way a
  WAL recovers without a `-shm` a browser cannot have —
  `docs/browser-spike.md` §2), and `extract` names its write probe with a
  `cfg`-selected tag instead of `std::process::id`, which panics on the target.
- **The seam is the call sites, not POSIX.** There is no
  `open`/`read`/`write`/`seek` handle, because a browser cannot offer one and a
  seam whose second implementation cannot be written is a seam with one
  implementation. `write_atomic` is the shape this buys: staging, `fsync` and
  `rename` are one call because they are one promise
  (`adr-0003-preserve-preview-bytes`), and no caller can perform half of it.
- **The free-space question arrived with its first caller (#64).** This slice's
  own ticket listed "file length" and "the free space `plan`/`extract` ask", and
  carried neither: an operation with no caller is a guess the browser
  implementation would then have to satisfy. `available_space` has a caller now —
  `extract` refuses a destination the plan would not fit — so it is here, and the
  table above gained its row in the same commit. File length still has none: the
  only `fs::metadata` calls in the tree are in tests, so it stays absent.
- **The platform answer is `fs4`, target-gated in the manifest.**
  `available_space` asks `statvfs` on unix (`f_bavail * f_frsize`: the bytes a
  non-privileged writer may use, not the blocks a superuser could) and
  `GetDiskFreeSpaceExW` on Windows, through `fs4` 1.1.0 under
  `[target.'cfg(any(unix, windows))'.dependencies]`. The gate is not tidiness:
  `fs4` does not build for `wasm32-unknown-unknown` at all, so the browser half
  answers this operation itself and refuses it.
- **The answer belongs to the filesystem, not the directory, and the targets
  differ on a path that is not there.** Windows answers for the volume the path
  would be on; unix refuses one, because `statvfs` needs a path that exists. So a
  caller that must have an answer on every target asks about an existing
  ancestor, and `extract` does exactly that: a dry run asks about the ancestor of
  the directory it does not create, a real run about the directory its own probe
  just made. On `wasm32` there is no answer at all, so the refusal is what the
  same caller reads.
- **One test aid, and only in a debug build.** No portable unprivileged machine
  can be made nearly full on demand, so the native backend answers
  `LRSALVAGE_FREE_SPACE_BYTES` when the crate is built with `debug_assertions` —
  which is what `cargo test` compiles — and never reads the name otherwise, so a
  release binary always asks the platform. That is how the real binary is held to
  the real refusal, including its exit code (`lrsalvage-cli`'s matrix row M19).
  It is the seam's only substitution, and a second one would need a reason this
  one does not have.
- **`exists` answers with a reason, `is_file` with a reason too, and a caller
  that wants a `bool` says `unwrap_or(false)`.** This is the measured point of
  the slice: on `wasm32` the `Path::exists` gate in `previews_db::open` reported a
  **present and readable** `previews.db` as `Missing` (`docs/browser-spike.md`
  §1), and that failure is now closed by a test rather than by a claim — the
  parity run reports the index for a store-mounted `previews.db` with no
  `index_problem`. Natively the answer is unchanged — `false` where a path cannot
  be inspected, which is what `Path::exists` always meant — but the type lets a
  target that cannot look say so instead of saying "nothing there".
- **The digest is the seam's.** `--verify-source` publishes `source_sha256` and
  `snapshot_sha256`, and `crates/lrsalvage-cli`'s matrix test checks the reported
  hex against a hash of the file's own bytes. A browser has a stream and not a
  file, and the digest of that stream is the same hex, so the one definition of
  the number lives with the operation that streams it; `snapshot::sha256_file`
  stays as the public name that hands it back with `error::Error` context. The
  parity harness compares both digests on both targets for the same vault, which
  is the acceptance test for that sentence.
- **`walk_files` errs on the root and skips inside it.** `previews_db::sweep` has
  always tolerated an unreadable folder inside a cache tree — one permission
  problem must not hide the payloads beside it — while a root it cannot walk at
  all yields an empty sweep. Both are preserved exactly, and they are why this is
  not the operation `masks` wants: `masks::list` reports a walk error instead of
  skipping, so it keeps its own walk. Neither browser kind has an unreadable
  subtree to skip: a directory handle either lists or throws, and the store's
  names are a `HashMap`.
- **`masks` is outside the seam.** #117 puts mask export out of scope for the
  browser app, so the module is compiled only for
  `#[cfg(not(target_arch = "wasm32"))]`, keeps its `walkdir` call for
  `.lrcat-data`, and is the one documented exception to "nothing else reaches a
  filesystem". Its cargo feature is unchanged: the target gate is a second
  condition on the same module, not a replacement for the feature.
- **The bridge is hand-written shims, not `wasm-bindgen`'s bindings.** Both are a
  target-specific dependency, and both answer the same call, so the choice is
  about what each costs. `wasm-bindgen`'s typed bindings buy ergonomics for a
  *large* API surface — DOM, fetch, console — and this seam's surface is one
  synchronous function whose body is six calls into `FileSystemDirectoryHandle`.
  `sqlite-wasm-vfs` already links `wasm-bindgen` transitively, so `deny.toml`, the
  MSRV and the package gate are unaffected either way, and so the seam's own
  crate takes **no** direct dependency for this: `lrsalvage-core`'s wasm block is
  `sqlite-wasm-rs`, `sqlite-wasm-vfs` and `rsqlite-vfs`, and nothing else. The
  crate that builds the *page* does take one — `wasm-bindgen` in
  `crates/lrsalvage-web`, because that is what turns a `cdylib` into something a
  page can call, and `wasm-bindgen-cli` is then a prerequisite of running the
  harness at all (*Where the harness lives*). What the shims buy is that a page
  needs neither of those to answer the seam: `web/protocol.js` is the JS half,
  plain modules with no build step.
  The cost is that the protocol has two implementations — Rust and JavaScript —
  kept in step by reading them, which is why the operation codes are one list in
  `src/fs/bridge.rs` and one in `web/protocol.js` (imported by **both** JS halves,
  so the page and the worker cannot disagree), why the Rust half is unit-tested
  against a fake host, and why the parity run exists at all.
- **Native first, and unchanged.** The native backend is the code those modules
  already had, moved: the same `std::fs` calls, the same `tempfile` builder and
  prefix, the same 64 KiB hashing buffer, the same `.stem.pid-n.partial` staging
  name, the same `fs::copy` semantics for the catalog trio. One user-visible
  string moved in #142: a write that fails now produces one note, `cannot write
  <destination>`. This slice adds one pragma to the checkpoint path and no other
  native behaviour; the CLI's fixture matrix, the four real-cache families and
  every byte they assert are unchanged.
