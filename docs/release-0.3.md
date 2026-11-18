# Releasing 0.3

The 0.3 checklist, as one list a release can be executed against line by line. It
is the output of the grilling on
[#106](https://github.com/marcd35/lrsalvage/issues/106), and its decisions live in
[ADR 0011](adr/0011-0-3-is-the-trust-minor.md); the contract it may move is
[ADR 0010](adr/0010-what-0-2-promises.md). This file holds only what to run and
what to check off.

Every box below is either provable or gone with its reason written where the box
stood. That grilling settled which is which, and it corrected the release's own
shape in five places it is worth stating up front, because a reader who has the
issue open will otherwise meet them as surprises:

- **`0.3.0` adds more than D13's sentence says.** ADR 0011 D5 and D13 describe "one
  flag and two report fields". Measured against the tree, a flag cannot land
  without its echo in the report's `options` (the frozen object echoes
  `min_long_edge`, `all_levels` and `include_present` today —
  [the report spec](../specs/modules/report.md)), and #63's "a digest change fails
  the run" needs a non-zero exit that none of `0`–`4` means. The surface 0.3.0
  adds is therefore **one flag, one `options` member, two digest fields and exit
  code `5`**, and D13 is corrected with a date in the slice that lands them. It is
  still additive: nothing in ADR 0010's eight surfaces is removed, renamed or given
  a different meaning.
- **§5.11's discarded copy no longer exists.** The gate #86 promised rests on a
  copy that was deleted with the scratch tree it lived in. It is regenerable, and
  this page's Evidence section says how — but the numbers are re-measured before
  they are asserted, and the copy that was measured is not the copy that is kept.
- **#96's own "patch release" sentence is wrong.** It says its fix reaches
  crates.io in a patch; ADR 0011 D6 puts it in `0.3.0`, and slice 2 corrects the
  ticket in its own pull request.
- **#66 is a record, not a change.** Resume identity keeps marker prefix +
  completeness + `SOF` dimensions, and the paragraph that says so lands in ADR
  0003, where the rule already lives.
- **`0.2.0`'s milestone is still open** and there is no `0.3.0` milestone. Slice 1
  fixes both, because a reader of `vNext` should be able to tell what is in this
  release without opening an ADR.

**0.3 is the trust minor.** Its theme is that every claim it publishes is held by
something a stranger can run, and what it mostly does is make what 0.2 already
claims checkable: the counts §5.11 published, the promise that the evidence was
never touched, the crate page that was blank, the advisory 0.2 shipped ignoring,
and the resume limitation that shipped as prose. One surface is added
(`--verify-source`); nothing is taken away.

**The version on `crates.io` is `0.3.0`**, and every sentence on this page that
names a version follows the publish rather than leading it. *Amended 2026-11-17,
after the tag and after the publish:* the tag, the GitHub release and both
registry uploads have happened and are recorded below; the page now describes a
release that did happen, and it says so in the past tense where it records an
act.

## The slices this list executes

Eight slices, plus the acts below the tag. Each is one pull request; the version
bump and the changelog section are boxes inside the last, not a ninth slice,
because a `0.3.0` bump that lands early produces a tree claiming a version whose
boxes are still open.

| Group below | Slices |
|---|---|
| Slices | #96 (the crate page and tarball), #101 (`chrono`), #60 (the resume pin), #63 (`--verify-source`), #86 (the fourth evidence family), #66 (the identity record) |
| Evidence | #86 — and §5.11's numbers re-measured rather than inherited |
| Artifact | #96, and trusted publishing, which 0.2 deliberately cut |
| Gates | #86's `real-discard` arm, and no other — every other gate already exists |
| Frozen surface | ADR 0010's dated *added by 0.3.0* block — a review, no slice |
| Docs | `CONTEXT.md`'s two digest terms, four README sentences, ADR 0003's amendment, every restatement of the fourth family |

## What was left, and in what order

The order is dependency, not preference. Slice 1 is where the other seven get
their acceptance criteria; slices 2 and 8 have to be in the tagged tree because
the tarballs and the release body come out of that tree; slice 5 precedes the bump
because the report's `version` and its field set are asserted together, so a bump
landing first fails a test that is telling the truth.

