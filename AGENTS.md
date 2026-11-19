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
- `report` — terminal summary, `report.csv`, `manifest.json`, contact sheet, the
  path-free verification report.
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

- **`sh scripts/check.sh` is the one gate list.** Run it before a hand-off, or
  name one gate (`sh scripts/check.sh clippy`); the file itself names the gates,
  so its header is the list and this paragraph does not repeat it. CI executes
  the Rust gates through the same file, so a gate cannot be documented weaker
  here than it is enforced. A missing tool fails rather than skipping: green must
  mean CI's green.
- **`scripts/probe-cache.sh <cache.lrdata>` measures a cache, and is not a gate.**
  It walks every level file by its declared segment lengths and lines the walk up
  against `previews.db` — the series, the segments each file carries, and where a
  row and a file disagree. `format-notes.md`'s per-level numbers come from it and
  from the shipped build's `scan`, `--report` and `extract`; a claim about
  "every file" means it was run over the whole tree, not a sample. It measures
  what the reader also measures, so `lrsalvage-cli`'s `tests/probe.rs` holds it
  to `scan --json` on a vault with a portrait level.
- **`typos` and `cargo deny` are the two gates with their own policy files.**
  Add domain words to `typos.toml`, never blanket-exclude prose. `deny.toml` is
  the licence and advisory policy: a dependency that trips its allow-list is a
  decision, not a config edit.
- The toolchain is pinned in `rust-toolchain.toml`; bump it deliberately and let
  CI prove the bump. That pin is the *build* toolchain, not the MSRV — the floor
  a user needs is `rust-version` in `Cargo.toml`, and `scripts/check.sh msrv`
  proves it, so the published number is a value a run passed rather than a table
  someone typed. Raising it also raises the syntax clippy lints against, so
  newly expected lints land in the same commit. A dependency that raises the
  floor fails that gate; raise `rust-version` deliberately with the reason in the
  bump, and never pin the dependency back to keep a lower number.
- **`.githooks/pre-commit` runs the cheap gates on every commit** — `fmt`,
  `shell` and `typos` through `scripts/check.sh`, which is the only place they
  are written. A fresh clone installs it with
  `git config core.hooksPath .githooks`; skip it deliberately with
  `git commit --no-verify`.
- **`spec_validate` is the gate no script can run.** It reports dangling links,
  duplicate ids and parent cycles across `specs/`, and it is an in-app tool, so
  neither `scripts/check.sh` nor CI can call it. A slice that adds, moves or
  re-links a spec ends by asking it for a clean graph — the link resolving and
  the design agreeing are two different questions, and `tests/spec_graph.rs`
  only answers the second.
- **The tests assert more than behaviour.** `tests/spec_graph.rs` checks each
  module's `depends-on` against the modules it actually reaches,
  `lrsalvage-cli`'s `tests/readme.rs` checks the README's flag and `--json`
  tables against the binary, and `lrsalvage-cli`'s `tests/links.rs` holds every
  relative link and heading anchor in this repository's prose to a file and a
  heading that exist. A new dependency or a new field lands with its spec and
  its table; a renamed heading lands with its links.
- **The real-vault test asserts the numbers `format-notes.md` publishes**, and it
  is the only thing that can: §5.4 carried a false claim for months because
  nothing checked it. Editing a byte-level claim means running the gate that
  measured it, not re-reading the claim: `sh scripts/check.sh real` for the
  converted cache, `real-render` or `real-1to1` for a §5.10 one, `real-discard`
  for §5.11's.
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
`#[ignore]`d tests in four evidence families, each with its own pair of names —
`LRSALVAGE_TEST_*` (a converted cache), `LRSALVAGE_RENDER_*` (a cache Lightroom
rendered from scratch), `LRSALVAGE_ONE_TO_ONE_*` (a cache it built 1:1 previews
for) and `LRSALVAGE_DISCARD_*` (a cache it discarded the standard and 1:1
previews of) — and real catalog bytes are never committed to the repo. To run
them where such catalogs exist, copy `.env.example` to a gitignored `.env` — or,
so that every worktree on the machine can reach the gates that verify a
byte-level claim, put those names in `~/.config/lrsalvage/real.env` (or the file
named by `git config lrsalvage.realEnvFile`) — and run `sh scripts/check.sh real`,
`real-render`, `real-1to1` or `real-discard`. Each family fails rather than
skipping when its own pair is unset; `real` is the route to the evidence
`README.md`'s compatibility table calls verified.

## Docs

**A measurement is restated in more than one file.** The notes measure it;
`README.md`'s status and compatibility rows, `CONTEXT.md`, the fixture labels and
a module spec may each restate it. Grep the fact before changing it — a
correction that stops at the notes leaves a copy a reader will trust, which is
exactly how §6.4 kept a false ICC claim after the render disproved it.

- `CODING_STANDARDS.md` — the judgement calls a check cannot hold. Read it before
  reviewing a diff, and before writing a claim a stranger will act on.
- `docs/agents/issue-tracker.md` — read the ticket with this **before the first
  design round**, not after the code disagrees with it.
- `docs/agents/rendering-a-test-cache.md` — how to make a cache Lightroom
  *rendered* rather than converted, what a new catalog decides on its own, and the
  five facts that make driving its GUI reliable rather than a lottery.
- `docs/format-notes.md` — byte layouts, evidence labels, the `.lrcat-data` trap.
- `docs/versions.md` — what a version number promises on each of the two release
  lines, and which capabilities each artifact currently has. It is the only place
  the two-line structure is explained, so a change to it is a change to a promise.
- `docs/app-distribution.md` — the non-Lightroom facts the app's distribution
  rests on: signing costs, SmartScreen, browser API support, toolkit accessibility
  and how peers ship a CLI beside a GUI. Every row carries a source and a read
  date, so re-checking one is a lookup rather than a research project.
- `docs/adr/` — decisions and their consequences; read before changing a seam.
- `specs/` — the spec graph: the product goal, the architecture and its
  invariants, and one spec per module. It is the ground truth for what a module
  owns and where its boundary runs.
- `CONTEXT.md` — glossary. Keep it free of implementation detail.
- `README.md` — the user-facing truth about what a preview is worth.
