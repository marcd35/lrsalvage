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
- **An inventory belongs to the command that prints it.** Prose names the command
  — `cargo package --list`, `scripts/check.sh`, `lrsalvage scan --json` — and
  never reprints the list. A sentence that says what a package, a gate set or a
  flag set *contains* is a cache with no gate behind it, and the reader acts on
  the day it goes stale.
- **A universal claim is a claim about a search.** "No suite keeps a private
  fixture", "every file", "nothing else in the tree" assert the result of
  looking. Name the counterexample you checked, or write the bounded claim — "no
  *new* writer", "apart from the pair that pins…". An unsearched universal reads
  as fact, and it is the claim class reviewers find most often because the tree
  contradicts it in one line.
- **A value computed and never shown is a claim with no evidence.** When a spec
  sentence names something a person sees, the diff renders it rather than only
  deriving it: a `reconcile()` whose result nothing reads is a promise the page
  breaks silently, and three comments saying otherwise are the bug.
- **What we chose never becomes what Adobe does.** A fixture pins our behaviour
  in a state; say so in the row and in the prose, because the state is ours until
  a real cache is measured in it.
- **A verified claim is re-measured, not re-read.** Changing a byte-level claim
  means running the family that measured it — `sh scripts/check.sh real` for most
  of §5, and the gate the section itself names where it is one of the others.
- **A document that prescribes work names one owner per decision.** Two boxes
  that place the same check in different modules is not emphasis; it is a
  contradiction the implementer settles by guessing. The same goes for acts: a
  box sits under the heading of the moment it happens, so a sentence that may only
  be written after a publish is not a *before the tag* box. A checklist written
  to remove ambiguity is the one document that cannot carry any.

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