| Slice | What it changes | Closes |
|---|---|---|
| 1 | This file: every box provable or cut, the milestone bookkeeping, and the six tickets' membership | — |
| 2 | A README in each crate root, the five manifest fields, `[package] exclude` for the test tree, and #96's own text corrected to 0.3.0 | #96 |
| 3 | `chrono` replaced in `extract::set_capture_time`, and `deny.toml`'s dated advisory ignore deleted in the same pull request | #101 |
| 4 | The `specs/fixtures.md` row for two catalogs writing one destination, and the test that fails if the marker-prefix or dimension check weakens | #60 |
| 5 | `--verify-source`, `source_sha256` and `snapshot_sha256`, the `options` echo, exit code `5`, the report and CLI specs, the README rows, and ADR 0010's and 0011 D13's dated corrections | #63 |
| 6 | `LRSALVAGE_DISCARD_*`, the `real-discard` arm, `tests/discard.rs`, the rebuilt copy and its `EVIDENCE.md`, and every restatement of the family | #86 |
| 7 | A dated amendment in ADR 0003: resume identity keeps its shape | #66 |
| 8 | The trusted-publisher workflow job, `0.3.0` in the workspace and in both `lrsalvage-core` pins (`lrsalvage-cli`, `lrsalvage-fixtures`), and `CHANGELOG.md`'s `[0.3.0]` section | — |

## Before the tag

### Slices

