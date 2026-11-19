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
directory, stamp a modification time.

It owns no format and no decision. `snapshot` decides what a catalog copy is,
`previews_db` decides what a payload path looks like, `extract` decides whether a
destination may be written — this module only does the I/O those decisions need,
in the one place where the answer can differ per target.

## Boundary

Every other module asks this module; nothing else in `lrsalvage-core` names
`std::fs`, `tempfile` or `filetime`, and `tests/fs_seam.rs` fails when that stops
being true. The one recorded exception is `masks`, which is native-only and
outside this seam (see below).

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

## Decisions

- **A `cfg`-selected concrete module, not a trait object and not a generic
  parameter.** Both alternatives keep one signature, and both cost something this
  does not: a trait object puts a vtable behind the payload read at the centre of
  every run, and a generic parameter would put a type parameter on eight modules
  to express a choice made once, at compile time, by the target. The cost of
  `cfg` is that the two implementations are kept in step by the compiler instead
  of by a test: a function present in one and missing in the other does not
  build, so the wasm gate is what holds that.
- **The seam is the call sites, not POSIX.** There is no
  `open`/`read`/`write`/`seek` handle, because a browser cannot offer one and a
  seam whose second implementation cannot be written is a seam with one
  implementation. `write_atomic` is the shape this buys: staging, `fsync` and
  `rename` are one call because they are one promise
  (`adr-0003-preserve-preview-bytes`), and no caller can perform half of it.
- **Two questions the design expected and the code never asks.** The slice's
  ticket listed "file length" and "the free space `plan`/`extract` ask". There is
  no call site for either: `grep -rn 'available_space\|statvfs\|fs2'` over
  `crates/` finds nothing, and the only `fs::metadata` calls in the tree are in
  tests. The free-space refusal is a *planned* capability — `docs/versions.md`
  gives `0.4.0` the run-refusing check, not this tree — so it is not here. An
  operation with no caller is a guess the browser implementation would then have
  to satisfy; both land with their first caller, which is a change to this list
  and this table.
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
  name, the same `fs::copy` semantics for the catalog trio. The only user-visible
  difference a failed write produces is which path its note names — the
  destination instead of the staging sibling — because the staging name is now an
  implementation detail this module does not hand out.
