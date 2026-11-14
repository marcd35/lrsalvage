---
id: architecture
type: architecture-design
title: "lrsalvage architecture"
status: draft
parent: lrsalvage
references:
  - format-notes
  - glossary
---

# lrsalvage architecture

## Shape

Two crates. `lrsalvage-core` is the whole pipeline and the **supported seam**:
its public API is what a GUI, a script or a test drives, so moving a public item,
changing a signature or hiding a module is a decision to record here rather than
a refactor. `lrsalvage-cli` is one adapter over it — arguments, presentation,
progress, exit codes — and carries no format knowledge.

The pipeline is a straight line, and every stage is pure with respect to the one
before it:

```
snapshot ─┬─► catalog ──────┐
          └─► previews.db ──┴─► plan ─┬─► extract ─► output tree
                                      └─► report  ─► summary / CSV / JSON / sheet
```

`plan` is the hinge. It is cheap, printable, and write-free — which is why `scan`
can be a full rehearsal of `extract`, and why `extract` never has to decide
anything about an image, only write what the plan selected.

## Module dependency edges

Edges the code actually has (non-test). A module may depend only on the ones
listed for it:

| Module | Depends on |
|---|---|
| `snapshot` | `error` |
| `catalog` | `snapshot`, `model`, `error` |
| `previews_db` | `snapshot`, `model`, `error` |
| `lrprev` | `jpeg`, `error` |
| `jpeg` | `error` |
| `header` | `error` |
| `model` | — (no internal dependencies, no I/O) |
| `plan` | `catalog`, `previews_db`, `lrprev`, `jpeg`, `model`, `error` |
| `extract` | `plan`, `exif`, `jpeg`, `lrprev`, `model`, `report`, `error` |
| `exif` | `model`, `error` |
| `report` | `plan`, `lrprev`, `jpeg`, `model`, `error` |
| `masks` | `error` |
| `error` | — |
| `cli` | any of the above, as a consumer |

Two edges are easy to miss and are real: `lrprev` calls `jpeg` to measure each
declared payload, and `extract` calls `report::escape_csv` so the run CSV is
escaped by the same rules as `report.csv`. `previews_db` borrows `snapshot`'s
read-only opener for the cache's own database, which is why it appears in the
table. `masks` is gated behind the `masks` cargo feature (`png` is optional);
everything else is always compiled.

## Invariants the code enforces

- **Ownership of side effects.** `extract` is the only module that writes
  user-visible output. `report` writes only to a path its caller named, and
  `scan` produces exactly one artifact at most: an explicitly requested contact
  sheet. `snapshot` is the only module that copies a catalog file, and it copies
  into its own temporary directory.
- **Format knowledge stays in the module named after the format.** Nothing else
  parses bytes: `lrprev` owns the `AgHg` container, `previews_db` owns the index
  database, `catalog` owns the `.lrcat` schema, `jpeg` owns marker framing,
  `exif` owns metadata segments, `masks` owns `.lrcat-data`. `plan` composes
  their answers and never learns a byte offset.
- **Decide, then write.** `plan` decides status, selection, tier and output path
  and writes nothing; `extract` writes and decides nothing about which image
  deserves to be recovered. A scan and an extract for the same inputs therefore
  agree by construction.
- **One error type.** Every fallible core function returns `error::Error`, with
  the path context that produced it. Exit codes are the CLI's mapping of that
  type, never a status smuggled through the core.
- **Nothing is clobbered on faith.** An existing destination is replaced only
  when it verifies as one of our own outputs; a report file name is claimed, not
  assumed.
- **User data is opened read-only.** The `.lrcat`, the preview cache and
  `.lrcat-data` are evidence. A destination inside any of those trees is refused
  before the first write.

## Where the reasoning lives

The three load-bearing decisions are recorded as adopted ADRs and are not
restated here: `adr-0001-parse-dont-carve`, `adr-0002-map-via-previews-db`,
`adr-0003-preserve-preview-bytes`. Byte-level facts about the formats, with an
evidence label per claim, live in `format-notes`; the product vocabulary lives in
`glossary`. A module spec links to those and carries only what is specific to its
own boundary.
