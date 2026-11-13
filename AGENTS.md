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
- `previews_db` — `previews.db` reads and the path rule for payloads.
- `lrprev` — the `AgHg` container: chunks, declared lengths, final-padding
  tolerance.
- `header` — parses the untrusted Lua-like pyramid header text (dimensions, profile
  name). No production path calls it: `previews.db` is the authority for sizes,
  the profile name and the mapping, so its table is documented but unused.
- `jpeg` — SOF dimensions and SOI/EOI completeness. No pixels are decoded.
- `model` — plain data, plus the sanitising of catalog text into output path
  components. No I/O.
- `plan` — decisions only: statuses, tiers, selection, stats. It never writes.
- `extract` — the per-image writer: destination checks, atomic temp-and-rename,
  resume verification, `--force` policy.
- `exif` — inserts EXIF, XMP and ICC segments around the untouched payload. ICC
  profiles are reconstructed from the catalog's profile name, never copied from a
  source file; an unknown name gets a note, not a guess.
- `report` — terminal summary, `report.csv`, `manifest.json`, contact sheet.
- `masks` — decodes `.lrcat-data` AI mask layers to PNG.
- `error` — the single error type and its I/O path context.
- `cli` — arguments and presentation; no format knowledge.

Format knowledge stays in the module named after the format. Nothing else parses
bytes, and `plan` never learns a byte offset.

## Verification

- `cargo test` must pass.
- `cargo clippy -- -D warnings` must be clean.
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
real catalog bytes are never committed to the repo.

## Docs

- `docs/format-notes.md` — byte layouts, evidence labels, the `.lrcat-data` trap.
- `docs/adr/` — decisions and their consequences; read before changing a seam.
- `CONTEXT.md` — glossary. Keep it free of implementation detail.
- `README.md` — the user-facing truth about what a preview is worth.
