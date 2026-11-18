---
id: adr-0012-two-release-lines
type: architecture-design
title: "ADR 0012: Two release lines, and what 0.4 is for"
status: active
parent: architecture
references:
  - adr-0010-what-0-2-promises
  - adr-0011-0-3-is-the-trust-minor
  - app-distribution
  - glossary
---

# Two release lines, and what 0.4 is for

**Status:** accepted · **Date:** 2026-11-18

## Context

`v0.3.0` shipped on 2026-11-18, its milestone closed 9/9, and the board it left is
the first one this project has had where **nothing is scheduled and nothing is
overdue**. That is a different problem from 0.2's and 0.3's, which were both
"choose a shape before the list grows": here the list cannot grow past what is
already filed, and the question is instead *what each open thing is waiting for*.

Measured on the day this record was written:

| Item | Milestone | What it is | Contract class (ADR 0010) |
|---|---|---|---|
| #123 core tarball cannot run its own `cargo test` | `0.4.0` | five `#[cfg(test)]` sites in `src/` import the unpublished fixtures crate | **patch** — no listed surface moves |
| #88 `--max-long-edge` (a pull request) | `0.4.0` | one flag, `above_cap`, `options.max_long_edge` | **minor** — additive |
| #64 free-space pre-flight | `vFuture` | a refusal path reusing exit `4` | **minor** — additive, no new code |
| #62 completions and a man page | `vFuture` | artifacts and prose | **patch** by ADR 0010's corollary; a later minor by ADR 0011 D12 |
| #67 filters and naming templates | `vFuture` | decision 2 is #88; the rest declined | — |
| #65 `verify` subcommand | `vFuture` | an exploration, parked on the app | — |
| #93 the macOS proof | `vFuture` | needs hardware nobody here has | — |
| #68 the app map, #117 its spec | `0.4.0` | the Windows front end | **own version line** (ADR 0011 D2) |

Three things are wrong with that picture, and none of them is "there is not enough
to do".

**A CLI milestone holds the app.** #68 and #117 sit in `0.4.0` while ADR 0010's
third corollary and ADR 0011 D2 both say the app versions independently and no CLI
milestone owns it. The tracker has been contradicting the record since the moment
both were written, and a reader of the milestone list cannot tell that 0.4.0 is
about a binary.

**A merged-by-intent pull request has rotted.** #88 is `CONFLICTING` against
`main` at +742/−116 across 15 files. Its body says it "lands in **0.3.0**" and
that its merge is "held until `v0.2.0` is tagged" — both false, one of them twice.
It also adds `docs/adr/0011-a-cap-demotes-the-level.md`, and **0011 is the trust
minor**, so the number was taken while the PR sat.

**The patch route has now been passed over twice, and nobody has said why.** ADR
0011's Consequences left `0.2.1` available for "a defect that changes nothing in
the contract", and #123 is exactly that defect: an auditor who unpacks
`lrsalvage-core-0.3.0.crate` and runs `cargo test` gets a compile error. That is a
published artifact failing a promise the release's own checklist recorded. Either
it gets a patch, or the refusal gets a reason.

And one question is new, arriving with the prototype: **the app does not have to
be an executable.** The prototype in #117 is a single self-contained HTML file
with no framework and no build step, and a browser can read a catalog and write
JPEGs through the File System Access API. That changes the signing work, the
update story and the platform matrix at once, so it cannot be settled by
preference after the stack is chosen.

## Decision

**Thirteen decisions, dated 2026-11-18. The first three fix how the two lines are
told apart; the rest fix what is in the next release of each, and what is not.**

### D1 — There are two release lines, and this record names them

- **The core line** ships `lrsalvage-core` and `lrsalvage-cli` under one number:
  the crates, the archives, the checksums and the changelog sections that name a
  `0.x.y`.
- **The app line** ships the desktop front end under its own number, with its own
  artifacts and its own releases.

Both drive the same core and both are held by ADR 0010's eight surfaces. Neither
number says anything about the other, and no release on one line waits for a
release on the other.

Rules out: a single "product version"; an app release that renumbers the core;
reading "independent" as "unrelated" — they share the *contract*, which is a
stronger tie than a number.

### D2 — The next numbers are `0.4.0` for the core and `0.1.0` for the app

