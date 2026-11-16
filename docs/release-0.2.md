# Releasing 0.2

The 0.2 checklist, as one list a release can be executed against line by line. It
is the output of the map at [#49](https://github.com/marcd35/lrsalvage/issues/49)
and its decisions live in [ADR 0010](adr/0010-what-0-2-promises.md); this file
holds only what to run and what to check off.

**0.2 is a release, not a feature.** It is `v0.1.0` plus the whole v2 reader —
which for a user of 0.1.0 means the modern layout, five `layout` words, `--json`,
`--report`, the cache stats and the companion rescue — and it changes **nothing**
on top of today's `HEAD`: no status, exit code, flag, field, column or output name.
The one user-visible byte that differs from a 0.1.0 run is the version inside the
`Software` marker, and a 0.1.0 output still verifies as ours because resume
compares the prefix.

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
- [ ] `sh scripts/check.sh real-1to1` green here, and failing rather than skipping
      when its pair is unset — the third evidence family beside `real` and
      `real-render`.
- [ ] Every §5.5 `unmeasured` row still names the fixture row that pins the
      reader's behaviour in it; no new byte-level claim landed without a label.

Artifact — what a stranger runs:

- [ ] `0.2.0` in the workspace, and `--version`, the `Software` marker and both
      schema strings agree — asserted by a test, not eyeballed.
- [ ] `CHANGELOG.md`'s 0.2.0 entry accounts for every user-visible change since
      `v0.1.0`, each naming its ADR.
- [ ] Three platform archives and `SHA256SUMS` attached to the release; archive
      names match `cargo-binstall`'s templates.
- [ ] `lrsalvage-core` then `lrsalvage-cli` on crates.io — no `version` on the
      `lrsalvage-fixtures` dev-dependency, or `cargo publish` refuses.
- [ ] `cargo install lrsalvage --version 0.2.0` works from a machine with no
      checkout.
- [ ] README names every install route, the macOS Gatekeeper workaround, and the
      two things 0.2 does not ship (installer, notarisation) as decisions rather
      than omissions.

Gates — what is enforced rather than written:

- [ ] `sh scripts/check.sh` green.
- [ ] `sh scripts/check.sh real`, `real-render` and `real-1to1` green.
- [ ] `spec_validate` clean.
- [ ] `spec_graph.rs` and `readme.rs` green after the documentation edits.
- [x] The MSRV job green on the `rust-version` the manifests publish — the number
      comes from a run, never from a table. (#56)

Frozen surface — what ADR 0010 promises:

- [ ] ADR 0010's eight surfaces reviewed against the whole diff: nothing removed,
      renamed, or given a new meaning.
- [ ] A 0.1.0 output still verifies as ours (the marker prefix, not the version).
- [ ] Anything that failed this review is out of 0.2 and becomes its own slice.

Docs — what a reader trusts:

- [ ] `format-notes` retirements landed; no dead `TODO` left behind.
- [ ] `CONTEXT.md` matches the vocabulary the release actually uses.
- [ ] README's "What this repository is not" rewritten: it now *has* a release
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
