# Coding standards

The rules here are the judgement calls a check cannot hold. Mechanical rules
live in tooling, and a tool's rule is not restated here: `scripts/check.sh` holds
format, lint, tests, the doc build, spelling and licences; `tests/spec_graph.rs`
holds a module's `depends-on` to the modules it reaches; `lrsalvage-cli`'s
`tests/readme.rs` holds the README's flag and `--json` tables to the binary. When
one of those is wrong, fix the tool.

Read this file when reviewing a diff, or when writing a claim a stranger will
trust.

## Claims

A claim is anything a reader acts on: a README sentence, a format-notes bullet,
a commit message that says why.

- **One home per measurement.** The section that measured a number states it;
  every other section points at that section. A count in two places drifts, and
  the drift stays invisible until someone trusts the stale copy.
- **A claim about Lightroom carries a label** from
  [the evidence standard](docs/format-notes.md) — verified, reported,
  unverified or TODO — and a claim about *this machine* names the cache that
  produced it.
- **This repository's own machinery is a lookup, not a fact.** Point at
  `ci.yml`, `scripts/check.sh` or the workflow that runs it. A paraphrase of a
  trigger list or a gate set goes stale without anyone noticing.
- **What we chose never becomes what Adobe does.** A fixture pins our behaviour
  in a state; say so in the row and in the prose, because the state is ours until
  a real cache is measured in it.
- **A verified claim is re-measured, not re-read.** Changing a byte-level claim
  means running `sh scripts/check.sh real`, whose assertions back the numbers
  §5 of the notes publishes.

## Code

- **The module owns its format.** Parsing bytes stays in the module named after
  the format; `plan` decides and never learns a byte offset. See `AGENTS.md`.
- **A boundary change edits its spec in the same commit**, and the slice ends on
  a clean `spec_validate`.
- **A test composes a `Vault`.** A test that writes its own fixture bytes is the
  finding, not the fix.

## Reviewing

The two axes cite this file instead of re-deriving standards from six others:

- **Standards** — does the diff break a rule above? Quote the rule.
- **Spec** — does the diff do what the ticket asked, and only that?
