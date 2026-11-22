# Releasing app 0.1

The [release tracker](https://github.com/marcd35/lrsalvage/issues/166)
owns the slice order. [ADR 0012](adr/0012-two-release-lines.md) owns the
release decisions. Use this checklist to record the evidence and execute
the release. For each completed box, record the date, tested commit and
the result or a link to it.

The [browser deployment runbook](agents/browser-app-deployment.md) covers the
Pages project, deployment workflow and versioned static archive.

The app release adds a versioned static page that takes a Lightroom
Classic catalog folder and preview cache through scan, review, JPEG export
and receipt. It preserves the [recovery contract](adr/0010-what-0-2-promises.md)
and the independent core release line.

Planning baseline on 2026-11-20 was `eb3af3e`. The browser filesystem
backend and entry and scan had landed. The release evidence below is
pending.

## Before the tag

- [ ] Close the tracker’s implementation slices against
      [the app acceptance spec](https://github.com/marcd35/lrsalvage/issues/117)
      and its dated corrections. Record the accepted candidate commit.
- [ ] Run the applicable gates in [scripts/check.sh](../scripts/check.sh)
      on that commit. Record the output parity gate required by ADR 0012 D12
      for the actual browser writer and CLI.
- [ ] The release maintainer builds the static candidate from trusted `main`
      and deploys it to Cloudflare Pages over HTTPS. Record the build commit
      and preview URL on [#172](https://github.com/marcd35/lrsalvage/issues/172).
      Verify cross-origin isolation, the app and core versions on the page and
      receipt, the four practice cases, and the guide and screenshots.
- [ ] Record the [synthetic writer result](https://github.com/marcd35/lrsalvage/issues/171).
      Follow the [release-scale writer protocol](agents/browser-app-deployment.md#verify-the-writer-at-release-scale)
      and link the dated result for the tested candidate.
- [ ] Record the [deployed browser and accessibility results](https://github.com/marcd35/lrsalvage/issues/172).
      Exercise the actual folder pickers in Windows Chrome and Edge.
      Link the accessibility statement with observed and unchecked criteria.
      Limit the support claim to the combinations that passed.
- [ ] Record the [maintainer recovery result](https://github.com/marcd35/lrsalvage/issues/173)
      on the same candidate. The recovery ends in usable JPEGs and a receipt
      reconciled with disk after a batch stop and continuation, listing the
      reports it wrote and offering the verification report. Share path-free
      diagnostics manually and check that the copied text names no path.
      Reverify a failed recovery after its fix.

## Tag

- [ ] Obtain the release go-ahead for the verified commit. Create and push
      `app-0.1.0` for that commit. Confirm the app deployment workflow used
      that tag and the CLI release workflow did not publish a core release.

## After deployment

- [ ] Record the production URL, deployed commit and the result of checking
      the isolation headers, versions, and a complete practice recovery
      on the production page.
- [ ] Publish the release links and tested support claims. Record the
      tagged static artifact and how to redeploy it. Mark the release
      tracker and milestone complete once the checked evidence is linked.
