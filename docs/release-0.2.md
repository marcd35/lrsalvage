# Releasing 0.2

The 0.2 checklist, as one list a release can be executed against line by line. It
is the output of the map at [#49](https://github.com/marcd35/lrsalvage/issues/49)
and its decisions live in [ADR 0010](adr/0010-what-0-2-promises.md); this file
holds only what to run and what to check off.

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
- [ ] Every §5.5 `unmeasured` row still names the fixture row that pins the
      reader's behaviour in it; no new byte-level claim landed without a label.

Artifact — what a stranger runs:

- [x] `0.2.0` in the workspace, and `--version`, the `Software` marker and both
      schema strings agree — asserted by a test, not eyeballed.
- [ ] `CHANGELOG.md`'s 0.2.0 entry accounts for every user-visible change since
      `v0.1.0`, each naming its ADR.
- [x] Three platform archives and `SHA256SUMS` attached to the release; archive
      names match `cargo-binstall`'s templates. Proven end to end at
      `v0.2.0-rc.1`: four assets, `sha256sum -c` green on the downloads, and the
      names resolving through binstalk's own default templates.
- [ ] `lrsalvage-core` then `lrsalvage-cli` on crates.io — no `version` on the
      `lrsalvage-fixtures` dev-dependency, or `cargo publish` refuses.
- [ ] `cargo install lrsalvage-cli --version 0.2.0` works from a machine with no
      checkout. The package is `lrsalvage-cli` and the binary it installs is
      `lrsalvage`, so the typed command keeps its `-cli`; the package name is
      settled and not renamed.
- [ ] The README's install commands ran verbatim against the published
      pre-release, on every platform 0.2 ships. Done 2026-11-17 against
      `v0.2.0-rc.1` for two of the three: the Linux `grep … | sha256sum -c` and
      `tar -xzf` passed with only that archive present (`…tgz: OK`, then the
      binary, `LICENSE-MIT` and `LICENSE-APACHE` at the root of the tree and
      nothing else), and the Windows `Select-String` / `Get-FileHash` /
      `Expand-Archive` / `--version` passed with the same hash and
      `lrsalvage 0.2.0` out of the unpacked `.exe`. Still to do: macOS, both
      `shasum -a 256 -c` and `xattr -d com.apple.quarantine`.
- [ ] `cargo binstall --git https://github.com/marcd35/lrsalvage lrsalvage-cli`
      tried once `v0.2.0` is tagged, with its stop rule: if it does not fetch the
      prebuilt, the README's binstall paragraph is corrected to say so instead of
      being left as an expectation. Attempted 2026-11-17 against
      `v0.2.0-rc.1` — it cloned the tree, resolved `lrsalvage-cli v0.2.0`, found
      no `v0.2.0` asset to download and fell back to `cargo install`, which fails
      while the crate is unpublished. That is the tag gap, not the route, so the
      box stays open until a final tag exists to prove it either way.
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

- [ ] `format-notes` retirements landed; no dead `TODO` left behind.
- [ ] `CONTEXT.md` matches the vocabulary the release actually uses.
- [x] README's "What this repository is not" rewritten: it now *has* a release
      workflow, and a GUI is planned rather than ruled out.

## Making the tag

- [ ] `CHANGELOG.md`'s 0.2.0 section is the release body, verbatim.
- [ ] The tag is `v0.2.0`, annotated, on the commit that merged the last slice.
- [ ] The release publishes the archives, `SHA256SUMS` and the changelog section —
      and nothing generated from `scan --report`, which describes a cache, not a
      release.
- [ ] After the tag: `rust-version` and `rust-toolchain.toml` are re-checked
      against the dependency tree, so the next release does not inherit a stale
      claim the way 0.1 did.
