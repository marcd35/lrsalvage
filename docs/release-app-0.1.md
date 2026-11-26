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
- [ ] Close [#191](https://github.com/marcd35/lrsalvage/issues/191) and
      [#192](https://github.com/marcd35/lrsalvage/issues/192) before tagging, and
      run the hosted walks below on a candidate that carries them: the first
      changes which folder the app suggests and how a destination is refused, the
      second changes the review and export surfaces the maintainer's recovery
      reads.
- [ ] Run the applicable gates in [scripts/check.sh](../scripts/check.sh)
      on that commit. Record the output parity gate required by ADR 0012 D12
      for the actual browser writer and CLI.
- [ ] Close [#186](https://github.com/marcd35/lrsalvage/issues/186) before tagging.
      On the accepted candidate, run the native and browser comparison and the
      existing catalog-backed CLI parity gate, then record the tested commit and
      results. [The app spec](../specs/app.md#recover-from-preview-cache) is the
      one home for what that comparison covers. The real-vault catalog-backed
      walkthrough below remains its own release check.
- [ ] The release maintainer builds the static candidate from trusted `main`
      and deploys it to Cloudflare Pages over HTTPS. Record the build commit
      and preview URL on [#172](https://github.com/marcd35/lrsalvage/issues/172).
      Verify cross-origin isolation, the app and core versions on the page and
      receipt, all practice cases — including the cache-only one, whose receipt is
      reconciled against the saved files — and the guide and screenshots.
- [ ] Record the [synthetic writer result](https://github.com/marcd35/lrsalvage/issues/171).
      Follow the [release-scale writer protocol](agents/browser-app-deployment.md#verify-the-writer-at-release-scale)
      and link the dated result for the tested candidate.
      Exceeding 120 seconds is a performance miss, not a release blocker, when
      correctness, disk reconciliation, and CLI parity pass. Record the miss and
      track a performance follow-up.
- [ ] Record the [deployed browser and accessibility results](https://github.com/marcd35/lrsalvage/issues/172).
      Exercise the actual folder pickers in Windows Chrome and Edge.
      Review the [app 0.1.0 accessibility statement](app-0.1-accessibility.md)
      for observed and unchecked criteria.
      Limit the support claim to the combinations that passed.
- [ ] Record the [matching progress evidence](https://github.com/marcd35/lrsalvage/issues/185)
      on the same candidate, following
      [the release-scale progress protocol](agents/browser-app-deployment.md#verify-matching-progress-at-release-scale).
      The local Chromium run and the accessible progress bar it reads are one
      result. The hosted 5,000-image scan and the hands-on screen-reader pass are
      two more: record each on its own, and never as implied by the local one.
- [ ] Record the [maintainer recovery result](https://github.com/marcd35/lrsalvage/issues/173)
      on the same deployed candidate, using the pre-alpha catalog and preview
      cache on the maintainer's test laptop. In a supported Windows browser,
      scan and review the full default eligible selection, then save it to a
      fresh destination with the default small-preview setting. Stop after some
      outputs are saved while other batch items remain pending. Let in-flight
      writes settle, reconcile the partial receipt with disk, and continue in the
      same tab.

      Find the recovery folder from the receipt and reconcile final counts and
      paths with disk. The fresh destination should yield one saved output per
      selected preview, zero already recovered, zero final failures, and no
      missing or duplicate outputs. Count scan exclusions separately. Choose
      samples that cover every available preview size, orientation, and source
      folder; a sample may cover more than one of those categories. Each sample
      must open, show the expected photo without visible corruption, and be
      useful at its recovered preview size. These are previews, not
      original-quality replacements. Confirm the receipt lists its reports and
      offers the verification report. Record transient failures and successful
      retries; a transient failure that is retried successfully is not a final
      failure.

      Record the candidate commit and URL, app and core versions, Windows and
      browser versions, scan and write counts, timings, and failures. Compare
      scan-to-first-visible-thumbnail time with the diagnostic first-thumbnail
      time, which starts at page load. Check that the gallery scrolls, opens
      previews, accepts selection changes, and proceeds to saving reliably;
      restore the default selection before saving. File follow-up tuning for
      slow but usable behavior. Timings are evidence, with no separate numeric
      threshold for this gate. The diagnostic longest write gap includes the
      deliberate stop and continuation pause.

      Use **Copy diagnostics**, check that the copied text contains no private
      paths, filenames, or catalog content, and share it in the issue comment
      with the aggregate result. Keep the receipt's CSV reports and manifest on
      the test machine. Fix and rerun a failing recovery before release. Limit
      public claims to the tested recovery and environment.

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
