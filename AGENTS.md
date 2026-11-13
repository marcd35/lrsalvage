# Working in lrsalvage

A Rust workspace that recovers JPEG previews from Lightroom Classic catalogs.
`crates/lrsalvage-core` is the library, `crates/lrsalvage-cli` the binary.

## Two rules that are load-bearing

**Parse, don't carve.** Walk the `AgHg` chunks by their declared lengths and
confirm each payload with its SOF dimensions. Marker scanning returns the EXIF
thumbnail inside a level and calls it a preview. See
[docs/adr/0001-parse-dont-carve.md](docs/adr/0001-parse-dont-carve.md) and
[docs/format-notes.md](docs/format-notes.md) §1.

**Map via `previews.db`.** The `(uuid, digest)` pair Adobe wrote is the only
authority for image → payload. Deriving keys by hashing names or paths is
unsupported and unverifiable. See
[docs/adr/0002-map-via-previews-db.md](docs/adr/0002-map-via-previews-db.md).

When touching output bytes, [ADR 0003](docs/adr/0003-preserve-preview-bytes.md)
applies: copy the payload verbatim and add metadata around it.

## Module ownership

- `snapshot` — the read-only temp copy of the catalog, WAL included. The only
  module that opens a catalog file.
- `catalog` — image rows, original paths, XMP metadata.
- `previews_db` — `previews.db` reads and the path rule for payloads.
- `lrprev`, `header`, `jpeg` — container chunks, the Lua-like header text, SOF
  parsing.
- `model` — plain data, no I/O.
- `plan` — decisions only; it never writes.
- `cli` — arguments and presentation; no format knowledge.

Format knowledge stays in the module named after the format. Nothing else parses
bytes, and `plan` never learns a byte offset.

## Verification

- `cargo test` must pass.
- `cargo clippy -- -D warnings` must be clean.
- A new byte-level fact goes into `docs/format-notes.md` with a
  **verified** / **reported** / **unverified** label. Unlabelled claims are bugs.

## User data

The catalog and cache are read-only evidence: open them read-only, write only
under the requested destination, and keep the tool's own scratch work in a temp
directory. The output root is rejected if it lands inside either tree.

Tests run against synthetic fixtures. Real catalogs are reached only from
`#[ignore]`d tests via `LRSALVAGE_TEST_CATALOG` and `LRSALVAGE_TEST_PREVIEWS`, and
real catalog bytes are never committed to the repo.

## Docs

- `docs/format-notes.md` — byte layouts, evidence labels, the `.lrcat-data` trap.
- `docs/adr/` — decisions and their consequences; read before changing a seam.
- `CONTEXT.md` — glossary. Keep it free of implementation detail.
- `README.md` — the user-facing truth about what a preview is worth.
