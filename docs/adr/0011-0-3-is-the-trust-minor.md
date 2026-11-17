---
id: adr-0011-0-3-is-the-trust-minor
type: architecture-design
title: "ADR 0011: 0.3 is the trust minor"
status: active
parent: architecture
references:
  - adr-0003-preserve-preview-bytes
  - adr-0007-publish-a-verification-report
  - adr-0010-what-0-2-promises
  - format-notes
  - glossary
---

# 0.3 is the trust minor

**Status:** accepted · **Date:** 2026-11-17

## Context

`0.2.0` shipped on 2026-11-17 and `vNext` then carried twelve open issues with no
ordering, no theme and no size estimate between them: a desktop GUI, three
questions about whether the published claims are true, three evaluation and
ergonomics items, three capability items, and a macOS proof. ADR 0010 lets a
**minor add** to the eight published surfaces, so every one of those twelve is a
candidate for the next release, and nothing outside a decision rule stops the list
growing while it is worked. 0.2 held its line ("checklist boxes plus the
milestone, and nothing else enters") only because the line was drawn first.

Three shapes were on the table, and one trap.

**A — the GUI's release.** #68 states its own destination: a desktop app for
someone who has never opened a terminal, driving `lrsalvage-core`, same output
bytes, never a second implementation. It ships on its own version line, and its
only stated prerequisite — the seam documentation slice — is closed.

**B — the tickets' release**, with a theme. Two honest candidates: *trust* (the
claims 0.2 publishes that no gate holds) and *ergonomics* (the tool as something
run every day).

**The trap — "the GUI plus the quick fixes".** The shape that arrives by default
if nothing is decided: a release whose size nobody estimated and whose theme
cannot be stated in one sentence.

What the decision rests on, measured rather than assumed:

- **The crate pages are blank.** Both manifests declare `repository` and
  `description` and nothing else — no `readme`, `keywords`, `categories`,
  `documentation` or `homepage` — and neither tarball carries a README, so
  crates.io reports `readme: false`. #96 holds the fix.
- **§5.11's counts are published and held by no gate** (#86): `recovered 10`,
  tiers `large 9` / `standard 1`, `thumbnail_pyramids 0`, `unmapped 0`,
  `missing_payload 0`, `below_floor 0`, exit 0. §5.4 carried a false claim for
  months because nothing measured it. Same shape.
