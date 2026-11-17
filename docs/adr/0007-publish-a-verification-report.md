---
id: adr-0007-publish-a-verification-report
type: architecture-design
title: "ADR 0007: Publish a path-free verification report, and no second way to redact one"
status: active
parent: architecture
references:
  - adr-0004-detect-layout-not-version
  - adr-0006-degrade-without-guessing
  - format-notes
---

# Publish a path-free verification report

**Status:** accepted · **Date:** 2026-11-14

## Context

ADR 0004 ends with the observation that a maintainer can check the tool's
compatibility claim against a stranger's cache if the stranger hands over the
layout and both version strings: *"Users who paste a `scan --json` into an issue
hand over the layout and both version strings, which is what makes a verification
report possible without full paths."* That sentence names a reader and then hands
them the wrong surface. `scan --json` carries `catalog` and `previews` as
**absolute paths** (`crates/lrsalvage-cli/src/main.rs`, `scan_json`), so pasting it
into a public issue publishes a stranger's folder names, and with them often their
real name.

The facts that made this worth deciding rather than leaving to prose are measured
(ADR 0004, `format-notes` §5): the modern cache is
`<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>_<longedge>`, 78 images hold 390 level
files, the digest is store-wide and therefore identifies nothing, and the
`_<longedge>` suffix matched the measured `SOF` on every file checked. So the
**shape** of a cache carries almost all of the verification value, and every
character of it is already public knowledge — which means a report can be useful
while naming no payload at all.

Two more things pushed the design away from the obvious shortcut:

- A redaction switch (`--json --redact`) leaves two ways to produce the same
  object, and the failure mode is someone picking the wrong one and pasting paths
  anyway. A promise enforced by remembering a flag is not a promise.
- `unrecognised` (ADR 0005 ¶6) is a count of files that took **no** shape. Putting
  it in a histogram of shapes would give "no shape" a bucket in a table whose
  meaning is "which shape this was".

This decision creates a new public file format — a thing strangers will paste and
maintainers will compare across releases — which is why it is an ADR and not only a
module spec.

## Decision

`scan --report <file>` writes one JSON file: the **verification report**.

- **Schema `lrsalvage.verification`**, `version` = the tool version that wrote it,
  `generated_unix` — the same envelope as `lrsalvage.manifest`, so a consumer
  learns from one key which facts a file is expected to hold.
- **A curated set, frozen in `report`'s spec, not a superset of `--json`**:
  `layout`, `db_version`, `crs_version`, the `options` the run applied,
  the plan's `stats`, and `shapes`. No `items`, no `catalog`, no `previews`, no
  path of any kind. Adding a field is a decision, recorded there.
- **`shapes` is one object per recognised shape class** — `level_file` with its
  `count` and how many of those names agree with the measured `SOF`, `container`
  with its `count` — and each class carries its **grammar template**:
  `<uuid[0]>/<uuid[0..4]>/<UUID>-<digest>_<longedge>`. The template is a constant
  owned by `previews_db`, the module that owns the naming rule; `report` prints it
  and never paraphrases it. A mixed cache therefore publishes both templates,
  which is exactly the case a single module-level grammar string would have
  described wrongly. `unrecognised` stays in `stats` where ADR 0005 put it.
- **Nothing in the file identifies a cache or a payload.** The only place a name
  appears is a template with `<uuid…>`, `<digest>` and `<longedge>` placeholders.
  Path-freedom is enforced twice: the writer builds the report from a struct with
  no path, leaf, uuid or *payload* digest field to fill, and a fixture row asserts
  the written bytes contain none of the vault's absolute paths, uuids, digests, shard
  folders or leaf names.
  **Amended 2026-11-17 by #63.** The struct now carries one digest pair —
  `source_sha256` and `snapshot_sha256`, the catalog's own hash and our private
  copy's, measured by `snapshot` and printed as lowercase hex. It is not a
  payload digest: no cache, uuid or leaf is derived from it, and hex carries no
  path, so the property this record exists for is unchanged. The field set in
  `report`'s spec stays frozen and adding a member is still a decision; the
  curated set above is simply the set as of 0.3.0, and that spec is where the
  list lives.
- **`--json` does not change.** It stays the stdout surface, path-bearing by
  design, gaining only the fields ADRs 0004 and 0006 already promise. One job per
  surface: stdout for a human or a local script, a file for a stranger's issue.
  There is no `--redact`.
- **The report never decides an exit code.** It is written whenever `--report` is
  given and a plan exists — on exit 0 and exit 3 alike, because a run that
  recovered nothing is the run a stranger most needs to hand over. A write failure
  is exit 4, through the same `at_destination` mapping every named destination
  uses; a run that exits 1 before a plan exists writes nothing.
- **It obeys the destination rule and is claimed.** The parent is validated like
  `--out` and `--contact-sheet` (never inside the catalog, the cache or
  `.lrcat-data`), the name takes the next free number (`scan (2).json`) and says
  so, and an unwritable path is exit 4.
- **`scan` writes only what you name.** The old invariant was "one artifact at
  most"; the rule that was doing the work is that `scan` never invents a write.

## Consequences

- A report is comparable between users and between releases: same keys, same
  meanings, no local facts. Two strangers' files can be read side by side.
- The frozen list cuts both ways. A newer tool may add a field, so a consumer must
  read absent-as-absent and use `version`, never absent-as-zero; a maintainer who
  needs more than the report can always ask for `--json` and hand over paths
  deliberately.
- There is no redaction code to get wrong, because there is nothing to redact:
  no module ever hands `report` a path.
- A new stat, or a new payload shape, now needs a line in a module spec before it
  can appear in a published file. That friction is the point.
- The fixture matrix grows rows whose assertion is the **absence** of a string,
  the first negative rows in the suite, and it pins two grammars rather than one.
- `docs/format-notes.md` §5.2 becomes the spec a third party is checking against:
  a report that disagrees with it is a measurement contribution, which is what
  issue #2's verification plan asked for.

## Considered options

- **`--json --redact`, and `--report` writes that.** One object code path, but two
  ways to produce a pasteable summary and a switch whose whole job is to be
  remembered. The wrong pick publishes the paths the feature exists to withhold.
- **A maintainer's diagnostics dump: `--json` plus shapes, minus paths.** More
  debugging power for an odd cache, at the cost of a larger file, a format that
  tracks every internal stat, and fields whose meaning only we know — and every
  new field becomes a redaction surface to re-check.
- **Plain text, two-space separated lines.** Friendliest to paste by hand and the
  only option that is not a new parseable format, but then a fixture asserts prose
  and a consumer cannot tell a field it does not know from a line that moved.
- **Only the minimum counts** (images, recovered, unrecognised). Enough to say
  "it read 78", not enough to verify the shape of the cache — which is the whole
  question the report exists to answer.
- **One grammar string for the whole cache.** Smaller, and wrong on a mixed
  cache: the run would describe one of the two shapes it saw.