- [x] **1 — this file** is merged, the `0.2.0` milestone is closed, `0.3.0` exists
      and holds #96, #101, #60, #63, #86 and #66. Nothing else enters either
      milestone: #62, #64, #65, #67, #68 and #93 are 0.4's or the app's, and #93
      stays open after this tag with the README sentence it already has.
      *Verified 2026-11-17:* the milestone holds those six plus this release's own
      paperwork (#104 closed, #106 open) and slice 6's fixture ticket (#70, closed),
      and nothing else entered it — #88 left for `0.4.0` first.
- **2 — #96. Deleted 2026-11-17, with the reason: the command this box names does
      not pass for one crate, and making it pass is a test-architecture decision
      this release does not take.** The crate pages, the manifest fields and
      `[package] exclude` are in, and
      `cargo package --list -p lrsalvage-core` and `-p lrsalvage-cli` show both
      tarballs carrying `README.md` and no test file that cannot build. The second
      half of the box is false for one crate, measured in this session by unpacking
      `lrsalvage-core-0.3.0.crate` outside the repository: `cargo build` succeeds,
      while `cargo build --all-targets` and `cargo test` fail with `cannot find
      module or crate lrsalvage_fixtures`. The `#[cfg(test)]` modules in
      `src/exif.rs`, `src/extract.rs` and `src/plan.rs` import the test-only
      fixtures crate, and no `exclude` reaches code inside a source file
      (`crates/lrsalvage-core/Cargo.toml` says so; ADR 0011 D6 refuses publishing
      the fixtures crate). The cli's tarball is green both ways, and `scripts/check.sh
      package` builds core's tarball without testing it, so the gate matches the
      truth rather than the box. The published core crate is a library whose suite
      is this repository's gate, not its own — and #96's matching *Done when*
      sentence was corrected rather than left standing. What would close the gap is
      the fixture-dependent `#[cfg(test)]` modules moved out of `src/`, which is the
      alternative core's manifest names to publishing that crate. Carrying that as a
      ticket rather than as this paragraph is **After the tag**'s own obligation.
- [x] **3 — #101.** `chrono` is gone from `Cargo.lock`, `set_capture_time` resolves
      a civil datetime through the replacement's time zone with the same
      disambiguation policy, and the `[advisories] ignore` entry for
      RUSTSEC-2026-0543 leaves in the same pull request. An ambiguous or
      non-existent local time still drops the mtime rather than guessing, and a
      test exercises that branch directly rather than only the happy path. The
      mtime and the drop rule are unchanged, so the changelog says nothing about
      this slice.
- [x] **4 — #60.** `specs/fixtures.md` gains the row for a second vault whose image
      shares an output name and dimensions with an existing output, with its
      assertion kind, evidence label and **`cannot show`** text recording that this
      is *our* limitation and not Adobe's behaviour. The test fails if the
      marker-prefix or the dimension check is weakened or removed. The README
      sentence landed in 0.2 and is not rewritten; the deferred decision is linked
      from the row rather than implied.
- [x] **5 — #63.** `--verify-source` on `scan` and `extract`; the verification
      report carries `source_sha256` and `snapshot_sha256` as hex, `null` when the
      flag was not given, so the object is eleven keys whether or not a run asked;
      `options` gains `verify_source`; a digest that moved exits `5` with a stated
      reason and is never a warning. The snapshot is what is compared, not the
      catalog as it stands later, and neither digest is derived from the cache.
      `report.csv` and `manifest.json` are untouched. `r1` moves from nine frozen
      keys to eleven with the reason recorded in the test, the README's exit-code,
      flag and verification-field rows are updated in this pull request, the report
      and CLI specs record the fields and the code, and ADR 0010 gains its dated
      *added by 0.3.0* block while ADR 0011 D13 gains its dated correction. A
      synthetic vault run with the flag reports equal digests; a deliberately
      modified snapshot fails loudly with its own message. Two repairs landed with
      it rather than in a slice of their own, because it is the first change to
      the seam between the crates: `scripts/check.sh package` now verifies the CLI
      tarball against the core tarball the same run built (a `--config patch`),
      since the version its manifest names is not on crates.io before this release
      and the gate was therefore verifying the previous one; and `ADR 0007` gains
      a dated amendment for the digest pair its path-freedom argument did not
      anticipate.
- [x] **6 — #86.** `LRSALVAGE_DISCARD_CATALOG` / `LRSALVAGE_DISCARD_PREVIEWS` name
      a discarded cache, and `sh scripts/check.sh real-discard` fails rather than
      skipping when they are unset, exactly as the other three families do.
      `crates/lrsalvage-cli/tests/discard.rs` asserts `scan --json` (`layout modern`,
      `images 10`, `recovered 10`, tiers `large 9` / `standard 1`,
      `thumbnail_pyramids 0`, `unmapped 0`, `missing_payload 0`, `below_floor 0`,
      `superseded 0`, **`deleted 0`** — the capture that said `1` was wrong),
      `scan` exit 0, and an `extract --min-long-edge 0 --include-present` writing
      ten with `failed 0`, `quality bigThumbnail` on the demoted pyramid and the
      2 731 955-byte payload total. The copy behind it was rebuilt by hand on
      2026-11-17 — the render's pair copied whole, `Library > Previews > Discard
      Standard and 1:1 Previews` applied to all ten, the result on an evidence
      root that is not a scratch tree with an `EVIDENCE.md` recording the render
      it was cut from, the dialog's wording and the hashes that prove the 29
      survivors are the render's bytes. The gate is green on that copy, and the
      payload total is the number the `0.1.0` build also reported, which is what
      makes it a fact about Adobe's payloads. The restatements ride in this pull
      request: `.env.example`, `scripts/check.sh` (usage line, `require_pair`
      comment, `case` arm), `AGENTS.md`'s gate list and its evidence-family
      paragraph, `docs/agents/rendering-a-test-cache.md`'s "names all three pairs"
      sentence and its new discard steps, `format-notes` §5.11, and this page —
      plus the copies a grep found outside that list, and one wrong sentence in
      `CODING_STANDARDS.md`.
- [x] **7 — #66.** ADR 0003 gains a dated amendment: resume identity stays marker
      prefix + completeness + `SOF` dimensions, one destination per catalog is the
      documented rule, and strengthening it waits for a report of the failure it
      prevents. It records that the alternatives were weighed — the digest in the
      marker, the `extract.csv` sidecar, asking the cache — and why each was
      refused, rather than only that they were.
- [x] **8 — publishing.** The workflow job is in, with `id-token: write` and
      no long-lived token; the workspace `version` is `0.3.0`, both
      `lrsalvage-core = { path, version = ... }` pins (the cli's and the fixtures
      crate's, whose requirement must match or the workspace will not resolve) move
      with it, and
      `CHANGELOG.md` gains `## [0.3.0]` with **Added** (`--verify-source`, the two
      digest fields, the `options` member), **Changed** (exit code `5`, the
      `Software` marker's version — its prefix is unchanged, so a 0.2.0 output is
      still ours) and a short **Packaging** note (the crate page and the tarballs).
      Nothing here is listed for #101, #60 or #86: none of them changes what a user
      or a script sees.
      *Proven 2026-11-17:* the job is in `release.yml` with `id-token: write`, no
      secret is named in it, `gh secret list` is empty, and the workspace, both
      `lrsalvage-core` pins and `Cargo.lock` all read `0.3.0`. The crate-side half
      was re-opened when the tag's first attempt was refused (`No Trusted
      Publishing config found`) and is proven now: a trusted publisher exists for
      each name, which both uploads below demonstrate, and the job skips a crate
      the index already carries, so the re-run published only what was missing.

### Evidence — what the release may claim

- [x] All four families are green here, on the commit the tag will land on:
      `sh scripts/check.sh real`, `real-render`, `real-1to1` and `real-discard`.
      Each fails rather than skipping when its own pair is unset, and none is
      borrowed from another. *Run on `a009091`:* four green runs, one suite each —
      `all gates green: real`, `real-render`, `real-1to1`, `real-discard`.
- [x] §5.11's numbers are re-measured against the rebuilt copy. The capture they
      rested on came from an `0.1.0` build whose report echoes `min_long_edge 640`,
      so neither the byte total nor the report's own members were inherited: the
      rebuilt copy gives the same `stats.bytes`, 2 731 955, and the row asserts it.
- [x] The rebuild's provenance is recorded where the copy is: the render it was cut
      from, the discard's own wording in the confirmation dialog, and the hashes
      that show the surviving files are the render's bytes.
- [x] `format-notes` §5.11 still says `VERIFIED`, and says in one sentence that the
      copy it names was rebuilt after the first was deleted, with the date.
      Nothing in it is downgraded to prose: the gate holds the counts and the
      payload total, and the section says which gate.
- [x] Where a byte-level claim has no gate behind it, the release says so in the
      notes rather than in a checklist nobody reads twice. The published section
      carries no byte-level claim at all — the numbers it could restate stay in
      `format-notes` — so there is nothing for it to caveat; the one route this
      release cannot prove, macOS, is stated in `README.md` in the tagged tree and
      tracked by [#93](https://github.com/marcd35/lrsalvage/issues/93), which is
      named when the tracker closes rather than left to be inferred.

### Artifact — what a stranger can download

- [x] `cargo package --list -p lrsalvage-core` and `-p lrsalvage-cli` list no test
      file that cannot build, and each tarball carries its crate's `README.md`.
      *Run 2026-11-17:* core lists `README.md`, `src/…` and `tests/properties.rs`, and
      no fixture-importing test file; the cli lists `README.md`, `src/…` and no test
      tree at all. *Run 2026-11-17:* it packaged
      `lrsalvage-core-0.3.0`, verified it and stopped at `warning: aborting upload
      due to dry run`; the cli's generated manifest carries
      `[dependencies.lrsalvage-core] version = "0.3.0"` with no `path`, so the pin
      resolves against the index in the artifact as well as the tree, and
      `scripts/check.sh package` verifies the cli's tarball against the core tarball
      beside it with a `--config patch`, so the gate proves the pair this run built
      rather than whatever copy the index happens to carry — the reason the script
      itself gives, and the one that outlives this release.
- [x] The tag still produces the three platform archives and `SHA256SUMS` from
      `release.yml`, with the archive names `cargo-binstall`'s template requires —
      unchanged by the publish job.
      *Verified 2026-11-17 on `v0.3.0`:* the release carries
      `lrsalvage-x86_64-unknown-linux-gnu-v0.3.0.tgz`,
      `lrsalvage-x86_64-pc-windows-msvc-v0.3.0.zip`,
      `lrsalvage-aarch64-apple-darwin-v0.3.0.tgz` and `SHA256SUMS`, and downloading
      all four and running `sha256sum --check --strict SHA256SUMS` reports three
      `OK`. The publish job changed none of them.
- The box that asked whether the publish job is the only new writer on crates.io
      **was measured false on 2026-11-17 and is true from 2026-11-18.** `gh secret
      list` is empty, so no repository secret holds a token and the job is the only
      writer this release adds; both crates list exactly one trusted publisher, which
      is what both uploads below demonstrate. When the box's question was first put
      to the tree the answer was different — the job's own attempt answered `No
      Trusted Publishing config found for repository marcd35/lrsalvage` — and that
      refusal, the human step that cleared it and the re-run that published are
      recorded where the publish box stands under **After the tag**.

### Gates — the ones a script can run

- [x] `sh scripts/check.sh` green: `fmt`, `shell`, `clippy --all-targets`, the
      workspace tests, `doc`, the release build, `typos` and `deny`. A missing tool
      fails rather than skipping. *Run on `a009091` with the tag not yet existing:*
      `all gates green: fmt shell clippy test doc build package typos deny msrv`.
- [x] `sh scripts/check.sh msrv` green on the `rust-version` the manifests publish,
      with the new dependency's floor checked against it — a dependency that raises
      the floor is a decision, not a pin. Same run: `rust-version = 1.89`, toolchain
      `1.89.0` installed and the workspace built on it.
- [x] `spec_validate` clean: no dangling link, duplicate id or parent cycle across
      `specs/`, including the row and the module specs this release edits. →
      `Spec-graph is valid: no issues found.`
- [x] `cargo test -p lrsalvage-core --test spec_graph` and
      `cargo test -p lrsalvage-cli --test readme` green after the documentation
      edits: a module's `depends-on` matches what it reaches, and the README's flag
      and `--json` tables match the binary. → `6 passed` and `3 passed` on
      `a009091`.
- [x] `typos` and `cargo deny` are green with their own policy files unchanged
      except where a slice says otherwise: `deny.toml`'s advisory ignore is
      *deleted* by slice 3, never renewed. → both green in the run above, and
      `chrono` is absent from `Cargo.lock`.

### Frozen surface — what 0.2 promised, still promised

- [x] ADR 0010's eight surfaces reviewed against the whole diff: nothing removed,
      renamed or given a different meaning, and the additions are exactly the flag,
      the `options` member, the two digests and exit code `5`, each named in 0010's
      dated block. *Reviewed 2026-11-17 across the release's whole diff,
      `v0.2.0..a009091` (16 commits, 45 files):* the only added flag is
      `--verify-source` at both of its `#[arg]` sites, the only added exit code is
      `const EXIT_EVIDENCE: u8 = 5`, nothing is removed or renamed, and
      `report.csv`, `manifest.json`, the status words, the output names and `0`–`4`
      are untouched by any test — the workspace suites and `spec_graph` are green on
      `a009091`.
