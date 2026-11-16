---
id: adr-0010-what-0-2-promises
type: architecture-design
title: "ADR 0010: What 0.2 promises, and what may change"
status: active
parent: architecture
references:
  - adr-0003-preserve-preview-bytes
  - adr-0004-detect-layout-not-version
  - adr-0006-degrade-without-guessing
  - adr-0007-publish-a-verification-report
  - adr-0009-rescue-an-image-from-its-companion
  - format-notes
  - glossary
---

# What 0.2 promises, and what may change

**Status:** accepted · **Date:** 2026-11-15

## Context

`v0.1.0` is a tag on a tree that reads pre-13.3 caches. It has no release, no
artifact and no changelog, and 32 commits sit between it and today — the modern
reader, the five `layout` words, `--json`, `--report`, the cache stats, and the
companion rescue. So 0.2 is the first release a stranger can actually hold, and
the first one whose *promises* have to be readable rather than inferred.

Two facts make the promise question unavoidable now. First, the product *is*
being trusted: a script that parses `scan --json`, a support thread quoting a
status word, a later run that must recognise an earlier run's output, and a
verification report somebody else reads all depend on surfaces no gate currently
names as one set. Second, a desktop GUI for non-technical users is planned after
this release and will drive `lrsalvage-core` directly, which multiplies the cost
of a silent change on the same surfaces.

SemVer's pre-1.0 licence is available and is the obvious path: "anything may
change in a 0.x minor". That is honest about the version number and useless as a
promise, because the audience here cannot tell which minor is safe by looking.

## Decision

**`lrsalvage` declares its public API — its *contract* — as the surfaces below,
and holds them by the same rule at every version, 0.2 included.**

The contract:

1. **The six status words** a catalog image can end in — `recovered`, `suspect`,
   `original-present`, `no-cache-entry`, `missing-payload`, `failed` — and
   `unmapped` for a payload no image claims.
2. **The exit codes** `0`, `1`, `2`, `3`, `4` and their meanings.
3. **The `scan --json` field set**, "these fields, and only these".
4. **The verification report's key set** under the schema string
   `lrsalvage.verification`, and **`lrsalvage.manifest`'s envelope**.
5. **`report.csv` and `extract.csv`**, including column order and the meaning of
   `payload_metadata`.
6. **Output naming**: the mirrored folder tree, the ` - <WxH>` suffix on extra
   levels, the virtual copy's Copy Name suffix, and the numbered `report (2).csv`
   rule for a name already taken.
7. **The flags** each command takes, and which command takes them.
8. **The `Software` marker's identity** — an output naming this tool is ours —
   but **not the version inside it**: resume compares the prefix `lrsalvage `,
   which is what lets a later version keep the files an earlier one wrote
   (`adr-0003-preserve-preview-bytes` ¶5).

Two tiers govern change:

- **Additive** changes may land in a **minor**: a new field, stat, flag, status
  word or exit code, provided no existing one changes meaning. A new exit code is
  additive only under that proviso — the code table is closed at five today and
  a sixth is a decision, not a bump.
- **Removal, renaming, or a change of meaning** of any listed surface needs a
  **new minor with the deprecation stated in the preceding one**, and its own
  changelog line. Never a patch.

Three corollaries:

- **A patch changes nothing in this list.** Anything that does was mislabelled.
- **The schema strings are identities, not versions.** `lrsalvage.verification`
  and `lrsalvage.manifest` stay constant; a reader must ignore unknown fields,
  and only a removal or a changed meaning is breaking. The tool's own version is
  a separate field and says nothing about the schema.
- **The GUI versions independently, on this same contract.** It is its own
  artifact driving `lrsalvage-core`; a GUI release needing a new capability lands
  as a core minor, and the marker names the tool and the core version — so CLI
  output and GUI output are mutually resumable, and neither owns the promise.

Every contract surface is held by a gate where one exists: `readme.rs` for the
flag table and the `--json` field set, `specs/fixtures.md`'s matrix for every
status and exit code, `report`'s spec for the frozen key set, and ADR 0010 itself
for the items with no gate yet — **the exit-code table, `extract.csv`'s columns and
the output-naming rules are review-held, not gate-held** (`report.csv`'s columns
are not among them: `readme.rs` asserts that header), and that gap is part of
this record rather than a surprise later.

## Consequences

- `0.2.0` is `v0.1.0` plus the v2 work, and **`0.2.x` changes nothing a script
  can see**. The release's own product is that its claims are true of one fixed
  artifact.
- The whole 0.2 backlog is therefore prose, labels, gates and release machinery;
  the follow-ups that *do* add surface — shell completions, `--verify-source`,
  a free-space pre-flight — are 0.3 minors, decided after it.
- A contract item that loses its gate becomes a visible gap: the changelog's
  "changed" section and this list are reviewed together.
- The pre-1.0 escape hatch is explicitly declined. A 0.x minor that breaks a
  contract is a mistake to fix, not a licence taken.
- `1.0` is not a marketing step and stays unclaimed until the measured evidence
  covers the compatibility pages the README asserts.

## Considered options

- **Plain pre-1.0 SemVer discipline** ("0.x minor may break anything; only
  patches are safe"). Honest and cheap, and it tells the only audience that
  matters nothing: someone hand-editing a script against `--json` cannot tell
  whether 0.3 is safe from a number that is below 1.0 by definition.
- **Go to 1.0 and let convention enforce it.** The convention is stronger and the
  evidence does not support it yet — the compatibility table has rows no machine
  here can verify, and a 1.0 that overclaims is the failure this whole map exists
  to prevent.
- **Promote each surface to its own semantic version** (a `schema_version`, a
  `contract_version`). More precise, more to keep in sync, and consumers get two
  numbers that can disagree. One list with two tiers is enough until a real
  second consumer needs otherwise.
- **Declare no contract at all** and let the code be the spec. That is today, and
  it is why `readme.rs` had to be written after a README row promised a hint
  nothing printed.
