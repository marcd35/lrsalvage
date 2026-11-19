# Writing a release checklist

`docs/release-<minor>.md` is one list a release is executed against, line by
line. This page is how one gets written and how one gets run; the checklists
themselves are the authority for their release, and the tracker issue is the
authority for its slices and their order.

The rule that shapes the whole file: **every box is either provable or deleted
with the reason written where the box stood.** A box nobody can tick and nobody
dares delete is the failure mode this rule exists to prevent.

`CODING_STANDARDS.md` governs the claims in it — one home per measurement, a
universal is a claim about a search, an inventory belongs to the command that
prints it. Read that first; this page does not repeat it.

## Writing one

1. **Read the tracker and the previous checklist.** The tracker holds the slices,
   their order and the constraints; the previous `docs/release-*.md` holds the
   shape. Neither is restated in the new file — where the tracker already says
   something, the new page links to it.
2. **Fix the release's surface sentence before anything else** — the one or two
   things it adds that a script can observe, and the sentence that nothing is
   removed or re-meant. Every slice box is judged against that boundary, so a
   slice that needs a fifth surface is a slice that has left the release.
3. **Turn each done-item into a box, and say where it is proved.** A tracker's
   *What done means* is acts, claims and rules rather than checkables; map each
   one to the section that holds it, so a reader closing the tracker checks
   coverage instead of re-deriving it.
4. **Give every remaining slice its acceptance criteria in the same pull
   request.** That is a checklist's first slice's whole job: the tickets get
   judged against these boxes, so a criterion that cannot be observed is the
   defect the review will find.
5. **Split the acts: before the tag, the tag, after the tag.** Every irreversible
   act gets its own go-ahead, and a box that may only be written once a publish
   has happened sits under that heading — never above the tag.
6. **Name each gate by pointing at `scripts/check.sh`.** Never reprint the gate
   list, the flag set or a package's contents; those are lookups with a command
   behind them, and a copy is a cache with no gate.
7. **Record the state of the world with a date and a commit**, and correct the
   tracker's own stale sentences where the box stands — the order it gives, a
   count it names, a horizon a ticket has outgrown.

## The facts, and where they live

| Fact | Its home |
|---|---|
| Archive names, the changelog-body extractor, the release's upload list | `.github/workflows/release.yml` |
| The gate list, and what each gate measures | `scripts/check.sh`'s header and body |
| What CI runs | `.github/workflows/ci.yml` — it calls `scripts/check.sh` |
| What a capability does, and which version has it | `README.md`'s tables (held by `tests/readme.rs`), `docs/versions.md` |
| Byte-level numbers and their evidence labels | `docs/format-notes.md` |
| The contract surfaces and what may change | `docs/adr/0010-what-0-2-promises.md` |
| A ticket's recorded decisions | the issue body |
| The spec graph's integrity | `spec_validate` — an in-app tool, so no script and no CI job can call it |

## Two timings for a version sentence

A version in prose describes one of two things, and they change at different
moments:

- **The tagged tree.** Capability cells, a changelog section, a spec — these
  move in the bump slice, because they describe what the commit contains.
- **A released artifact.** Install and archive sentences, a *verified at* line,
  "both crates are on crates.io at that version" — these move after the publish,
  because until it happens they would be false, and the release's own rule is
  that no sentence is false at the tagged commit.

## Box shapes

- **A command and its expected outcome**, with the tree it was measured on: a
  later reader has to be able to tell two runs over two trees apart.
- **A file and the sentence in it**, quoted, rather than a paraphrase.
- **A deleted box keeps its reason and its date**, where it stood, including the
  measurement that made it undeletable-or-gone.
- **A ticked box carries its evidence**: a commit, a run's last line, a file:line.

## Housekeeping

`.githooks/pre-commit` already runs `fmt`, `shell` and `typos` on every commit, so
running those three by hand before a commit is redundant; `sh scripts/check.sh`
with no arguments at hand-off is not. The four `real-*` families need this
machine's own caches, and each fails rather than skipping when its pair is unset.
