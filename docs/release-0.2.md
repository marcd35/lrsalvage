# Releasing 0.2

The 0.2 checklist, as one list a release can be executed against line by line. It
is the output of the map at [#49](https://github.com/marcd35/lrsalvage/issues/49)
and its decisions live in [ADR 0010](adr/0010-what-0-2-promises.md); this file
holds only what to run and what to check off.

Every box below is either provable or gone with its reason written where the box
stood. A grilling on [#89](https://github.com/marcd35/lrsalvage/issues/89) settled
which is which — what the release may claim, what it cannot, and which acts are
irreversible — and each box it cut, moved or re-scoped carries that reasoning
alongside it.

**0.2 is a release, not a feature.** It is `v0.1.0` plus the whole v2 reader —
which for a user of 0.1.0 means the modern layout, five `layout` words, `--json`,
`--report`, the cache stats and the companion rescue — and it changes **nothing**
on top of today's `HEAD`: no status, exit code, flag, field, column or output name.
The one difference in the bytes a 0.1.0 run wrote is the `Software` marker 0.2
writes into its own EXIF segment, which 0.1.0 did not write at all. Resume
compares that marker's `lrsalvage ` prefix rather than the version inside it,
which is what keeps a later version from re-writing what 0.2 wrote; a 0.1.0
output has no marker to compare, so a 0.2 run reports it as not ours and leaves it
byte-identical.

## The slices this list executes

| Gate below | Slices |
|---|---|
| Evidence | #50 (can a purge reach a whole pyramid), #55 (`real-1to1` family) |
| Artifact | #52 (release workflow), #53 (changelog), #54 (crates.io) |
| Gates | #56 (MSRV proven in CI) |
| Frozen surface | ADR 0010 itself — no slice, a review |
| Docs | #51 (contract in the README), #57 (the sealed seam), #58 (format-notes retirements), #59 (report intake), #60 (resume limitation), #61 (README rewrite) |

## What was left, and in what order

The five slices and the one merge this release still needed, in the order it
needed them. Each slice is one pull request, and all five had to be in the tree
the tag ships — the last two because the release body and the published tarballs
come out of that tree, not out of a tree that moved after it.

| Slice | What it changes | Closes |
|---|---|---|
| 1 | This file: every box provable or cut, and the README saying what the macOS lines are worth | — |
| 2 | `CONTEXT.md`'s missing `layout` words, ADR 0005's dead `(§5.7 TODO)`, the remaining `format-notes` `TODO`s re-read, and the §5.5 row audit | the two Docs boxes and the Evidence audit |
| 3 | The resume-identity sentence: marker, JPEG and dimensions are not photo identity | `#60`'s Docs box |
| 4 | `cargo package`: no `version` on either fixtures dev-dependency, licence texts in both crate roots, dry runs recorded | the packaging box |
| 5 | `CHANGELOG.md` walked against `v0.1.0..HEAD`, one line per user-visible change and its ADR | the changelog box |
| — | [#70](https://github.com/marcd35/lrsalvage/issues/70)'s discarded-cache fixture row (PR 87) | the state a discard leaves, pinned |

The order is dependency, not preference: slice 1 is where the other four get their
acceptance criteria, slices 4 and 5 have to exist before the tag because the
publish and the release body are taken from that tree, and everything the tag
cannot change is moved below it rather than left in a list a tag would freeze.

## Before the tag

Evidence — what the release may claim:

- [x] #50's trigger state attempted once, with its stop rule: `format-notes` §5.5
      `M10f` reads **unmeasured** with the dated attempt, §5.11 records the
      deletion that can be triggered — which takes the companion too — and the
      README's rescue sentence matches.
- [x] `sh scripts/check.sh real-1to1` green here, and failing rather than skipping
      when its pair is unset — the third evidence family beside `real` and
      `real-render`. Green 2026-11-17, and with the pair unset it exits 1 naming
      both names rather than skipping.
- [x] Every §5.5 `unmeasured` row still names the fixture row that pins the
      reader's behaviour in it; no new byte-level claim landed without a label.
      Audited 2026-11-17: the table's seven rows name nine fixture rows, and each
      of those exists exactly once — `M3`, `M4`, `X1`, `X2` and `M10f` labelled
      `unmeasured §5.5`; `M10c` `verified §5.10`; `M13` "verified §5.10 for the
      segments; synthetic for the payload's own orientation"; and `M10d`, `M10e`
      `synthetic`. Five of the nine read `unmeasured`, and the paragraph above the
      table said all of them did — it now states the split.
      **§5.11 is the one published claim whose label rests on no gate.** It reads
      `VERIFIED` for a dated one-off measurement on a copy of the `Auto (3840px)`
      render, and no test holds it: the standing §5.4 had when its claim went
      false. Its numbers live in §5.11 and nowhere else — the README cites that
      section for the mechanism, that a discard takes the companion too, and
      never for a count. A fourth evidence family (`LRSALVAGE_DISCARD_*`, a
      `real-discard` gate) needs a fresh copy of that cache and a Lightroom
      session; that is a slice, and it is
      [#86](https://github.com/marcd35/lrsalvage/issues/86) in `vNext`, not
      something this release claims to have done.
- [ ] [#70](https://github.com/marcd35/lrsalvage/issues/70)'s discarded-cache
      fixture row is merged — PR 87, which adds row `M10h` — so the state a
      Lightroom discard leaves is pinned by the reader rather than by prose.
      Independent of the five slices; it only has to land before the tag.

Artifact — what a stranger runs:

- [x] `0.2.0` in the workspace, and `--version`, the `Software` marker and both
      schema strings agree — asserted by a test, not eyeballed.
- [x] `CHANGELOG.md`'s 0.2.0 entry accounts for every user-visible change since
      `v0.1.0`, each naming its ADR. Walked 2026-11-17 over `v0.1.0..046729b` —
      the whole range from the tag to the branch point, fourteen of whose commits
      touched `crates/*/src` — and each maps to a line: the modern reader and the
      index keying it fixed
      (ADRs 0004, 0005), the degraded mode (0006), `--report` (0007), a payload's
      own metadata and the `Software` marker (0003), the second pyramid (0008),
      the rescue (0009), plus the eight new `--json` fields and the two new
      `report.csv` columns that started from a comparison against `v0.1.0`'s
      README and `report.rs` rather than from memory. Two entries name a gate
      instead of an ADR because no ADR governs them: the MSRV move to 1.89
      (`scripts/check.sh msrv`) and the verification report form. Everything else
      in the range is a gate, a spec, evidence or our own release docs, which
      ship no user-visible surface.
      Both defects the walk found are fixed rather than listed: the opening
      sentence promised "the first release a user can install rather than build"
      while the crates are still unpublished at the tag, and "a tag on today's
      tree" would have outlived the day it was written.
- [x] Three platform archives and `SHA256SUMS` attached to the release; archive
      names match `cargo-binstall`'s templates. Proven end to end at
      `v0.2.0-rc.1`: four assets, `sha256sum -c` green on the downloads, and the
      names resolving through binstalk's own default templates.
- [x] Both crates package, and the tarballs are what a publication would carry —
      slice 4. No `version` on either `lrsalvage-fixtures` dev-dependency, because
      Cargo strips a dev-dependency only when it carries none and `cargo publish`
      otherwise refuses a package crates.io does not have. `LICENSE-MIT` and
      `LICENSE-APACHE` in each crate root: both manifests declare
      `MIT OR Apache-2.0`, the release workflow copies the two texts into every
      archive for exactly that reason, and cargo cannot reach a file outside the
      package root.
      Measured 2026-11-17: `cargo publish --dry-run -p lrsalvage-core` packaged,
      compiled from `target/package/` and reached `aborting upload due to dry
      run`; `cargo package --list` on both crates now lists `LICENSE-MIT` and
      `LICENSE-APACHE`; the dry run printed **no** warning about the missing
      texts before them, which is why the omission survived two releases.
      **For `lrsalvage-cli` that file list is all the proof the tag can hold.**
      `cargo package`, `cargo package --no-verify` and `cargo publish --dry-run`
      all fail on it in the same way — `no matching package named
      lrsalvage-core` — because the manifest's `lrsalvage-core 0.2.0` resolves
      through the index. The cli's tarball is therefore built for the first time
      after core is live, which is why **After the tag** runs a cli dry run
      between the two publishes and why a cli failure there is fix-forward.
- [x] The README's install commands ran verbatim against a published pre-release,
      for the two platforms verified here. Done 2026-11-17 against `v0.2.0-rc.1`
      for Linux and Windows: the Linux `grep … | sha256sum -c` and `tar -xzf`
      passed with only that archive present (`…tgz: OK`, then the binary,
      `LICENSE-MIT` and `LICENSE-APACHE` at the root of the tree and nothing
      else), and the Windows `Select-String` / `Get-FileHash` / `Expand-Archive` /
      `--version` passed with the same hash and `lrsalvage 0.2.0` out of the
      unpacked `.exe`.
      **The macOS half is cut, not deferred.** `shasum -a 256 -c` and
      `xattr -d com.apple.quarantine` have never run on a Mac, no Mac is available
      to this release, and a CI runner is not the proof: it can check the hash,
      but quarantine attributes are set by the tool that downloads a file, so
      `xattr -d` there is a no-op that establishes nothing. The README says so in
      its own words, and a `vNext` ticket tracks a real verification on a Mac. The
      alternative — leaving the box open until a Mac appears — would have held the
      tag behind hardware instead of behind a claim, which is the wrong trade for
      a route whose commands are the same shape as the two that passed.
- [x] README names every install route, the macOS Gatekeeper workaround, and the
      two things 0.2 does not ship (installer, notarisation) as decisions rather
      than omissions.

Gates — what is enforced rather than written:

- [x] `sh scripts/check.sh` green.
- [x] `sh scripts/check.sh real`, `real-render` and `real-1to1` green.
- [x] `spec_validate` clean.
- [x] `spec_graph.rs` and `readme.rs` green after the documentation edits.
- [x] The MSRV job green on the `rust-version` the manifests publish — the number
      comes from a run, never from a table. (#56)

Frozen surface — what ADR 0010 promises:

- [x] ADR 0010's eight surfaces reviewed against the whole diff: nothing removed,
      renamed, or given a new meaning.
      Reviewed 2026-11-17 across `v0.1.0..main`. The status words, the exit codes,
      the `lrsalvage.manifest` envelope and `report.csv`'s column order are
      unchanged; `scan --json`'s fields, the `stats` keys, `report.csv`'s two new
      columns and the flags are additive; output naming is byte-identical. The
      exception is recorded rather than hidden: `unmapped` and `missing-payload`
      narrowed *within* 0.2 (ADR 0008, ADR 0009) and the changelog's `### Changed`
      states both — the statement ADR 0010 asks for, in the first release that
      could carry one, since `v0.1.0` shipped no changelog to announce it in.
- [x] A later version keeps recognising what 0.2 wrote: resume compares the
      marker's `lrsalvage ` prefix and not the version inside it, so a 0.2.x run
      keeps what 0.2.0 wrote. Held by `extract`'s comparison
      (`specs/modules/extract.md`, ADR 0010 ¶8) and pinned by
      `extract::tests::a_preexisting_verified_file_is_skipped_and_its_bytes_untouched`,
      which marks a destination `lrsalvage 0.1.0` while the binary under test is
      `0.2.0` and expects it skipped. The claim this line used to make — that a
      **0.1.0 output** verifies as ours too — was never satisfiable: 0.1.0 wrote
      no marker at all, which is why a 0.2 run reports a 0.1.0 destination as not
      ours and leaves it byte-identical.
- [x] Anything that failed this review is out of 0.2 and becomes its own slice.
      Nothing failed it.

Docs — what a reader trusts:

- [x] `format-notes` retirements landed; no dead `TODO` left behind.
      Audited 2026-11-17: twelve live `TODO`s, beside the legend that defines the
      label and one prose reference to it. The one #89 called dead was not: ADR
      0005's `(§5.7 TODO)` pointed at a `TODO` bullet living inside a section
      whose heading reads **VERIFIED**. That is a real defect and the opposite
      one, so the claim moved into §5.5's unmeasured list and the ADR cites it
      there. The tombstone `TODO` was narrowed by §5.11, which measured one
      deletion that takes the rows and their payloads together. The rest are live
      and name what they wait for: the `AgHg` `version`/`kind` values (one
      published hexdump), padding contents, a `formatVersion` other than `3`, the
      three unexplained count gaps in §5.4, the `.lrcat-data` shard split, which
      orientation column wins when the two disagree, an orientation code outside
      the four measured ones, and the source table's thread URLs, which were not
      captured.
- [x] `CONTEXT.md` matches the vocabulary the release actually uses. Audited
      2026-11-17: the **Preview layout** entry named the five cases in prose and
      none of the words a script reads, so it now names all five — `legacy`,
      `modern`, `mixed`, `none`, `unknown` — in the sense
      `specs/modules/previews-db.md` gives them, and says why the word is
      published.
- [x] The README states the resume-identity limitation beside the resume
      paragraph: identity is the marker, the JPEG and the size rather than the
      photograph, so a different image that derives the same output name at the
      same dimensions is skipped as already recovered. The measurement behind the
      wording narrowed it further than #60 assumed: the skip happens **before**
      the force check in `extract_one`, so `--force` cannot replace a collided
      destination either — it only replaces a file that does not verify as ours.
      The flag table says so now, and the remedy is one destination per catalog
      or deleting the collided file. That is the whole of
      [#60](https://github.com/marcd35/lrsalvage/issues/60) that 0.2 takes:
      decision 2 put the sentence in this release, decision 3 forbids changing
      the identity here, and the fixture row and its test stay with #60 in
      `vNext`.
- [x] README's "What this repository is not" rewritten: it now *has* a release
      workflow, and a GUI is planned rather than ruled out.
- [x] The compatibility contract is published in the README: one table, ADR 0010's
      eight surfaces, each naming the section that states it and what holds it,
      with the two tiers, the marker's prefix clause and the review-held rows
      marked as such — `report.csv` among the gate-held ones. (#51)

## Making the tag

The tag and everything under it are a maintainer's acts, each on its own
go-ahead: the agent prepares the command, runs every gate it can run first, and
verifies the outcome afterwards. Nothing under **After the tag** happens without
its own separate go-ahead — one approval for "the release" is not one for an
irreversible upload.

- [ ] `CHANGELOG.md`'s 0.2.0 section is the release body, verbatim.
- [ ] The tag is `v0.2.0`, annotated, on the commit that merged the last of
      slices 1–5 or PR 87, whichever lands last. That tree is the one published
      from, so it carries slice 4's packaging and slice 5's changelog section by
      construction, not by a later check.
- [ ] The release publishes the archives, `SHA256SUMS` and the changelog section —
      and nothing generated from `scan --report`, which describes a cache, not a
      release. The tag is the only trigger (`.github/workflows/release.yml`), so
      this is the act that makes the release public rather than a draft.

## After the tag (irreversible)

**The publish is the frozen-tree part of this list.** Everything under *Publish*
is an external act that another commit cannot walk back, so each is run
deliberately, one at a time, on its own go-ahead, and the README and the changelog
follow them rather than leading. The docs group after it writes and merges commits
like any other — it is here because it can only be written once the publish has
said what is true, not because it is irreversible.

Publish — the artifact's second route, and the one no dry run can prove:

- [ ] `lrsalvage-core` then `lrsalvage-cli` on crates.io, from a clean checkout of
      `v0.2.0` rather than from a working tree: `cargo publish -p lrsalvage-core`,
      then `cargo publish --dry-run -p lrsalvage-cli` — the first moment that
      command can run, and the only build the cli tarball gets before it ships —
      then `cargo publish -p lrsalvage-cli`. The order is not a preference: the
      cli resolves `lrsalvage-core 0.2.0` through the index, so it cannot go first
      and nothing about it can be packaged beforehand.
      **Two stop rules, decided before the attempt rather than after it.**
      If core lands and the cli publish fails, the fix is forward: correct the
      tree and publish `lrsalvage-cli 0.2.0`, because an index carrying core and
      no cli is recoverable by publishing the cli.
      If core itself is defective — it builds from the repository but not from the
      tarball, or the licence texts are missing from that tarball — `0.2.0`
      cannot be re-uploaded: yank it, fix it, and release `0.2.1` with its own
      annotated tag and its own release body. Either way, **any sentence naming a
      version follows the published one**, in `README.md`, `CHANGELOG.md` and the
      release body.
- [ ] `cargo install lrsalvage-cli --version 0.2.0` works from a machine with no
      checkout. The package is `lrsalvage-cli` and the binary it installs is
      `lrsalvage`, so the typed command keeps its `-cli`; the package name is
      settled and not renamed.
- [ ] `cargo binstall --git https://github.com/marcd35/lrsalvage lrsalvage-cli`
      tried once `v0.2.0` is tagged, with its stop rule: if it does not fetch the
      prebuilt, the README's binstall paragraph is corrected to say so instead of
      being left as an expectation. Attempted 2026-11-17 against
      `v0.2.0-rc.1` — it cloned the tree, resolved `lrsalvage-cli v0.2.0`, found
      no `v0.2.0` asset to download and fell back to `cargo install`, which fails
      while the crate is unpublished. That is the tag gap, not the route, so the
      box stays open until a final tag exists to prove it either way. A draft
      release was rejected as a way to answer it early: a draft's assets are not
      fetchable, so the attempt would establish nothing.

Docs and the record, once the publish has said what is true:

- [ ] The README's two crates.io sentences are corrected in a pull request merged
      after the publish: "Neither name is on crates.io yet ([#54]), so this route
      is not open today", under `### cargo install`, and the binstall paragraph's
      "both of its forms wait on something 0.2 does not have yet", whose "starts
      working at the publish" is the other half of the same claim. The page must
      never describe a route that does not exist in either direction.
- [ ] `rust-version` and `rust-toolchain.toml` are re-checked against the
      dependency tree, so the next release does not inherit a stale claim the way
      0.1 did.
- [ ] [#54](https://github.com/marcd35/lrsalvage/issues/54),
      [#70](https://github.com/marcd35/lrsalvage/issues/70), the `0.2.0`
      milestone and [#89](https://github.com/marcd35/lrsalvage/issues/89) are
      closed with the release link, and the two things this release deliberately
      leaves undone are issues rather than paragraphs: the crates.io polish (a
      crate README, the manifest metadata, and the six test files that import
      `lrsalvage-fixtures`, which the published manifest strips so an auditor's
      `cargo test` fails), and the real-Mac verification of the two macOS install
      commands.