- [x] A later version keeps recognising what 0.2 wrote: the resume gate's
      destination marked `lrsalvage 0.1.0` is still skipped byte for byte, and a
      0.2.0 output is skipped for the same reason — the version inside the marker
      is never compared. 0.3.0's own marker moves only that number. Held by
      `a_preexisting_verified_file_is_skipped_and_its_bytes_untouched`, green in
      the `extract` suite on `a009091`.
- [x] Every sentence naming 0.2 as the present release is re-read and rewritten:
      `README.md`'s version paragraph, the macOS gap's pointer, *"the frozen-surface
      review in the 0.2 checklist"*, and *"the commit a tag is cut on is chosen by
      the 0.2 checklist"* — plus the same claim in `release.yml`'s own comment.
      Historical files keep their 0.2 references: this is a sweep of what a reader
      takes for the present, not a rename.
- [x] The macOS paragraph still states its gap in its own words and points at
      [#93](https://github.com/marcd35/lrsalvage/issues/93), which stays open after
      this release; no box here claims the macOS route was proved.

### Docs — the record

- [x] `CONTEXT.md` gains **Source digest** and **Snapshot digest**, each saying how
      it differs from the payload digest Adobe writes into `previews.db`, and
      **Verification report** grows the digests and the second half of its
      path-free argument: a SHA-256 names no path. *Evidence family* stays out —
      that is how this repository tests, not what the product is about.
- [x] ADR 0003's amendment (slice 7), ADR 0010's *added by 0.3.0* block and ADR
      0011 D13's dated correction (slice 5) are all in the tree, each dated.
- [x] `specs/modules/report.md` records the two fields and the `options` member as
      part of the frozen set, and `specs/modules/cli.md` records exit code `5` and
      the flag — the two specs a reader consults instead of the code.
- [x] Each slice's own ticket is corrected in the pull request that implements it,
      or, where a slice merged uncorrected, in the docs close-out with a dated
      **Corrected** sentence: #96's patch-release sentence, #86's `deleted 1`,
      #63's "two fields" without the `options` echo.
      *Recorded 2026-11-17:* the three slices merged without the correction their own
      box asked for, so each body was corrected here instead, with a dated
      **Corrected** sentence naming the reason and the evidence, and the release's
      docs pull request says so.

## Making the tag

The tag and everything under it are a maintainer's acts, each on its own go-ahead:
the agent prepares the command, runs every gate it can run first, and verifies the
outcome afterwards. Nothing under **After the tag** happens without its own
separate go-ahead — one approval for "the release" is not one for an irreversible
upload.

- [x] The tree the tag lands on is measured, not inherited: `sh scripts/check.sh`
      green on that commit after it is merged and before a tag exists, all four
      evidence families green on it, `cargo publish --dry-run -p lrsalvage-core`
      packaging and reaching `aborting upload due to dry run`, and the release body
      read from the extractor `release.yml` itself runs rather than by eye.
      *Measured 2026-11-17 on `a009091`, the squash commit of #120, before the tag
      existed:* every gate green, the four families green, the dry run reaching
      `warning: aborting upload due to dry run`, and the extractor's `awk` giving 39
      non-blank lines that open `## [0.3.0]` and end at the section boundary.
- [x] `CHANGELOG.md`'s `## [0.3.0]` section is the release body, verbatim. The
      published body's first and last lines are that section's, and the heading
      after it did not travel into the release. *Checked against the published
      release:* `diff` against the extractor's output is a single trailing blank
      line GitHub trims, the first line is `## [0.3.0]`, the last non-blank line is
      the Packaging bullet about trusted publishing, and `## [0.2.0]` is absent.
- [x] The tag is `v0.3.0`, annotated, on the merge commit of the close-out pull
      request — the tree published from, which carries slice 8's bump, changelog and
      workflow by construction rather than by a later check. → `v0.3.0` is an
      annotated tag on `a009091`, the squash commit of #120.
- [x] The release publishes the three archives, `SHA256SUMS` and the changelog
      section — and nothing generated from `scan --report`, which describes a cache,
      not a release. →
      [the release](https://github.com/marcd35/lrsalvage/releases/tag/v0.3.0) has
      exactly those four assets and no other.

## After the tag (irreversible)

**The publish is the frozen-tree part of this list**, and 0.3.0 is the first
release whose publishes run through OIDC rather than a token. That had never been
rehearsed by anyone, because crates.io cannot attach a trusted publisher to a name
that does not exist and cannot rehearse a publish it has not received — so both
stop rules were decided here, before the attempt. The attempt then failed at stop
rule one, and the record below is what happened next.

Publish — the artifact's second route, and the one no dry run can prove:

- [x] `lrsalvage-core` then `lrsalvage-cli` 0.3.0, from a clean checkout of `v0.3.0`
      rather than from a working tree, published by the trusted-publisher job in
      `release.yml`. The order is not a preference: the cli resolves
      `lrsalvage-core 0.3.0` through the index.
      **Done 2026-11-18 (UTC), after two refusals.** The first attempt failed at
      stop rule one — the `Authenticate against crates.io` step answered `Failed to
      retrieve token from Cargo registry. Status: 400. Error: No Trusted Publishing
      config found for repository marcd35/lrsalvage`, so nothing was uploaded,
      nothing yanked, and no token was substituted. The trusted publisher was then
      added by hand for each crate (owner `marcd35`, repository `lrsalvage`,
      workflow `release.yml`, no environment) — a re-run before it existed was
      refused with the same message, so that is two refusals for one act, and the
      second is the one the record must not hide: adding the entry is what cleared
      it, not re-running. The third attempt published `lrsalvage-core 0.3.0` and
      then `lrsalvage-cli 0.3.0` through OIDC: `Uploaded lrsalvage-cli v0.3.0 to
      registry crates-io` … `Published lrsalvage-cli v0.3.0 at registry
      crates-io`. Stop rule one's "scoped, short-lived token" was never taken up:
      this run holds no crates.io token, and `gh secret list` is still empty.
      The published body's last Packaging bullet — `0.3.0` is "the first release
      whose publishes run through crates.io trusted publishing" — was false while
      the publish was blocked and is true as of that re-run. The body was not
      edited; it did not need to be.
      **Stop rule one.** If the OIDC job fails on `lrsalvage-core`, the fix is a
      scoped, short-lived token for that one release so the version lands, and the
      workflow defect becomes its own issue — never a yank of a version that was
      never uploaded.
      **Stop rule two.** If core lands and the cli publish fails, the fix is
      forward: correct the tree and publish `lrsalvage-cli 0.3.0`, because an index
      carrying core and no cli is recoverable. If core itself is defective —
      building from the repository but not from the tarball — `0.3.0` cannot be
      re-uploaded: yank it, fix it and release `0.3.1` with its own annotated tag
      and its own release body. Either way, every sentence naming a version follows
      the published one.
- [x] `cargo install lrsalvage-cli --version 0.3.0` works with no checkout in
      scope: the build comes from the registry, `lrsalvage --version` prints
      `lrsalvage 0.3.0`, and `~/.cargo/.crates.toml` records the registry source.
      *Run 2026-11-17 from an empty scratch directory on a machine that does hold a
      checkout, with `--locked --root`, so nothing was installed into the user's
      `~/.cargo/bin`:* cargo compiled `lrsalvage-core 0.3.0` and `lrsalvage-cli
      0.3.0` from the registry, the binary answered `lrsalvage 0.3.0`, and the
      root's `.crates.toml` records
      `lrsalvage-cli 0.3.0 (registry+https://github.com/rust-lang/crates.io-index)`
      — the line `~/.cargo/.crates.toml` would hold for the same install. What this
      proves is the registry source; what it does not prove is the absence of a
      checkout anywhere on the machine.
- [x] `cargo binstall lrsalvage-cli` tried once against the tag, with its stop
      rule: if it does not fetch the prebuilt, the README's binstall paragraph is
      corrected to say so rather than left as an expectation.
      *Run 2026-11-17:* it resolved `lrsalvage-cli v0.3.0 (x86_64-pc-windows-msvc)`
      from github.com and installed the prebuilt archive in 3.4 s — no fallback to
      `cargo install` — and the installed binary answered `lrsalvage 0.3.0`. The
      `--git` form was not re-run and the README still says what was proved of it,
      against `v0.2.0`.

Docs and the record, once the publish has said what is true:

- [x] The README's install and crates.io sentences are corrected in a pull request
      merged after the publish: both names are on crates.io as 0.3.0, the route was
      proved, and no sentence describes a route that does not exist in either
      direction. *Done:* the version paragraph, the prebuilt-archive table and its
      three command blocks, the macOS paragraph's added line, `cargo install
      --version 0.3.0` and the binstall paragraph all name 0.3.0 and only what was
      proved of it — the bare binstall form against `0.3.0`, the `--git` form
      against `v0.2.0`, the macOS routes not at all. The registry route is stated as
      proved because `cargo install` built `lrsalvage 0.3.0` from the index, and the
      checksum block is stated as checksums because that is what was run.
- [x] `rust-version` and `rust-toolchain.toml` are re-checked against the
      dependency tree, so the next release does not inherit a stale claim.
      *Re-checked 2026-11-17:* `rust-version = 1.89` is the number
      `sh scripts/check.sh msrv` built the workspace on with every current
      dependency (`jiff` and the rest) resolved, and `rust-toolchain.toml` pins
      `1.100.0`, which `ci.yml` and `release.yml` install as their only written
      number. Neither moved this release.
- [x] #96, #86, #101, #60, #63, #66, the `0.3.0` milestone and
      [#106](https://github.com/marcd35/lrsalvage/issues/106) are closed with the
      release link, and anything a slice cut on the way is an issue with its reason
      rather than a paragraph here. #93 stays open, and #62, #64, #67 and the rest
      of 0.4's candidates stay open as such — the release says so rather than
      leaving their absence to be inferred. *Done 2026-11-17, with one exception:*
      all eight closed, each with the [v0.3.0
      release](https://github.com/marcd35/lrsalvage/releases/tag/v0.3.0) in the
      closing comment, and the tracker's own comment names #93, #62, #64, #65, #67,
      #68 and #117 as staying open. The exception is the first clause's second half:
      the one thing this release cut — the published `lrsalvage-core` tarball
      cannot run its own `cargo test` (box 2) — needs its own issue, and filing one
      was not authorised in this run, so it stands as box 2's measurement until it
      is filed. That is stated in the tracker's closing comment too.

## The discard, rebuilding it

§5.11's state is what `Library > Previews > Discard Standard and 1:1 Previews`
leaves, and it is reproducible on a machine with Lightroom Classic and a rendered
cache: take the render's catalog and previews as a pair, apply the discard to all
ten images, confirm the dialog that names the photo count, quit Lightroom so the
index settles, and measure before and after by declared segment lengths. The GUI
steps, the five facts that make driving them reliable and the discard's own steps
are in [rendering a test cache](agents/rendering-a-test-cache.md); §5.11 records
the confirmation dialog's wording, which is what a driver keys on.

Two things the first copy did not survive, and this rebuild does: the copy lives
outside any scratch directory, and the hashes that tie it to the render it came
from sit beside it. A copy that no document can identify is a copy a tidy-up
deletes.