The app reports both, once, in the places a person looks when something went
wrong: its About panel, its receipt's diagnostics line, and anything it writes
that a bug report would quote — `lrsalvage app 0.1.0 · core 0.4.x`. The
`Software` marker continues to name the **core** version and nothing else
(ADR 0010's third corollary), which is what keeps a destination either front end
wrote mutually resumable.

The number says nothing about *what the app is*. Its shell, its installer and the
platforms it is tested on are the spike's and the stack's (D8, D10); its platform
scope today is #68's and #117's — Windows users first, macOS and Linux not planned
for `0.1.0`.

Rules out: `1.0.0` at the app's first release, which would claim a maturity the
recovery pipeline has not earned; the app mirroring core numbers, which would
produce a number that says nothing about the app; a "bundled version" that is the
app's own.

### D3 — The two lines are separated by milestone, not by a new vocabulary

Milestones are the authority: `0.4.0` and its successors hold core work,
`app-0.1.0` and its successors hold app work, and `vFuture` holds what has no
release. Three labels — `cli`, `app`, `core` — exist so a filtered list can cross a
milestone boundary. Every open milestone carries a one-sentence theme in its
description.

Rules out: an `App:`/`CLI:` title prefix, which would fight `Map:`, `Spec:` and
`Release tracker:`; a project board, which would be the third place the plan
lives; a second labels-per-release scheme.

### D4 — `0.4.0` is the daily-use minor

Its contents are **#88** (which closes #67's live decision), **#64**, **#62**, and
**#123** riding along as a defect repair. Its theme in one sentence: *the tool you
run every day, and the published crate that should behave like its repository.*
ADR 0011's Consequences pre-committed #62, #64 and #67 to this release, so this is
the promised release rather than a new list.

Rules out: "while we are here" additions; #65, #93 and the app entering it; a
release that grows while it is worked.

### D5 — #123 does not get a `0.3.1`, and the refusal is recorded here

The patch route is *available* — #123 moves no status word, exit code, field,
column, flag or output name — and it is refused on cost, not on class. A patch
costs a tag, three platform archives, `SHA256SUMS`, two trusted publishes, a
changelog section and a checklist, for five test modules. The alternative cost is
a published crate page that states the limitation in its manifest and is wrong
until `0.4.0` republishes core, which is stated in `CHANGELOG.md`'s `[Unreleased]`
section so the wait is visible rather than implied.

Rules out: annotating the published crate page (impossible — its readme is the
tarball's, and the tarball for 0.3.0 is fixed); bundling #123 into a patch that
carries a contract change; leaving the debt to be discovered by an auditor.

### D6 — #88 is repaired now and merged by `0.4.0`, not by preference

Rebase it onto `main`, **renumber its record to `0013`** (0011 is the trust minor;
this record is 0012), restate "lands in 0.3.0" as "lands in 0.4.0", and keep the
`0.2.0` hold-note only if it can be made true — it cannot, so it goes. Rebased, it
is 0.4.0's **only additive surface**: the flag, the `above_cap` stat (the frozen
set grows 17 → 18) and `options.max_long_edge`.

Rules out: merging a 742-line PR whose stated premise has rotted; editing history
instead of renumbering the file; landing a frozen-key change that no record
mentions.

### D7 — #67 closes when #88 merges

Its decision 2 was the only live part, and #88 lands the two README non-goals that
decision promised and never wrote. Decisions 1, 3 and 4 stand as declined and are
already recorded on the issue.

Rules out: keeping an exploration open for a decision that shipped; re-opening
the naming-template half without a product change, which decision 3 refuses.

### D8 — The app's path is spike → seam → stack, and no tracker before both land

#68 stays the map and the only thing a reader needs. The first ticket is the
**browser-or-desktop spike** (D10); the second is the **shared seam** — progress,
cancellation, typed errors, selected preview reads and writes, exact export
planning — which #117's Core contract makes a prerequisite to every implementation
slice; the third is the **stack**, which now includes the packaging, signing and
accessibility consequences of whichever shell the spike leaves standing. An
`app-0.1.0` tracker is written *after* the stack lands, because until then its
size cannot be estimated, and ADR 0011 rejected the app's release partly on
exactly that ground.

Rules out: a tracker written against an unsized plan; implementation slices before
the seam; a stack decision that predates the browser question.

### D9 — Signing, distribution and updates, decided before the shell is chosen

If the app ships as a shell: **SignPath Foundation** signs it — free, OV-level,
certificate issued to the Foundation, so the publisher a user sees is "SignPath
Foundation" rather than this project, one manual approval per release, and the
project publishes a code-signing policy with the attribution its terms require. A
**portable ZIP ships beside the installer**, so a wary user can run the binary
without installing anything. About carries a **link-only** update check.

Rules out: shipping unsigned to an audience that abandons on a warning screen;
self-signing, which Microsoft's own table says is worse than unsigned for public
users; paying before SignPath has been asked; a background updater in v1, which
needs a signed manifest and a hosting decision nobody has made; and — if D10 lands
on a browser — this whole decision, which becomes moot rather than negotiable.

The costs and conditions of each route, with a source and a date on every row,
are in `app-distribution` §1–§3. This record states the decision and does not
restate a price.

### D10 — The browser-or-desktop question is decided by a rubric, not by the spike

A spike answers *feasible*; it never answers *better*, and a green spike pulls
hard toward "we built it, ship it". Three tests decide, in this order:

1. **The feasibility gate**, mechanical. `lrsalvage-core`'s SQLite dependency
   compiles to `wasm32` against a demand-paged VFS over a File handle; a real
   `catalog.lrcat` opens and lists its images in a time a person tolerates; a few
   thousand JPEGs write through `FileSystemWritableFileStream` at a rate that does
   not read as a hang. Red on any one ends the browser path.
2. **The audience test**, measured. The whole browser case is "no install beats a
   warning screen", which is a claim about people, not bytes. It is tested by
   running the sample-catalog flow past people who are not developers — the
   usability test #117 already asks for — and watching first-run completion.
3. **The durability test**, judgement, written down. This tool's value lands once,
   possibly years later, on an old backup. A downloaded executable still runs in
   2030; a hosted page depends on this project still hosting it *and* on a
   Chromium API that at least one browser vendor has formally opposed making
   Baseline. That risk can be bought down — a static export, no telemetry, source
   public — but deliberately, in writing.

**Tie-break, pre-committed:** feasibility green *and* the audience test favouring
the browser means the browser ships as the 0.1.0 experience and an executable is
**deferred, not cancelled** — the UI is stack-agnostic, so a shell wrapping the
same HTML over the same core is a later slice, not a rewrite. A wash, or a
preference for the executable, means the executable ships and the web app becomes
the trial page #117's prototype already is.

The ledger this rests on, and the reason it is not obvious: the browser removes a
*second* platform matrix — three operating systems × installer, signing and, on
macOS, notarisation behind an Apple Developer account this project does not have,
which is the same missing hardware that keeps #93 open. It does not remove the
repository's existing matrix: `release.yml` still builds three archives for the
CLI whoever ships the app. And it does not remove work, it moves it — `wasm32`
becomes a fourth target `scripts/check.sh` must eventually keep green, with its own
SQLite build.

Every figure behind that ledger is in `app-distribution`, with the source and the
date it was read: §4 for the browser's constraints, §5 for which toolkits can keep
D11's accessibility promise at all. A row there that has gone stale is amended by
re-reading its source, never by editing the decision it fed.

Rules out: choosing on the ledger without a spike (the mechanisms are unverified
here); letting the spike choose; treating "deferred" as "decided against"; and
shipping two front ends at once, which doubles a release surface whose size nobody
has estimated.

**Added the same day, 2026-11-18, because the first draft of its execution list
got it wrong.** A milestone name and a version number carry **no stack and no
platform claim**. A milestone is where a release line's work lives; the shell, the
installer and the tested platforms belong to the spike and the stack, and #68's
destination — "a Windows desktop app" — is a *destination*, not a settled
description of the artifact. If D10 lands on a browser, that destination is
amended by a dated correction on #68 in the slice that records the decision,
never by widening what the map already promised.

The spike therefore does not decide whether there is an app. It decides what the
app is.

### D11 — Accessibility is a level, not a sentiment

The app targets **WCAG 2.2 AA** (the current W3C Recommendation; WCAG 3 is a
Working Draft) and holds it with a **conformance statement** that names which
criteria were verified by which observation — keyboard-only, a screen reader,
200–400% zoom and reflow, high contrast — and which remain unchecked. It carries
no "AA compliant" badge until the unchecked list is empty.

Rules out: "follows WCAG principles", which is unfalsifiable; AAA as a target over
a photo grid; and a conformance claim with no observation behind it, which is the
same failure this project names for byte-level claims.

### D12 — The app's output is gated against the CLI's, byte for byte

From the first app slice that writes a file, `scripts/check.sh` asserts that the
same vault, extracted through the app and through the CLI with the same options,
yields byte-identical trees, an identical `extract.csv` and an identical
`manifest.json` modulo the `Software` marker's version.

Rules out: "the app is never a second implementation" as prose; a GUI that grows
its own naming or metadata rule quietly; and a parity claim tested by hand once.

### D13 — One page explains the two lines, and it is not a second milestone list

`docs/versions.md` is the only place the structure is explained. It carries: what
a number means on each line; how a person finds the number they have; a
**capability** matrix — capabilities, never flags — whose core cells *link* the
README sections `readme.rs` already holds to the binary; the support window; and
one dated line naming the versions it was verified against. It may not enumerate
milestones, tickets or dates.

Rules out: a second copy of the flag table; a second milestone list; a table
nobody can tell is stale; and a page that explains the two lines in the README,
where the reader arrives mid-question.

## What this record does not decide

**The order `0.4.0`'s slices land in, and the checklist they are executed
against.** Both belong to the tracker issue this record produces, in #106's shape,
for the same reason 0.3.0's did: an order is a scheduling decision that depends on
what is in flight, and a second copy of a checklist drifts.

**The toolkit, and therefore the installer format, the signing execution and the
release workflow the app gets.** D10 gives the criteria and D9 the signing route;
neither names a toolkit, and naming one before the spike would repeat the mistake
#104 was written to avoid.

**The app's slice set.** #117 holds the acceptance list; it becomes slices after
the seam and the stack, not before.

## Consequences

- **`0.4.0` becomes readable.** It holds core work only, and the milestone that
  holds an app map and a spec is closed and replaced by `app-0.1.0`.
- **`0.4.0` is a minor because of #88, not because of its theme.** #64 adds a
  refusal path and #62 adds artifacts; without the cap flag the release would be
  a patch under ADR 0010's corollary. That is worth knowing when a later release
  argues about its own class.
- **ADR numbers shift.** This record takes `0012`; #88's cap record becomes
  `0013` when it is rebased. A reader who remembers an "0011 about a cap" is
  remembering a draft that never landed.
- **The `0.3.0` crate page stays wrong until `0.4.0` republishes core.** The
  mitigation is a changelog line, and it is the mitigation precisely because a
  published tarball cannot be corrected.
- **A browser app would make macOS reachable without a Mac**, which is the only
  route this project has to closing #93's shape of gap by *removing* the platform
  rather than proving it. It would not close #93, which is about the CLI's
  archives.
- **If the browser wins, `wasm32` joins the gate list.** That is a fourth target
  beside three operating systems — not a replacement for them, because the CLI
  keeps its matrix.
- **Nothing in the core line's plan moves because of the app.** D1's second half
  is the operative sentence: no release on one line waits for a release on the
  other.

## Considered options

- **The app's release now (shape A of #104, revisited).** Rejected again, and more
  sharply: #68's remaining bullets are three *decisions* — spike, seam, stack —
  so the app's size still cannot be estimated, and the browser question makes the
  estimate even less stable than it was when #104 first refused this shape.
- **A `0.3.1` for #123.** Refused in D5: available, and too expensive for what it
  buys. Recorded rather than silently skipped, because this is the second time the
  patch route has been left on the table.
- **One number for both artifacts.** Rejected. The tools that share a number —
  PhotoRec and QPhotoRec at 7.2 — are one release with two front ends. Independent
  artifacts skew in practice: ExifToolGui 6.3.12 over ExifTool 13.58, RawDigger
  1.4.10 over LibRaw 0.22.3, ripgrep 15.2.0 over its `grep` crate 0.4.1.
- **The Microsoft Store as the signing route.** Deferred, not refused: free
  registration and a package Microsoft re-signs with no SmartScreen warning is the
  only route that is both free and warning-free, and the MSIX file model for a tool
  that writes thousands of files into a folder the user picked is not verified
  here. It waits for the shell to exist.
- **Auto-update in the app's first release.** Deferred: an updater needs a signed
  update manifest and a hosting decision, and neither is worth buying before
  anybody has installed the thing.
- **Starting the app's tracker alongside the spike.** Rejected: a checklist
  against an unsized plan is the trap D3 of ADR 0011 named.
