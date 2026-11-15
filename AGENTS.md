# Working in lrsalvage

A Rust workspace that recovers JPEG previews from Lightroom Classic catalogs.
`crates/lrsalvage-core` is the library, `crates/lrsalvage-cli` the binary.

## Two rules that are load-bearing

**Parse, don't carve.** Walk the `AgHg` chunks by their declared lengths and
confirm each payload with its SOF dimensions. Marker scanning can return an
embedded EXIF thumbnail and call it a preview. See
[docs/adr/0001-parse-dont-carve.md](docs/adr/0001-parse-dont-carve.md) and
[docs/format-notes.md](docs/format-notes.md) §1.

**Map via `previews.db`.** The `(uuid, digest)` pair Adobe wrote is the only
authority for image → payload. Deriving keys by hashing names or paths is
unsupported and unverifiable. See
[docs/adr/0002-map-via-previews-db.md](docs/adr/0002-map-via-previews-db.md).

When touching output bytes, [ADR 0003](docs/adr/0003-preserve-preview-bytes.md)
applies: copy the compressed image data verbatim and add or replace only metadata
segments.

## Module ownership

- `snapshot` — the read-only temp copy of the catalog, `-wal`/`-shm` included and
  checkpointed. The only module that copies a catalog file.
- `catalog` — read-only catalog queries: image rows, original paths, and the XMP
  metadata blob.
- `previews_db` — `previews.db` reads, the path rule for payloads, the five-word
  `Layout` and the one cache-tree walk.
- `lrprev` — the `AgHg` container: chunks, declared lengths, final-padding
  tolerance.
- `header` — parses the untrusted Lua-like pyramid header text (dimensions, profile
  name). No production path calls it: `previews.db` is the authority for sizes,
  the profile name and the mapping, so its table is documented but unused.
- `jpeg` — SOF dimensions, SOI/EOI completeness, and the two-byte payload-like
  sniff. No pixels are decoded.
- `model` — plain data, plus the sanitising of catalog text into output path
  components. No I/O.
- `plan` — decisions only: statuses, tiers, selection, stats. It never writes.
- `extract` — the per-image writer: destination checks, atomic temp-and-rename,
  resume verification, `--force` policy.
- `exif` — inserts EXIF, XMP and ICC segments around the untouched payload. ICC
  profiles are reconstructed from the catalog's profile name, never copied from a
  source file; a payload that already carries one keeps it, and an unknown name
  gets a note, not a guess.
- `report` — terminal summary, `report.csv`, `manifest.json`, contact sheet.
- `masks` — decodes `.lrcat-data` AI mask layers to PNG.
- `error` — the single error type and its I/O path context.
- `cli` — arguments and presentation; no format knowledge.

Format knowledge stays in the module named after the format. Nothing else parses
bytes, and `plan` never learns a byte offset.

`crates/lrsalvage-fixtures` is test-only and is not a module: it builds the
synthetic vaults `specs/fixtures.md` requires — the byte shapes, the catalog and
`previews.db` — and is a dev-dependency of both crates. A test composes a
`Vault`; it does not write a fixture of its own, and a fourth ad-hoc writer is
not allowed.

## Verification

- **`sh scripts/check.sh` is the one gate list.** It runs `fmt`, `clippy`
  (`--all-targets`), the workspace tests, `cargo doc`, the release build, `typos`
  and `cargo deny`. Run it before a hand-off, or name one gate
  (`sh scripts/check.sh clippy`). CI executes the Rust gates through the same
  file, so a gate cannot be documented weaker here than it is enforced. A
  missing tool fails rather than skipping: green must mean CI's green.
- **`typos` and `cargo deny` are the two gates with their own policy files.**
  Add domain words to `typos.toml`, never blanket-exclude prose. `deny.toml` is
  the licence and advisory policy: a dependency that trips its allow-list is a
  decision, not a config edit.
- The toolchain is pinned in `rust-toolchain.toml`; bump it deliberately and let
  CI prove the bump.
- **`.githooks/pre-commit` runs the cheap gates on every commit** — `fmt` and
  `typos` through `scripts/check.sh`, which is the only place they are written.
  A fresh clone installs it with `git config core.hooksPath .githooks`; skip it
  deliberately with `git commit --no-verify`.
- **The tests assert more than behaviour.** `tests/spec_graph.rs` checks each
  module's `depends-on` against the modules it actually reaches, and
  `lrsalvage-cli`'s `tests/readme.rs` checks the README's flag and `--json`
  tables against the binary. A new dependency or a new field lands with its spec
  and its table.
- Byte-level facts learned from real files must be recorded in
  `docs/format-notes.md` with an evidence label (**verified**, **reported**,
  **unverified**, or **TODO**) before they are built on. Unlabelled claims are
  bugs.

## User data

The catalog and cache are read-only evidence: open them read-only, write only
under the requested destination, and keep the tool's own scratch work in a temp
directory. The output root — and a contact sheet's parent directory — is rejected
if it lands inside either tree.

Tests run against synthetic fixtures. Real catalogs are reached only from
`#[ignore]`d tests via `LRSALVAGE_TEST_CATALOG` and `LRSALVAGE_TEST_PREVIEWS`, and
real catalog bytes are never committed to the repo. To run them where such
catalogs exist, copy `.env.example` to a gitignored `.env` and run
`sh scripts/check.sh real`; that is the only route to the evidence `README.md`'s
compatibility table calls verified.

## Docs

- `docs/format-notes.md` — byte layouts, evidence labels, the `.lrcat-data` trap.
- `docs/adr/` — decisions and their consequences; read before changing a seam.
- `specs/` — the spec graph: the product goal, the architecture and its
  invariants, and one spec per module. It is the ground truth for what a module
  owns and where its boundary runs.
- `CONTEXT.md` — glossary. Keep it free of implementation detail.
- `README.md` — the user-facing truth about what a preview is worth.
