# Contributing to lrsalvage

This file is a map, not a manual. Each thing below has exactly one home, and this
page points at it rather than repeating it — a second copy drifts, and this
repository has the scar tissue to prove it.

## Read these first

| File | Why |
|---|---|
| [`CODING_STANDARDS.md`](CODING_STANDARDS.md) | The judgement calls a check cannot hold: one home per measurement, what a claim is, when a boundary change edits its spec. Read it before writing anything a stranger will trust. |
| [`AGENTS.md`](AGENTS.md) | How the tree is laid out, which module owns which format, and the rules that are load-bearing. |
| [`docs/versions.md`](docs/versions.md) | Two artifacts, two version lines. If you are unsure whether your change is core work or app work, this is the page that says. |
| [`specs/`](specs/) | The spec graph: the product goal, the architecture and its invariants, and one spec per module. It is the ground truth for what a module owns and where its boundary runs. |
| [`docs/adr/`](docs/adr/) | Decisions, with what they rule out. Read before changing a seam; do not re-open a recorded decision without new evidence. |

## Before you open a pull request

**One gate list.** [`scripts/check.sh`](scripts/check.sh) is the whole of it —
formatting, clippy with `--all-targets`, the tests, the doc build, the release
build, spelling, the licence policy and the MSRV. The file's header *is* the list,
so it is not repeated here. CI runs the same file, so a green run locally means a
green run there; a missing tool fails rather than skipping.

Two gates no script can run, and both belong to a slice that touches them:

- **`spec_validate`** for a slice that adds, moves or re-links a spec — dangling
  links, duplicate ids and parent cycles are what it catches.
- **The evidence families** — `sh scripts/check.sh real`, `real-render`,
  `real-1to1`, `real-discard` — for a slice that changes a byte-level claim.
  Editing a number in `docs/format-notes.md` means running the gate that measured
  it, not re-reading the claim. `.env.example` names the variable pairs.

## What goes where

- **A new format fact** lands in [`docs/format-notes.md`](docs/format-notes.md)
  with an evidence label before anything is built on it.
- **A new decision** lands as an ADR in [`docs/adr/`](docs/adr/), dated, with what
  it rules out.
- **A new term** lands in [`CONTEXT.md`](CONTEXT.md), which is a glossary and
  nothing else — no implementation detail.
- **A new module** lands with its spec in [`specs/modules/`](specs/modules/), and
  `crates/lrsalvage-core/tests/spec_graph.rs` checks its `depends-on` against the
  modules it actually reaches.
- **A new flag or `--json` field** lands with the README table it appears in;
  `lrsalvage-cli`'s `tests/readme.rs` compares both tables to the binary.
- **A test's fixture** composes a `Vault` from `crates/lrsalvage-fixtures`. A test
  that writes its own bytes is the finding, not the fix.

## Reporting a problem

Quote **which line and which version** — `lrsalvage --version` for the core line,
and the About panel's `app … · core …` pair for the app. Never paste a real
catalog path, a real file name or `scan --json` output into an issue: the
verification report exists so a run's *shape* can be shared with every path
replaced, and `.lrcat` files are somebody's photographs.

A bug in what the tool says is a claim being wrong, which this project treats as
the most serious kind of defect. A bug in a byte it writes is worse.

## Licence

Dual-licensed under MIT and Apache-2.0, and contributions are accepted under the
same terms. `cargo deny check` keeps the dependency tree honest about that.