- **`chrono` is unmaintained**, all versions, no upgrade offered
  (RUSTSEC-2026-0543). 0.2 shipped with the advisory ignored in `deny.toml`,
  dated, and nothing else tracks that ignore (#101).
- **Resume identity's hole is pinned and undecided** (#60, #66): a different
  photograph at the same output name and size is skipped as already recovered.
- **`--verify-source` is recorded as an additive minor** (#63): a new flag and two
  new report fields, which ADR 0010's minor tier allows.

## Decision

**Thirteen decisions, dated 2026-11-17. The first three fix the shape; the rest
fix what is in it, what is out, and what it may change.**

### D1 — `0.3.0` is the trust minor

Its theme is that **every claim it publishes is held by something a stranger can
run**, and its contents are the issues that either remove a published claim
nothing holds or prove the promise the tool has never proved: that the evidence
was never touched.

Rules out: the GUI as this release; ergonomics as this release's theme; and any
release whose theme cannot be said in one sentence.

### D2 — The GUI is not this release, and no CLI release waits for it

The app versions independently on ADR 0010's contract, as that record's third
corollary already says. Its destination is #68's map, its work comes from that
map's slice tickets, and no CLI milestone owns it.

Rules out: a GUI that ships a bundled CLI version number; a CLI release gated on
the app's schedule; and reading this as the GUI being deferred *by priority* —
it was not weighed against trust and found lower, it is simply not a release.

### D3 — The trap is rejected as a shape

"The GUI plus the quick fixes" is not a release and does not become one here.

Rules out: any release assembled by taking what is open rather than by theme, and
any release whose size nobody estimated.

### D4 — The contents are six issues

#96 (crate page and tarball), #86 (§5.11's counts gated), #101 (`chrono`
replaced), #60 (the resume limitation pinned), #63 (`--verify-source`), and #66
(a decision, not a change: resume identity keeps its shape).

Rules out: "while we are here" additions — #62, #64, #65 and #67 are explicitly
not in it, and neither is #93.

### D5 — #63 is in, and it is what makes this a minor rather than a patch

`--verify-source` adds a flag and two report fields, which ADR 0010's additive
tier permits in a minor. **Amended 2026-11-17:** the slice as it landed also adds
an `options` member and exit code `5`, recorded in D13's dated correction and in
ADR 0010's *added by 0.3.0* block. It is also the only user-visible change in the
release: without it, every item left is packaging, a gate and a dependency swap,
which ADR 0010's corollary says is a patch.

Rules out: a `0.2.1` carrying this work; hashing by default (a second read pass
over the catalog on every run); hashing the cache; and warning rather than failing
when a digest moves.

### D6 — #96 is in, and the fix is `exclude`, not `publish`

A README in each crate root, the five manifest fields, and `[package] exclude`
for the test tree that imports the stripped `lrsalvage-fixtures`.

Rules out: publishing `lrsalvage-fixtures`, which would turn a test-only crate
into a public dependency of the library; and a crate page that only links to the
repository, since the page is what a `cargo install` reader meets first.

### D7 — #86 is in as a fourth evidence family

`LRSALVAGE_DISCARD_*`, a `real-discard` arm of `scripts/check.sh` that fails
rather than skipping when its pair is unset, and the gate list in `AGENTS.md`
kept as long as the gates behind it.

Rules out: a sample of the discarded copy instead of the copy; §5.11 staying
**verified** prose with nothing asserting it; and a gate documented weaker than
it is enforced.

### D8 — #101 is in, and the advisory ignore leaves with it

`chrono` is replaced — it serves one function, `extract::set_capture_time` — and
the `[advisories] ignore` for RUSTSEC-2026-0543 is deleted in the same release
that stops depending on the crate.

Rules out: a second release shipping an ignored unmaintained advisory; pinning
the crate back to keep a number somewhere; and leaving the ignore in place
"because nothing calls it in anger".

### D9 — Resume identity keeps marker prefix + completeness + `SOF` dimensions

#66 closes **decided**, not deferred: the identity is shape, and the documented
rule is one destination per catalog. Strengthening it needs a report of the
failure it prevents, and none exists.

Rules out: putting the payload digest in the `Software` marker (it changes every
output byte and is a change of meaning under ADR 0010, needing its own preceding
minor to deprecate the old form); the `extract.csv` sidecar (a second resume path
that half-closes the hole and silently falls back when the CSV is absent); and
re-deriving the digest from the cache on every resume.

### D10 — #60 is in

The fixture row and the test that fails if the marker-prefix or dimension check
is weakened. Its README sentence landed in 0.2; the pin did not.

Rules out: closing #60 on the strength of the prose alone.

### D11 — Trusted publishing is in, as a slice

Both crate names now exist, which is the half crates.io could not do before: the
release workflow takes `id-token: write`, and a trusted publisher is configured
per crate, replacing the two manual 0.2 publishes.

Rules out: a third manual publish; a long-lived crates.io token in a workflow
secret; and treating the workflow as optional because 0.2 managed without it.

### D12 — Six refusals, each with its reason and its home

- **#62 shell completions and a man page** — a later minor. Artifacts and prose,
  and a man page is a claim surface that needs its own review pass before it is
  believed.
- **#64 free-space pre-flight** — a later minor. It adds a refusal path and a
  `--dry-run` branch: behaviour, not evidence.
- **#67 filters and naming templates** — a later minor's *exploration*. Its own
  record says no real user need is on record, and a rename template contradicts
  the rule that names come from the catalog.
- **#65 verify subcommand** — #68's horizon. Its own record says it becomes the
  GUI's first screen, which is where "are my files good?" gets asked.
- **#93 the macOS proof** — open, unscheduled. It needs hardware nobody has, so it
  can sit outside every theme without weakening it, and the README already states
  the gap in its own words rather than implying the page was proved.
- **#68 the GUI map** — its own version line, above.

Rules out: silence. A reader of `vNext` can tell why each refused issue is not
in the release without opening this ADR.

### D13 — The contract moves in one direction only

0.3.0 is **additive**: one flag and two report fields. None of ADR 0010's eight
surfaces is removed, renamed or given a different meaning, and the `Software`
marker's version — not its identity — is the only other thing that changes.

Rules out: a removal or rename riding along under a trust theme; and a patch
claiming any part of this release.

**Corrected 2026-11-17, while the slice landed.** D13's "one flag and two report
fields" undercounted what #63 needed. The comparison is `source_sha256` against
`snapshot_sha256` at acquisition — not before and after the run, which would fail
a run whose copy was perfect whenever Lightroom saved the catalog midway — and a
mismatch needs a code a script can branch on, so 0.3.0 also adds `options`
member `verify_source` and exit code `5`. Both are additive under ADR 0010, the
`0`–`4` rows keep their meanings, and the additions are recorded in that
record's dated block rather than only here. The flag reaches `extract` as well as
`scan`: the check is worth having on the run that writes, and on `extract` it
publishes nothing, because `report.csv` and `manifest.json` are not surfaces for
a digest.

## What this record does not decide

The **order** the slices land in, and the **checklist** a release is executed
against — both belong to the tracker issue this record produces, which is grilled
before a slice is written, as #89 was. Neither is decided here, and a reader
should not infer an order from the order above.

## Consequences

- **`0.4` has a candidate list before `0.3` ships**: #62, #64 and #67, with #65
  parked on the GUI's map. That is the difference between a deferral and a wish.
- **ADR 0010 is amended in place, dated.** It named shell completions,
  `--verify-source` and a free-space pre-flight as "0.3 minors"; 0.3.0 carries the
  second only, so that sentence now means *the next minor's contents*, not one
  release holding all three.
- **`0.2.1` stays available** for a defect that changes nothing in the contract.
  This record does not claim the version number; it claims what 0.3.0 is for.
- **The release's centrepiece rests on a copy on one machine.** #86 asserts
  §5.11 against the discarded copy of the `Auto (3840px)` render, the only discard
  ever measured here. If that copy cannot be stood up, this record is amended —
  the gate is not quietly downgraded to the prose it replaces.
- **#93 stays open after 0.3.0 ships.** The macOS route is proven on Linux and
  Windows and unproven on a Mac, and that asymmetry is a stated limitation rather
  than a release blocker.
- **A release can be about evidence rather than features.** 0.3.0 adds one flag;
  what it mostly does is make what 0.2 already claims checkable, which is the
  precedent worth carrying to 0.4 and to the app.

## Considered options

- **The GUI's release (shape A).** The honest version of "let the app be next".
  Rejected on two measured grounds: its own map still has eight bullets that are
  not decisions, so its size cannot be estimated; and the two debts with no owner
  — an unmaintained advisory 0.2 shipped ignoring, and counts no gate holds —
  would age with no release dated to fix them.
- **The ergonomics minor.** #62, #64, #67, #96. Rejected because every item is
  reversible without a promise, while the two items that are not (#101, #86) have
  no other home. A pleasant tool whose published numbers nobody checks is the
  tool that misleads politely.
- **The trap: the GUI plus the quick fixes.** Rejected outright. It is not a
  shape, it cannot be sized, and it is what happens when this question is not
  asked.
- **A `0.2.1` patch carrying the same work.** Rejected because it either drops
  #63 — the only substantive fix the trust set offers — or mislabels an additive
  change as a patch, which ADR 0010's corollary forbids.
