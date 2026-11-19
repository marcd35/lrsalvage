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

It is **not** part of the supported seam: no front-end calls it. A second
front-end drives `plan`, `extract` and `report`, and this module is how those
reach a disk.

## Operations

The list is what the call sites perform, in the order they ask for it. Each has
one signature and two implementations — `native` for a target with an operating
system, `wasm` for `wasm32-unknown-unknown` — chosen by `cfg` at compile time.

| Operation | What it answers | Reached from |
|---|---|---|
| `read` | a whole file's bytes | `plan`, `extract`, `report` |
| `read_prefix` | the first `n` bytes, or all of a shorter file | `jpeg` |
| `exists` | is anything at this path; `Err` when the target cannot look | `previews_db`, `extract` |
| `is_file` | is a regular file at this path | `snapshot`, `plan` |
| `list_files` | the regular files in one directory | `previews_db` |
| `walk_files` | every regular file under a root | `previews_db` |
| `create_dir_all` | a directory and its parents | `extract` |
| `write` | a whole file, replacing what was there | `report`, `extract` |
| `create_new` | an empty file, refusing a path that is taken | `extract`'s write probe |
| `write_atomic` | stage, fsync, rename: all of the new bytes or none | `extract` |
| `remove_file` | remove one file | `snapshot`, `extract` |
| `copy` | copy a file's bytes and permissions | `snapshot` |
| `scratch_dir` | a private directory that removes itself on drop | `snapshot` |
| `sha256_file` | SHA-256 of a file's stream, lowercase hex | `snapshot` |
| `canonicalize` | the absolute, symlink-free spelling of a path | `extract` |
| `current_dir` | the working directory | `extract` |
| `set_mtime` | a file's modification time, in Unix seconds | `extract` |
| `available_space` | how many bytes a filesystem can still take, for a non-privileged writer | `extract` |

## Decisions

- **A `cfg`-selected concrete module, not a trait object and not a generic
  parameter.** Both alternatives keep one signature, and both cost something this
  does not: a trait object puts a vtable behind the payload read at the centre of
  every run, and a generic parameter would put a type parameter on every module
  that asks for an operation, to express a choice made once, at compile time, by
  the target. The cost of
  `cfg` is that the two implementations are kept in step by the compiler instead
  of by a test: a function present in one and missing in the other does not
  build, so the wasm gate is what holds that.
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
- **The platform answer is `fs4`, target-gated in the manifest.** `available_space`
  asks `statvfs` on unix (`f_bavail * f_frsize`: the bytes a non-privileged
  writer may use, not the blocks a superuser could) and `GetDiskFreeSpaceExW` on
  Windows, through `fs4` 1.1.0 under
  `[target.'cfg(any(unix, windows))'.dependencies]`. The gate is not tidiness:
  `fs4`'s `available_space` is `cfg(any(unix, windows))` and its `sys` module
  exists only under those two, so the crate does not build for
  `wasm32-unknown-unknown` at all, and `scripts/check.sh wasm` would say so. A
  hand-written shim over `libc`/`windows-sys` would have added no crate — both
  are already in the lock — and was rejected for putting the tree's first
  `unsafe` in the seam; `fs2` was rejected as unmaintained since 2018. The
  licence, the MSRV and the two rejected alternatives are recorded on #64.
- **The answer belongs to the filesystem, not the directory, and the targets
  differ on a path that is not there.** Windows answers for the volume the path
  would be on; unix refuses one, because `statvfs` needs a path that exists. So a
  caller that must have an answer on every target asks about an existing
  ancestor, and `extract` does exactly that: a dry run asks about the ancestor of
  the directory it does not create, a real run about the directory its own
  destination check has already created, and no caller creates one in order to
  ask.
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
  **present and readable** `previews.db` as `Missing`, because no filesystem could
  answer the question and a `bool` has no third value (`docs/browser-spike.md`
  §1). Natively the answer is unchanged — `false` where a path cannot be
  inspected, which is what `Path::exists` always meant — but the type now lets a
  target that cannot look say so instead of saying "nothing there".
- **The digest is the seam's.** `--verify-source` publishes `source_sha256` and
  `snapshot_sha256`, and `crates/lrsalvage-cli`'s matrix test checks the reported
  hex against a hash of the file's own bytes. A browser has a stream and not a
  file, and the digest of that stream is the same hex, so the one definition of
  the number lives with the operation that streams it; `snapshot::sha256_file`
  stays as the public name that hands it back with `error::Error` context.
- **`walk_files` errs on the root and skips inside it.** `previews_db::sweep` has
  always tolerated an unreadable folder inside a cache tree — one permission
  problem must not hide the payloads beside it — while a root it cannot walk at
  all yields an empty sweep. Both are preserved exactly, and they are why this is
  not the operation `masks` wants: `masks::list` reports a walk error instead of
  skipping, so it keeps its own walk.
- **`masks` is outside the seam.** #117 puts mask export out of scope for the
  browser app, so the module is compiled only for
  `#[cfg(not(target_arch = "wasm32"))]`, keeps its `walkdir` call for
  `.lrcat-data`, and is the one documented exception to "nothing else reaches a
  filesystem". Its cargo feature is unchanged: the target gate is a second
  condition on the same module, not a replacement for the feature.
- **`wasm32` is honest, not silent.** Every operation on that target returns
  `io::ErrorKind::Unsupported` with one message until the browser slice
  implements them. That is a compile-only guarantee plus a refusal that a caller
  can report; it is not an implementation, and this module's own tests are the
  specification the browser backend has to satisfy.
- **Native first, and unchanged.** The native backend is the code those modules
  already had, moved: the same `std::fs` calls, the same `tempfile` builder and
  prefix, the same 64 KiB hashing buffer, the same `.stem.pid-n.partial` staging
  name, the same `fs::copy` semantics for the catalog trio. One user-visible
  string moves: a write that fails now produces one note, `cannot write
  <destination>`, where there used to be two — `cannot write <staging sibling>`
  when the staging file could not be written and `cannot finalise <destination>`
  when the rename did not land. The note is a field of the per-file CSV, so this
  is the one byte a *failed* run can differ by; a successful run's bytes, the
  flags, the fields and the exit codes are identical.
