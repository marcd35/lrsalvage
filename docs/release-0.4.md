# Releasing 0.4

The 0.4 checklist, as one list a release can be executed against line by line. It
executes the slice order in
[#125](https://github.com/marcd35/lrsalvage/issues/125); its decisions are recorded
in [ADR 0012](adr/0012-two-release-lines.md) D4–D7 and
[ADR 0013](adr/0013-a-cap-demotes-the-level.md); the contract it may move is
[ADR 0010](adr/0010-what-0-2-promises.md). This file holds only what to run and
what to check off.

It is the output of the grilling on #125, in the shape
[0.3's](release-0.3.md) was, and that shape is written down in
[writing a release checklist](agents/writing-a-release-checklist.md). Where the
tracker already states something — the
slices, their order, the constraint list — this page links rather than restates,
because the two documents that disagree about a release are the release's own
problem. What the tracker cannot carry is a box, so the boxes are here.

**Every box below is either provable or gone with its reason written where the box
stood.** The grilling settled which is which, and it corrected six things a reader
with #125 open would otherwise meet as surprises; they are stated first for that
reason.

**0.4 is the daily-use minor**, and its theme is ADR 0012 D4's sentence: *the tool
you run every day, and the published crate that should behave like its
repository.* It adds exactly four things a script can observe — `--max-long-edge`,
`above_cap`, `options.max_long_edge`, and a free-space refusal that reuses exit
`4` — and removes, renames or re-means nothing. A fifth observable would be the
next minor's, and a slice that finds it needs one is a slice that has left this
release.

**`crates.io` carries `0.3.0` as this page is written.** Every box here that says
`0.4.0` is published is a box to be ticked after the publish, never a state of the
world; the 0.3 page needed a dated amendment to say what it had assumed, and this
one says it once, in advance, instead.

## Six things a reader with the tracker open will otherwise meet as surprises

1. **Two of the tracker's six slices had already landed when this list was
   written.** #88 merged as `dc5a8ad`, which is what closes
   [#67](https://github.com/marcd35/lrsalvage/issues/67) per ADR 0012 D7; #123
   merged as #129 and #131; and the cap's real-cache evidence merged as #132,
   which is `main` at `96f6a0a` and is what makes §5.12 of
   [format-notes](format-notes.md) a *verified* section rather than a synthetic
   vault's claim. The tracker's slice table says "my reading, to be grilled"
   above itself, and this page is that grilling: it **keeps the order** and
   records what is no longer in it. The executable remainder is four rows, not
   six: this file's own merge, then #64, #62 and the bump.

2. **The tracker's gate sentence and #62's own done-when disagree, and the gate
   wins.** #125 says "no new gate enters, except #123's change to `package`";
   #62's *Done when* says `sh scripts/check.sh` covers its generation step, and
   its recorded decision 2 says the artifacts are generated rather than
   hand-written — which only holds if something compares them to what the
   committed tree carries. So **one `completions` gate enters**, and #125's
   sentence is corrected where the slice box stands rather than left to be
   discovered by the person running it. The issue-body edit is drafted in
   [*Text held for approval*](#text-held-for-approval).

3. **#123 is three boxes, not the single sentence the tracker's done-list
   carries.** 0.3.0 measured the three clauses failing differently — `cargo build`
   passed while `cargo build --all-targets` and `cargo test` failed at five sites
   in `src/` — so one box would have hidden two of the three ways the property
   can regress. The third clause, the crate page and the manifest saying what a
   reader finds, is held by no gate at all.

4. **#62's and #64's own bodies name a horizon the release moved past.** Both say
   "Horizon: 0.3", and #62's decision 1 opens "A 0.3 slice, not 0.2". ADR 0011's
   Consequences pre-committed both to this release and ADR 0012 D4 kept them
   there; their milestone already reads `0.4.0`, so the board and the decision
   record agree and only the bodies are stale. The dated correction is drafted in
   [*Text held for approval*](#text-held-for-approval) and posted only on
   approval.

5. **The two Dependabot majors are this slice's, and both are merged into
   0.4.0's tree.** The tracker hands their disposition to slice 1: decide them, or
   close them with a reason written on the PR. Both merge, before the bump, on
   evidence re-run against today's tree — the green on each PR was produced
   against a base that predates this release. See
   [*The two Dependabot majors*](#the-two-dependabot-majors).

6. **The tracker's done-item 6 is not a universal.** "No sentence in `README.md`
   or `docs/` false at the tagged commit" is a claim about a search, and
   `CODING_STANDARDS.md` says an unsearched universal is the claim class reviewers
   find first. It becomes a **bounded sweep** with its file list and its
   exclusions, under [Docs](#docs--the-record).

## The slices this list executes

| Group below | Slices |
|---|---|
| Slices | #88 and #123 (both landed), #64, #62, and the bump |
| Evidence | `real-1to1` over §5.12's rule, and all four families re-run on the tag commit — a green from another tree is a green about another tree |
| Artifact | #123's tarball; four completion scripts and one man page; the three archives and `SHA256SUMS`; two publishes |
| Gates | `package` already tests core's tarball (it landed with #123), and one `completions` gate enters; no other script gate changes |
| Frozen surface | ADR 0010's dated *added by 0.4.0* block — a review, no slice |
| Docs | the README's exit-code row and its present-tense pointers, `docs/versions.md`'s two cells and its *verified at* line, ADR 0010, two module specs, the changelog |

## What is left, and in what order

The order is #125's table and is not repeated here. This one lists only what still
moves, plus the rows that have already moved:

| Slice | What it changes | Closes |
|---|---|---|
| 1 | **landed** `1249c24`, `21abb46`: this file, and the two Dependabot majors merged into the tree | — |
| 2 | **landed** `dc5a8ad`: the cap — flag, `above_cap`, the report's `options` echo, `docs/adr/0013`, and the two README non-goals #88 landed | #88, #67 |
| 3 | **landed** #129 and #131: the five fixture-dependent `#[cfg(test)]` modules moved out of `src/`, and `scripts/check.sh package` testing core's tarball rather than only building it | #123 |
| 4 | **landed** `4b70929`: the free-space refusal reusing exit `4`, the `--dry-run` verdict, the README's exit-code row, and one seam the suite can drive | #64 |
| 5 | #62: four completion scripts and one man page, generated, the `completions` gate, the release's upload list, one README line | #62 |
| 6 | The bump: `0.4.0` in the workspace manifest and both `lrsalvage-core` pins, `CHANGELOG.md`'s `[0.4.0]`, `docs/versions.md`'s two cells | — |

### The tracker's done-list, mapped

#125's *What "done" means* is seven items, and none of them is a box a person can
tick as written — four are acts, two are claims and one is this page's own rule.
Each is therefore one of the sections below, and the mapping is stated once so a
reader closing the tracker can check coverage without re-deriving it:

| Tracker's item | Where it is proved |
|---|---|
| 1. Every box ticked, or deleted with its reason | this page's rule; it has no box of its own |
| 2. `v0.4.0` annotated, three archives, `SHA256SUMS` and the five generated files attached | [Making the tag](#making-the-tag) |
| 3. Both crates on crates.io, through the trusted-publisher job | [After the tag](#after-the-tag-irreversible) |
| 4. `cargo install lrsalvage-cli --version 0.4.0` proved with no checkout | [After the tag](#after-the-tag-irreversible) |
| 5. `cargo test` green in an unpacked `lrsalvage-core-0.4.0.crate` | [Artifact](#artifact--what-a-stranger-can-download)'s three boxes, and the appendix [Checking the 0.4.0 tarball by hand](#checking-the-040-tarball-by-hand) |
| 6. No sentence in the README or `docs/` false at the tagged commit | [Docs](#docs--the-record), as a bounded sweep with its exclusions |
| 7. The tracker closed with the release link | [After the tag](#after-the-tag-irreversible)'s closing bookkeeping |

## Before the tag

### Slices

- [x] **1 — this file, and the two majors.** This file is merged; #4 and #5 are
      rebased, re-run and merged, one at a time, in the order
      [*The two Dependabot majors*](#the-two-dependabot-majors) gives. Nothing
      else enters this slice: the checklist's own merge is the release's first
      act, and a major that merged after the bump would be a dependency the
      tagged tree's evidence never met. *Verified 2026-11-20:* #5 merged as
      `1249c24` from head `2bcc0d2`, cut from `main` at `e1e065f`; #4 merged as
      `21abb46` from head `51789a0`, cut from `1249c24`. Each head carried the
      whole gate list green — its last line naming the tree it measured, `(tree
      2bcc0d2 plus 0 changed)` and `(tree 51789a0 plus 0 changed)` — and a green
      CI run on that same head. `msrv` was green on `rust-version = 1.89`, which
      covers both floors [*The two Dependabot majors*](#the-two-dependabot-majors)
      records, and `package` was green on both. `deny.toml` did not change:
      `deny` ended `advisories ok, bans ok, licenses ok, sources ok` with the
      crates `sha2 0.11.1` brings in the tree, and `base64 0.23.1`'s
      `simd-unsafe` feature is empty in that version. The licences that question
      turns on are recorded in
      [#154](https://github.com/marcd35/lrsalvage/pull/154), the slice's record
      pull request, rather than reprinted here.
- [x] **2 — #88, landed.** The flag `--max-long-edge` on `scan` and `extract`,
      `above_cap` on the summary, `scan --json` and the report's `stats`,
      `options.max_long_edge` in the echo, and `docs/adr/0013` renumbered with its
      dated amendment. Its spec surface landed with it: `specs/modules/plan.md`
      states the winner rule and both bounds, `specs/modules/report.md` the frozen
      list and the echo, and `specs/fixtures.md` carries M16–M18, L7 and the
      companion-rescue note. *Measured 2026-11-18:* `specs/modules/cli.md` names
      the cap nowhere, so a later slice that assumes the flag is written down
      there will not find it — `readme.rs` is what holds the README's flag table
      to the binary, and that is the flag's gate. *Verified 2026-11-18:*
      `dc5a8ad` is on `main`, `docs/adr/0013` exists with its renumbering note,
      and #67 is closed.
- [x] **3 — #123, landed.** The five fixture-dependent `#[cfg(test)]` sites left
      `src/` for `tests/`, so an unpacked tarball builds `--all-targets` and runs
      the suite it carries; `scripts/check.sh package` now **tests** each tarball
      where it is unpacked, outside the checkout, and the cli's against the core
      tarball beside it. *Verified 2026-11-18:* `fdd1593` and `e1ac3bc` are on
      `main` and the `package` gate's own body does the build-and-test it used to
      do for the cli only. The three boxes this release must still **tick** are
      under [Artifact](#artifact--what-a-stranger-can-download): landing the fix
      is not the same act as proving the tarball named `0.4.0`.
- [x] **4 — #64, landed.** ADR 0012 D4 puts it in this release
      because the app's save screen depends on it, so its acceptance criteria are
      fixed before it is built. **The mechanism, once:** the run compares the
      plan's byte total against free space on its destination and refuses with
      exit `4` before the first write.
      - **The check is `extract`'s, and it runs once the plan exists.** It sits
        with the destination validation in
        `crates/lrsalvage-core/src/extract.rs`, because that module owns
        destination safety and because the byte total it needs is the plan's;
        `plan` stays I/O-free and contributes `stats.bytes` and nothing else. It
        runs before the first file is written, so no JPEG, no `extract.csv`, no
        `manifest.json` and no report exists when it fires. The output root may
        already exist — today's unwritable-destination refusal runs after
        `create_dir_all` — and this box does not claim otherwise.
      - **The number is the plan's, not the catalog's.** `--min-long-edge`,
        `--max-long-edge` and `--all-levels` move it, because it is what this run
        would write at these options. A run that would write nothing is the empty
        window that already exits `3`, so it meets this check with nothing to
        compare.
      - **Corrected 2026-11-19, in the slice's own pull request:** the number is
        the plan's total **less the outputs the destination already holds**,
        because an existing destination is skipped when it verifies as ours and
        failed when it does not, and neither rewrites the file. A resume is
        therefore measured by the bytes it has left; without that, a first pass
        that filled the destination would refuse every later resume for the bytes
        it had already written. `--force` subtracts nothing, since a forced run may
        replace what is there. *Found by the two-axis review of the slice.*
      - **The message carries the shortfall twice**: in bytes and in a human form.
        Exit `4`, no sixth code (ADR 0010 closed the table at six).
      - **A destination with exactly enough space proceeds.** The comparison has
        no margin, and a boundary test asserts it at equality rather than near it.
      - **"Nearly full" is said, not claimed to fit.** The verdict's third state
        prints where a user sees it, and the box names the exact wording in the
        slice's pull request, because a warning nobody reads is not a check.
      - **`--dry-run` runs the same check and prints the same verdict**, writing
        nothing — #64's decision 4, and the reason the check sits with the
        destination validation rather than inside the per-file write loop, where
        `--dry-run` would never reach it.
      - **Free bytes arrive through one function the suite substitutes.** This is
        the seam the grilling chose: no CI runner can be made nearly full, so
        without it the refusal would rest on one hand measurement and no gate. A
        workspace test drives planning to the refusal and asserts the error
        variant; `main.rs`'s mapping test already pins that variant to `4`, so
        the exit code is held by the pair rather than by the seam alone. The
        fixture matrix gains a row whose **cannot show** text records that a
        short disk is not a vault state a fixture can compose.
      - **The surface is exactly one widened trigger.** The README's exit-`4` row
        gains the free-space clause, the flag table gains nothing, `readme.rs`
        passes, and `specs/modules/extract.md` records the check and its place in
        the order. `CHANGELOG.md`'s `[Unreleased]` gains its **Added** line in the
        same pull request.
      - **The ticket's own horizon is corrected in the pull request that
        implements it**, from the drafts in
        [*Text held for approval*](#text-held-for-approval).
      - *Verified 2026-11-19:* `4b70929` is on `main` — the slice's whole gate
        list green on the commit it merged from (`c33a8f4 plus 0 changed`), all
        four evidence families green on that same tree, `spec_validate` clean,
        and the horizon correction posted. The box's two dated corrections are in
        the same slice: the number is what is left to write, and the refusal
        reuses `error::Error::Destination` rather than a new variant. The slice's
        own pull request is #145.
- [x] **5 — #62, landed.**
      - **Four completion scripts and one man page, generated from the clap
        definition** — `bash`, `zsh`, `fish` and `powershell` — and no hidden
        subcommand, so ADR 0010's flag-and-command surface does not grow. The
        generation is a build step, not a release-time script someone remembers.
      - **One `completions` gate enters `scripts/check.sh`.** It regenerates into
        a temp directory and diffs against the committed files, so a committed
        artifact that no longer matches the binary fails a run rather than a user.
        The gate's red arm is **proved once** in a scratch copy of the tree — a
        gate nobody has seen fail is a hope, and drift with no failing test is
        what `readme.rs` was written to stop.
      - **The man page's synopsis and flag list are derived**, and its exit-code
        section has **one source**, named in the generator. A hand-typed second
        table there is the `readme.rs` drift again; the codes come from
        `exit::PUBLISHED`, and the test that holds the two together is
        `main.rs`'s `the_published_exit_numbers_are_pinned_and_each_code_has_a_case`,
        which pins the six constants and asserts `PUBLISHED` covers them.
      - **`release.yml` uploads the five files by explicit path, not a glob.** A
        `dist/*.tgz`-shaped addition would sweep a completion script into the
        archive set; the asset list itself is written once, in the Artifact box
        below, and this slice adds five files to it.
      - **The archive names do not move.** `release.yml`'s own comment explains
        why: the name is a promise to `cargo-binstall`, a tool this project does
        not control.
      - **The README's Checks section names the new artifacts in one line**, which
        is #62's own done-when, and the ticket's horizon is corrected in the same
        pull request.
      - *Verified 2026-11-19:* `294414b` is on `main` — the squash of [#148](https://github.com/marcd35/lrsalvage/pull/148),
        whose head `e870d46` carried the whole gate list green and all four
        evidence families green on that same tree. `spec_validate` is clean. The
        `completions` gate's red arm was proved four times: a renamed flag, a file
        dropped into the committed directory, a renamed `release.yml` path, and a
        removed `-text` rule. The horizon correction was posted on #62 on
        2026-11-19, from [*Text held for approval*](#text-held-for-approval). The
        slice's retrospective turned two of its findings into assertions on the
        same gate and one into [#152](https://github.com/marcd35/lrsalvage/pull/152)'s
        `shell` check.
- [x] **6 — the bump.** `0.4.0` in the workspace manifest and in both
      `lrsalvage-core` pins (`lrsalvage-cli`, `lrsalvage-fixtures`, whose
      requirement must match or the workspace will not resolve), `Cargo.lock`
      re-read; `CHANGELOG.md`'s `[Unreleased]` becomes `## [0.4.0]` with a fresh
      empty `[Unreleased]` above it; and every assert that names a version moves.
      *Measured 2026-11-18:* `rg '0\.3\.0' crates/` finds the two pins and the
      comment in `crates/lrsalvage-core/Cargo.toml` that records what was measured
      on 2026-11-17 — the pins move, the comment is history and keeps its number.
      *Measured 2026-11-19:* it also finds the man page, twice —
      `crates/lrsalvage-cli/artifacts/lrsalvage.1` carries `lrsalvage 0.3.0` in its
      `.TH` line and `v0.3.0` in its `VERSION` section, both from `clap`'s
      `version`. So this bump ends by regenerating the five artifacts —
      `cargo run -p lrsalvage-cli --example artifacts -- crates/lrsalvage-cli/artifacts`
      — and `sh scripts/check.sh completions` is the gate that says so if it is
      forgotten. The artifacts move; like the comment above, no `0.3.0` of
      history is rewritten in prose.
      The changelog section's content is decided below, under
      [Docs](#docs--the-record); the bump does not invent it. *Verified 2026-11-20:*
      the bump is `b64f3ae` — `0.4.0` in the manifest and both pins, `Cargo.lock`
      re-read as five workspace lines, the changelog renamed with an empty
      `[Unreleased]` above it, the man page regenerated in two tokens, and one
      assert outside `crates/` named after a version —
      `crates/lrsalvage-web/tests/baseline.rs`'s `outputs_sha256`, which moves
      because the `Software` marker carries the version. **The box stays unticked
      until that commit is on `main`**, because that is the evidence slice 4's and
      slice 5's `Verified` lines name.

*Measured 2026-11-20 on `b5164c7`:* slice 6 landed as #155 — `0.4.0` in the
workspace manifest and in both `lrsalvage-core` pins, `Cargo.lock` re-read,
`## [0.4.0]` above a fresh `## [Unreleased]`, and the five generated artifacts
regenerated, so the man page carries `0.4.0` in its `.TH` line and in its
`VERSION` section. The one `0.3.0` left under `crates/` is the 2026-11-17
measurement comment in core's manifest, which is history and keeps its number.

### Evidence — what the release may claim

- [x] All four families are green on the commit the tag will land on: `sh
      scripts/check.sh real`, `real-render`, `real-1to1` and `real-discard`. Each
      fails rather than skipping when its own pair is unset, and none is borrowed
      from another. The gates are named in `scripts/check.sh`'s header, which is
      the one inventory of them.
- [x] **`real-1to1` is green on that commit, not merely on the tree #132
      measured.** §5.12 is a *verified* section whose numbers are asserted by that
      gate — `crates/lrsalvage-cli/tests/one_to_one.rs`,
      `a_cap_chooses_the_largest_level_at_or_below_it` — so the box re-runs the
      gate rather than re-reading the section. This release adds no byte-level
      claim of its own: §5.12 is the section 0.4.0's work added, and it changed on
      2026-11-18 with the evidence that produced it.
- [x] No byte-level claim is restated in the release body. The body is the
      changelog section, and the sections it could quote stay in
      [format-notes](format-notes.md) — the rule 0.3's page states and this
      release keeps, because a number in two places drifts and the drift is
      invisible until someone trusts the stale copy.
- [x] Nothing the tag publishes carries a catalog path or `scan --json` output: the
      body is the changelog section, the assets are archives, `SHA256SUMS`, four
      completion scripts and one man page, and no report file is attached. The
      verification report's path-freedom is why a run's shape can be quoted at
      all; a real path cannot.

*Measured 2026-11-20 on `b5164c7`, a clean tree — every run printed
`plus 0 changed`:* the four families are green on their own pairs (`real` 1
passed, `real-render` 1, `real-1to1` 2, `real-discard` 1), with `real-1to1`
re-run rather than borrowed from the tree #132 measured. The extracted body
carries no path-like string and quotes none of §5.12's steps: its cap entry
points at `format-notes` §5.7 and leaves `1,920` / `1,024` / `4,096` where they
already live — the README's flag row and the notes.

### Artifact — what a stranger can download

The three boxes of #123's *Done when*, in the order an auditor meets them. They
are three because 0.3.0 measured them failing differently, and one of them no gate
holds.

- [x] **The gate.** `sh scripts/check.sh package` packs both crates, unpacks each
      tarball outside the checkout, builds it with `--all-targets` and runs its
      suite there. It is the property's gate, and it is green on the tag commit.
      This box is provable on any tree and proves the tree's own version; it is
      not the same claim as the artifact box below.
- [x] **The named artifact.** `lrsalvage-core-0.4.0.crate`, unpacked into a
      temporary directory with no checkout of this repository in reach, answers
      `cargo build --all-targets` and `cargo test` green — both recorded with the
      commit they ran on, in the appendix
      [*Checking the 0.4.0 tarball by hand*](#checking-the-040-tarball-by-hand).
      This is the act 0.3.0 could not perform and the one the tracker's done-item
      5 names.
- [x] **The surface.** No crate page and no manifest says any longer that a
      published tarball cannot test itself. *Measured 2026-11-18:* the sentence
      that used to stand — the *Known limitation* entry ADR 0012 D5 leaned on —
      is gone from `CHANGELOG.md`, and
      `crates/lrsalvage-core/Cargo.toml`'s packaging comment now records the
      measurement and points at the gate that holds it. The box re-reads both at
      the tag, because this is the clause a green gate cannot hold.
- [x] `cargo package --list -p lrsalvage-core` and `-p lrsalvage-cli` list each
      crate's `README.md` and no test file that imports the fixtures crate — the
      `package` gate asserts both of those, so this box's evidence is that gate's
      run rather than a list reprinted here.
- [x] The cli's tarball is verified against the **core tarball the same run
      built**, not against whatever the index carries. The `package` gate does
      this with a `--config patch`, and the reason is the script's own: a version
      that is not yet on the index would otherwise be verified as the previous
      one.
- [x] The tag produces the release's assets: the three platform archives and
      `SHA256SUMS` that `release.yml` already builds, plus the five files #62
      attaches — `lrsalvage.bash`, `_lrsalvage`, `lrsalvage.fish`,
      `_lrsalvage.ps1` and `lrsalvage.1`, generated from the binary's own `clap`
      definition, committed under `crates/lrsalvage-cli/artifacts/` and held to it
      by `scripts/check.sh completions`. **This is the one place in this checklist
      the list is written** — the tag, binstall and changelog boxes point here
      rather than recounting it — and the archive names stay the shape
      `release.yml`'s comment
      explains they must keep. `release.yml` names the same five paths because a
      workflow cannot read prose: that is this list's executable form, and
      `if-no-files-found: error` is what fails a tag whose generator wrote one
      fewer. `SHA256SUMS` stays the archive set's: a checksum
      file whose shape changed with a new kind of asset is a different promise, so
      the five are attached beside it, not inside it.

*Measured 2026-11-20, packed from `b5164c7`:* the tarball was copied to a
directory outside every checkout and unpacked there, where `cargo build
--all-targets` finished and `cargo test` ran four suites — 203 + 2 + 5 passed, 1
ignored, none failed. `cargo package --list` lists each crate's `README.md`, and
core's two packed test files import no fixture crate, because the
fixture-dependent tests sit in the manifest's `exclude`. `Known limitation` is
gone from `CHANGELOG.md`, and core's manifest comment records the 2026-11-17
measurement and points at the gate that holds it.

### Gates — the ones a script can run

- [x] `sh scripts/check.sh` green on the tag commit: every gate the file names, in
      the order it names them, with a missing tool failing rather than skipping.
      The run's last line is the evidence, and it prints the tree it measured —
      that is what makes two runs over two trees distinguishable afterwards.
- [x] `sh scripts/check.sh msrv` green on the `rust-version` the manifest
      publishes, after the two majors land: `sha2 0.11` raises its own floor and
      `base64 0.23` moves its own, and a dependency that raises the published
      floor is a decision rather than a pin. The gate reads the number out of
      `cargo metadata` and builds on it, so the contribution is re-proving it, not
      restating it.
- [x] `sh scripts/check.sh package` green — it is the gate #123 changed and the
      gate two of the artifact boxes lean on, and its two builds are why it is
      named while iterating and carried at hand-off.
- [x] The `completions` gate green, and its red arm proved once (slice 5). It is
      the one gate this release adds, against #125's sentence that none would; the
      reconciliation is recorded in the first list on this page and in the tracker
      comment drafted below, so the contradiction is stated rather than quietly
      resolved.
- [x] `spec_validate` reports no issues across `specs/` — no dangling link,
      duplicate id or parent cycle, including the rows and module specs this
      release edits. **It is the gate no script can run**: it is an in-app tool,
      so neither `scripts/check.sh` nor CI calls it, and the slice that re-links a
      spec ends by asking it rather than assuming `tests/spec_graph.rs` answered.
- [x] `cargo test -p lrsalvage-core --test spec_graph` and `cargo test -p
      lrsalvage-cli --test readme` green after the documentation edits: a module's
      `depends-on` matches what it reaches, and the README's flag and `--json`
      tables match the binary. These are the two tests that hold a docs pull
      request to the code, and `readme.rs` is the reason the README's tables are
      not hand-kept twice.
- [x] `typos` and `cargo deny` green with their policy files unchanged: a new
      domain word goes into `typos.toml` and never into a blanket exclusion, and a
      dependency that trips `deny.toml`'s allow-list is a decision rather than a
      config edit. `base64 0.23` turns on a default SIMD feature it did not have
      before — `cargo deny` is the check that says whether that changed the
      licence or advisory picture. **Corrected 2026-11-20:** this said the answer
      is recorded in the pull request that merges the bump, and a Dependabot PR
      carries the bot's body and nothing of a review's — so the answer is
      recorded in the slice's record pull request,
      [#154](https://github.com/marcd35/lrsalvage/pull/154), which is where
      slice 1's box points.

*Measured 2026-11-20 on `b5164c7`:* `all gates green: fmt shell clippy test doc
wasm build completions package typos deny msrv (tree b5164c7 plus 0 changed)`.
`spec_validate` reports no issues across `specs/` — the gate neither a script nor
a CI job can call — and the three named tests are green on the same tree:
`spec_graph` 7 passed, `readme` 3 passed, `links` 1 passed.

### Frozen surface — what 0.3 promised, still promised

- [x] ADR 0010's eight surfaces reviewed against the release's whole diff: nothing
      removed, renamed or given a different meaning, and the additions are exactly
      the four the introduction names, each of them in ADR 0010's dated *added by
      0.4.0* block. The review names what it searched — the `#[arg]` sites for
      flags, the exit-code constants and their mapping, and the frozen `stats`
      list in `specs/modules/report.md` — because "nothing else moved" is a claim
      about a search.
- [x] The frozen `stats` list's growth is the decision ADR 0013 records, not a
      field that appeared, and ADR 0012 D6 is the one home for how many names it
      holds before and after: this page points at it rather than counting.
      *Corrected 2026-11-20:* this named ADR 0013 as the home for the count. 0013
      records the decision and carries no number — the number is in D6, which is
      where ADR 0010's block points.
- [x] A destination a 0.3.0 run wrote is still recognised and skipped byte for
      byte, because the marker comparison stops at the prefix. Held by
      `extract`'s `a_preexisting_verified_file_is_skipped_and_its_bytes_untouched`,
      green on the tag commit, and the release's own marker moves only the version
      inside it.
- [x] The README's exit-code row for `4` and its flag rows are accurate, and
      `readme.rs` holds the parts a test can hold. The row's prose is review-held
      — that gap is stated in the README's compatibility contract and is not
      newly created here.


*Reviewed 2026-11-20 over `v0.3.0..b5164c7`, 112 files:* by named search rather
than by reading the diff. The six exit constants are identical at both tags, and
`crates/lrsalvage-cli/src/exit.rs` is a moved path rather than a new surface. No
status word, `--json` row, report key, CSV header or naming rule moved except
the two additions, and exactly one flag row was added with none removed — the
row `readme.rs` holds to the binary. `stats` holds the eighteen names ADR 0013
says it holds, the eighteenth being `above_cap`.

### Docs — the record

The tracker's done-item 6 becomes this list, and **what it does not cover is
stated with it**: `docs/release-0.2.md`, `docs/release-0.3.md` and the ADRs'
dated history keep their `0.3.0` references, because they are records of what was
true then. This is a sweep of what a reader takes as the present, not a rename.

- [x] The six sentences that name the 0.3 checklist — five in `README.md`, one in
      `release.yml` — are re-read and repointed where they describe *this* release,
      and kept where they describe 0.3.0's own measurement. *Measured 2026-11-18,
      and each classified:* `README.md`'s frozen-surface sentence and its "the
      commit a tag is cut on is chosen by…" sentence are present-tense and move,
      as does `release.yml`'s comment naming the checklist the tag's commit is
      chosen by; `README.md`'s archive-check sentence, its `cargo install
      --version 0.3.0` sentence and its version paragraph describe 0.3.0's own
      runs and stay until the post-publish box rewrites them. *Moved 2026-11-20 by
      the bump's own slice:* `README.md:676`, `README.md:785` and
      `.github/workflows/release.yml:30` now name `docs/release-0.4.md`, and the
      three that describe 0.3.0's own runs are untouched.
- [x] `docs/versions.md`'s two `0.4.0` capability cells become proved rows **in
      the bump slice** — the cap names the flag and links the README section
      `readme.rs` holds; the free-space row names the refusal — because they
      describe the tagged tree. Its *verified at core 0.3.0* line, the sentence
      naming which release is current, and its support paragraph's example move
      **after the publish**, because they are claims about a released artifact.
      *Measured 2026-11-18:* `CONTEXT.md` names neither version and so carries no
      box here; the pages that do name one are the README, `CHANGELOG.md`,
      `docs/versions.md`, `docs/format-notes.md`, the ADRs and the two earlier
      checklists, and each is classified either by the paragraph above this list
      or by the `[Unreleased]` box below. *Verified 2026-11-20:* at
      `docs/versions.md:97` the cap's cell is *yes*, linking the flag to
      `../README.md#quick-start`; at `:110` the free-space row says the run
      refuses with exit `4` before it writes anything, naming the shortfall, and
      links the exit-code table. The *verified at* line, the current-release
      sentence and the support paragraph's example are untouched for the
      post-publish box.
- [x] ADR 0010 gains its dated *added by 0.4.0* block naming the four additions,
      in the shape 0.3.0's block took — flag, stat, `options` member and the
      widened exit-`4` trigger — with the sentence that says no existing surface
      changed meaning. *Verified 2026-11-20:* the block is *Added by 0.4.0 —
      2026-11-20*, under 0.3.0's, and it points at ADR 0012 D6 for how the frozen
      set grew rather than counting it.
- [x] **ADR 0012 D7's attribution is corrected with a date**, in this slice's own
      pull request: the two README non-goals are #67's decision 1, and decision 2
      is the cap. *Found by the review of this page rather than by a gate*, and
      the gap is structural: an ADR's cross-reference to an issue's decision
      number names text that is not in this tree, so no test can compare the two.
      *Verified 2026-11-18:* the correction is in D7 and the wrong pointer is gone
      from the record.
- [x] `specs/modules/extract.md` records the free-space check and where it sits in
      the order; `specs/modules/cli.md` records what `--dry-run` now prints, if it
      prints something new; and `specs/fixtures.md` carries the `cannot show` row
      the seam's slice adds. The cap's specs need no edit: they landed with #88.
      *Verified 2026-11-20:* all three stand as written —
      `specs/modules/extract.md:91` names `extract::ensure_room` and the plan's own
      byte total, `specs/modules/cli.md:151` has the verdict in both modes with
      `:162` saying `--dry-run` prints the same lines, and
      `specs/fixtures.md:245` is M19 with its *cannot show a short disk* text. The
      bump edits no spec.
- [x] `CHANGELOG.md`'s `[0.4.0]` section carries three headings, because the
      section is the release body verbatim:
      **Added** — the cap, `above_cap`, `options.max_long_edge`, and the
      free-space refusal as a new trigger for exit `4`;
      **Changed** — the `Software` marker names `0.4.0`, as 0.3.0's body carried
      its own line;
      **Packaging** — #123's tarball running its own suite, the two dependency
      majors if both re-runs are green, and the five generated files if they are
      attached. *Verified 2026-11-20:* the section carries those three and a
      fourth, **Build**, and the correction is stated here rather than left to be
      discovered: #142 and #144 — the filesystem seam and its `wasm32` half —
      landed after this box was written, they are in the tree the tag ships, and a
      release body that omitted them would be a body about another tree. The bump
      renamed the heading and added only what this box enumerates plus the
      section's lead paragraph, so the fourth heading moves no decision this page
      recorded.
- [x] `[Unreleased]` is a heading three files name, and the bump renames it.
      *Measured 2026-11-18:* `format-notes.md` §5.12 names it in the present tense
      and moves; ADR 0012 D5's two references are a dated record of where the wait
      was stated and may stay, gaining one dated amendment only if the sweep finds
      a reader who would take them as the present. *Verified 2026-11-20:*
      `CHANGELOG.md` opens with an empty `## [Unreleased]` above `## [0.4.0]`,
      `format-notes.md` §5.12 now names the `--max-long-edge` work as 0.4.0's, and
      the sweep found the reader that paragraph describes: D5's first paragraph
      pointed at the heading in the present tense, so it carries its dated
      amendment.
- [x] Each slice corrects its own ticket's stale sentence in the pull request that
      implements it, from the drafts in
      [*Text held for approval*](#text-held-for-approval) — #64's and #62's
      horizon lines, and #125's no-new-gate sentence where the `completions` gate
      lands.

*Measured 2026-11-20:* the three horizon corrections are posted — #64's, #62's,
and #125's no-new-gate sentence. Of the six sentences that name the 0.3
checklist, the three that were present tense now name this release (the README's
frozen-surface sentence, its tag-commit sentence, and `release.yml`'s comment),
and the three that describe 0.3.0's own runs stay. `docs/versions.md`'s two
capability rows became proved rows in the bump slice, ADR 0010 carries its dated
*Added by 0.4.0* block, and the three module specs record their surfaces. Its
*verified at* line and its current-release sentence move after the publish, not
before it.

## Making the tag

The tag and everything under it are a maintainer's acts, each on its own go-ahead:
the agent prepares the command, runs every gate it can run first, and verifies the
outcome afterwards. One approval for "the release" is not one for an irreversible
upload.

- [x] The tree the tag lands on is measured, not inherited: `sh scripts/check.sh`
      green on that commit after it is merged and before a tag exists, all four
      evidence families green on it, `cargo publish --dry-run -p lrsalvage-core`
      packaging and stopping at its dry-run line, and the release body read from
      the extractor `release.yml` itself runs rather than by eye.
- [x] `CHANGELOG.md`'s `## [0.4.0]` section is the release body, verbatim: the
      published body's first and last lines are that section's, and the heading
      after it did not travel into the release.
- [x] The tag is `v0.4.0`, annotated, on the merge commit of the close-out pull
      request — the tree published from, which carries the bump, the changelog and
      the upload list by construction rather than by a later check.
- [x] The release publishes exactly the assets the Artifact box enumerates, and
      **nothing generated from `scan`** — a report describes a cache, not a
      release.

*Measured 2026-11-20 on `b5164c7`, for the two boxes above that are ticked:*
every gate green, the four families green, `cargo publish --dry-run -p
lrsalvage-core` reaching `warning: aborting upload due to dry run`, and the body
taken from the extractor `release.yml` itself runs — 118 non-blank lines opening
`## [0.4.0]` and stopping before `## [0.3.0]`, with **four** headings rather than
the three this page predicted: `Added`, `Changed`, `Packaging` and `Build`, the
last being the `wasm32` seam work that landed this release.

*Then, 2026-11-20:* the tag went on **`b5164c7` itself** — the measured commit,
not a later one — after a rehearsal tag proved the chain and skipped the publish
job (0.3.0 was still the index's latest afterwards). The real run attached nine
assets: the three archives, `SHA256SUMS`, and the five generated files, with
nothing generated from `scan`, and all three archives verify against `SHA256SUMS`.

The tag is the commit the measurements name rather than the one that records
them, which is why this page's ticks are not in the tagged tree: 0.3 tagged its
close-out squash the same way, and a page that names its commit is worth more
than a commit that contains its own evidence.

## After the tag (irreversible)

0.3.0 was the first release whose publishes ran through OIDC, and the record in
[0.3's checklist](release-0.3.md) is what that route cost: two refusals before a
trusted publisher existed, then both uploads through the workflow. Nothing about
this release changes that machinery, so the stop rules below are 0.3's, adapted to
what is now known.

- [x] `lrsalvage-core` then `lrsalvage-cli` 0.4.0, from a clean checkout of
      `v0.4.0` rather than a working tree, published by the trusted-publisher job
      in `release.yml`, with no token in the repository. The order is not a
      preference: the cli resolves `lrsalvage-core 0.4.0` through the index.
      **Stop rule one.** If the job fails to authenticate on `lrsalvage-core`, the
      first question is whether a trusted publisher still exists for that name —
      the 0.3.0 attempt answered `No Trusted Publishing config found` and the fix
      was a human entry, not a re-run. If the entry is present and the job still
      refuses, the fix is a scoped, short-lived token for that one release, and
      the workflow defect becomes its own issue. Never a yank of a version that was
      never uploaded.
      **Stop rule two.** If core lands and the cli publish fails, the fix is
      forward: correct the tree and publish `lrsalvage-cli 0.4.0`. If core itself
      is defective — building from the repository but not from the tarball — the
      artifact box above is the one that would have caught it, and `0.4.0` cannot
      be re-uploaded: yank it, fix it, and release `0.4.1` with its own tag and
      body.
- [x] `cargo install lrsalvage-cli --version 0.4.0` works with no checkout in
      scope: the build comes from the registry, `lrsalvage --version` prints
      `lrsalvage 0.4.0`, and the install root's `.crates.toml` records the
      registry source. Use `--root` so nothing is installed over the user's own
      binary.
- [x] `cargo binstall lrsalvage-cli` is tried once against the tag, and its stop
      rule is the README's: if it does not fetch the prebuilt, the paragraph says
      so rather than keeping an expectation. It is re-run this release because the
      asset list grew — by the five files the Artifact box names — which is exactly
      the shape that could disturb the template match, and because 0.3.0's run
      proved the bare form, not this asset set.
- [x] The post-publish docs pull request: the README's install, archive and
      version sentences name 0.4.0 and only what was proved of each route, its
      version paragraph points at this checklist rather than the 0.3 one — the
      sentence 0.3.0 needed a post-publish pull request for — and
      `docs/versions.md`'s *verified at* line, current-release sentence and
      support example follow the publish. Nothing here says a route exists that
      was not run, and the macOS pair stays unproved with #93 still open.
- [x] `rust-version` and `rust-toolchain.toml` re-checked against the dependency
      tree the two majors leave behind, so the next release does not inherit a
      stale claim.
- [x] The tracker's closing bookkeeping: #64 and #62 closed with the release link,
      and #125 closed with it — **not** before. #88, #123 and #67 closed when they
      landed, and #4 and #5 close on their own merges. Everything this release
      does not carry stays open **and is named when the tracker closes**, rather
      than left to be inferred from its absence: #93 (the macOS proof), #65 (the
      `verify` exploration) and #68 and #117 (the app map and its spec), all
      outside ADR 0012 D4's contents. #126 used to be on this list: the spike
      closed as completed on 2026-11-20, and
      [#141](https://github.com/marcd35/lrsalvage/issues/141) — the audience test
      — took over as the app line's deciding ticket.
      *Closed 2026-11-20, after the release:* #64 and #62 carried the release link
      and closed as completed, #125 closed with it once the tag was out, and the
      five that stay open were named on the tracker rather than left to its
      absence — #93, #65, #68, #117 and #141.

*Then, 2026-11-20, after the tag:* `publish` completed green — `lrsalvage-core`
then `lrsalvage-cli`, each skipped if the index already carried it — and both are
`0.4.0` on crates.io. The install was proved where no checkout was in scope: the
binary built from the registry, answered `lrsalvage 0.4.0`, and `.crates.toml`
recorded `registry+https://github.com/rust-lang/crates.io-index`. `cargo binstall
lrsalvage-cli@0.4.0` downloaded the prebuilt archive from the release in under
three seconds rather than falling back to `cargo install`, so the nine-asset list
still matches `binstalk`'s template and the stop rule has nothing to report. The
`msrv` gate was green on this tree, so `rust-version` needed no second number.

The closing bookkeeping below stays open until the tracker closes: it is the one
box whose evidence is the three issues closed *after* this page is written.

## The two Dependabot majors

The tracker hands their disposition to this slice: decide them, or close them
"with a reason written on the PR". **Both merge, before the bump, into 0.4.0's
tree** — the release's first slice, listed as such above, because a major that
landed after the bump would be a dependency the tagged tree's evidence never met
and a release body that does not mention it.

What each is, measured 2026-11-18:

| | #4 | #5 |
|---|---|---|
| Bump | `base64` 0.22.1 → 0.23.1 | `sha2` 0.10.9 → 0.11.1 |
| Where it is used | only the contact sheet's data URI, in `report.rs` | the two digests, in `snapshot.rs` and `extract.rs` |
| Its own floor | MSRV 1.71 | MSRV 1.88 (this workspace publishes 1.89) |
| What the bump changes | a default `simd-unsafe` feature it did not have | the digest API's versions, not its output |
| Its green today | seven checks, on a base 9 commits behind `main` | seven checks, on a base 16 commits behind `main` and older than `v0.3.0` |

**The evidence is the re-run, not the green.** Both branches predate this
release's tree — #5's base predates `v0.3.0` itself — so the green standing on
each is a statement about a tree nobody is tagging. Both rewrite `Cargo.lock`, so
whichever merges first makes the other rebase; that is the cost of taking them in
order rather than as a group, and Dependabot's grouping config does not group
majors.

**The order is #5 then #4.** `sha2` is the one whose floor meets the `msrv` gate
and whose gap from `main` is the larger unknown, so it is the one worth meeting
on a tree where a failure is unambiguous. `base64` follows, and it is the one to
re-read `deny.toml` against, because a default-on SIMD feature is a build-level
change the licence and advisory policy has not seen.

**What each merge must show, and where it is recorded** — the merging pull
requests are Dependabot's, so the evidence sits in the slice's record pull
request, [#154](https://github.com/marcd35/lrsalvage/pull/154), and the merge
comment on each PR states the three gates without the run's line. *Corrected
2026-11-20:* this said "in the pull request that merges it", which for a bot's PR
is a place no evidence can be written. This is the replacement for a green on a
stale base:

- CI green on a run whose merge base is the current `main`.
- `sh scripts/check.sh msrv` green, with the floor the bump publishes compared to
  `rust-version`.
- `sh scripts/check.sh deny` green, with `deny.toml` unchanged — if it is not
  unchanged, the change is a decision recorded in the pull request.
- `sh scripts/check.sh package` green, because a dependency a tarball resolves
  without a `path` is a dependency the artifact can fail on.

### Text held for approval

Each item below is posted only on the maintainer's explicit approval of that
exact action. The items marked **Posted** are on the tracker and keep the text
that was approved, so a reader can compare the two.

**On #5 and #4, if the re-run is green — the merge comment.** One comment, on each
PR, at the moment it is merged. **Posted 2026-11-20**, verbatim, on both:

> Merged into 0.4.0's tree as part of
> [#125](https://github.com/marcd35/lrsalvage/issues/125)'s first slice, on a
> re-run whose merge base is current `main` — the green that stood here was
> produced against a base that predates this release. `docs/release-0.4.md`
> records the decision and the order; `msrv`, `deny` and `package` are green on
> the merge commit.

The squash-commit subject stays Dependabot's own, unchanged — `chore(deps): bump
sha2 from 0.10.9 to 0.11.1` and `chore(deps): bump base64 from 0.22.1 to 0.23.1` —
so the tree's history says what changed without a second description.

**On either, if the re-run is red — the close comment.** Held in reserve, because
a rebase can reveal what a stale base hid:

> Closing rather than merging: on a re-run against current `main` this bump fails
> `<gate>`, with `<the failure>`. 0.4.0's dependency set therefore stays what
> 0.3.0 shipped, and the bump reopens when the failure is understood. Recorded in
> `docs/release-0.4.md`.

**On #64 and #62 — the horizon correction.** One comment each, posted when the
slice's own pull request lands, so the ticket and the tree agree. #64's is on the
tracker. #62's is **Posted 2026-11-19**, verbatim:

> **Corrected 2026-11-19:** this says "Horizon: 0.3" and the milestone reads
> `0.4.0`. ADR 0011's Consequences pre-committed it to this release and
> [ADR 0012](https://github.com/marcd35/lrsalvage/blob/main/docs/adr/0012-two-release-lines.md)
> D4 kept it there; `docs/release-0.4.md` carries its acceptance criteria. Nothing
> in its recorded decisions changes.

**On #125 — the gate sentence.** One comment, posted when #62's gate lands.
**Posted 2026-11-19**, verbatim:

> **Corrected 2026-11-19:** this issue says "no new gate enters, except #123's
> change to `package`". #62's recorded decision 2 requires the generated artifacts
> to be compared against the committed ones, which is a gate; `completions`
> therefore enters `scripts/check.sh` with that slice.
> `docs/release-0.4.md` records the reconciliation.

## Checking the 0.4.0 tarball by hand

The artifact box's evidence, and the act 0.3.0 could not perform: an unpacked
`lrsalvage-core-0.4.0.crate` that builds its test targets and runs its own suite.
Run it from a temporary directory and **not** from inside a checkout — an unpacked
tree inside this repository can walk up to `specs/` or to another member and pass
where an auditor's would fail. `scripts/check.sh package` stages its copy the same
way, for the same reason.

```console
cargo package -p lrsalvage-core
mkdir -p <empty directory outside any checkout> && cd <that directory>
tar -xzf <path to target/package/lrsalvage-core-0.4.0.crate>
cd lrsalvage-core-0.4.0
cargo build --all-targets
cargo test
```

Record, beside the box: the commit the tarball was packed from, the commands'
outcomes, and the fact that no checkout was in reach. **What this does not
prove:** that the copy on the index is this copy. The upload is a different act,
its own box, and what ties the two together afterwards is the `v0.4.0` tag and
the checksum crates.io records for the upload.

The gate is the property's keeper and this is the artifact's one measurement, not
its duplicate: `package` proves whatever version the working tree's manifest
names, on whichever machine runs it, and this proves the file a stranger
downloads. 0.3.0's box 2 is why both exist — the gate it had built a tarball and
did not test it, and the box it carried had claimed more than that.
