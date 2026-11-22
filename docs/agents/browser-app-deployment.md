# Build and deploy the browser app

The [app release checklist](../release-app-0.1.md) owns the release go-ahead and
evidence. The [browser app workflow](../../.github/workflows/app-release.yml)
builds and checks one static artifact per run. Pull requests build the artifact
and run the verifier and Chromium product check without deployment credentials.
Preview and production deployments use the checked ZIP from their own run.
The CLI has its own release workflow and tag.

## Build locally

Install the pinned Rust toolchain, add the `wasm32-unknown-unknown` target, set
up the wasm C compiler described in [the env example](../../.env.example),
install the `wasm-bindgen-cli` version in [Cargo.lock](../../Cargo.lock), and use
Python 3.11 or later. From the intended checkout, run:

```sh
python scripts/build-browser-app.py --out target/app-static
```

Choose a new output directory for each build. The build records the app version,
embedded core version, commit, and checkout cleanliness in `build.json`. Use
`--tag app-0.1.0` from a clean checkout of that tag for the tagged artifact.

The builder runs `vault-practice` with the shared fixture crate and checks the
published folders through the native app scan. The ZIP and in-page practice
flow use the same generated files. The source bytes and photo credits are in
[the practice assets](../../crates/lrsalvage-app/practice/credits.txt).

To open a local candidate, serve the output with
`npx wrangler@4.166.0 pages dev target/app-static`. Visit the address it prints.
The Pages emulator resolves `/recovery` and applies the site's `_headers` file.

## Verify app pull requests

Review the credential-free **browser app** build and its artifact checks.
Run the product and CLI parity checks locally with `sh scripts/check.sh browser`. On Windows,
use Git's shell. The script lists its tool prerequisites and runs the same
shared synthetic cases as CI: the page's entry against the native one, the
review flow, the export flow held to the CLI on identical bytes, and the receipt
with its reports and clipboard diagnostics.

For an on-disk synthetic catalog, generate the cases into a fresh directory:

```sh
cargo run --locked -p lrsalvage-app --bin vault-app -- target/app-fixtures
```

Use the ten-photo `review` case. Find its `catalog` and `cache` paths in
`target/app-fixtures/native.json`. Both paths are relative to
`target/app-fixtures/review`. In the app, select the catalog's containing folder
and the separate cache. The review case includes a deliberately undecodable
preview. Keep generated catalogs in scratch or `target`.

## Writing a browser check

A check is one module, `scripts/check-<name>.mjs`, exporting `async (send) => numbers`:
`send` is the DevTools call, and the object it returns is printed as the phase's
result. `scripts/drive-browser.mjs` gives it a `--check-<name>` flag and
`scripts/browser-app.sh` runs it as a phase; that script's header names the
phases it already has.

Iterate against a fixture directory rather than a rebuild:

```sh
sh scripts/browser-app.sh --work <dir> --only receipt
```

`--work` reuses a directory — the native run, the vaults and the CLI's own files
are made only when missing — and `--only` runs one phase. Both leave the
directory in place on success, which is what turns a failing assertion into a
rerun instead of a twelve-minute rebuild. Where a check can, hold the page to the
CLI over one vault: `check-export.mjs` and `check-receipt.mjs` both compare the
files the page wrote with the CLI's own runs on the same bytes.

Three facts about headless Chromium cost a debugging session each the first
time, and the platform does not say any of them out loud:

- `Browser.grantPermissions` with the legacy `clipboardReadWrite` /
  `clipboardSanitizedWrite` names is rejected as `Invalid parameters` on Chrome
  157. Ask with `Browser.setPermission` and the descriptor form instead:
  `{ name: 'clipboard-read', allowWithoutSanitization: true }`, and the same for
  `'clipboard-write'`.
- `navigator.clipboard.readText()` refuses an unfocused document with
  `NotAllowedError: Document is not focused`, and a headless tab is never focused.
  `Emulation.setFocusEmulationEnabled` plus `Page.bringToFront` is what makes a
  copy readable.
- `FileSystemDirectoryHandle.removeEntry` throws `NoModificationAllowedError`
  while a writable stream is open anywhere below it, so a test cannot delete a
  folder out from under in-flight writes. Fail the write instead: a
  `createWritable` that throws `QuotaExceededError` is the same observation from
  the other side.

No check can answer a native picker — the accepted folder is supplied as the value
a pick returns, which is the measurement `#149`'s *Testing Decisions* records.

## Verify the writer at release scale

Run the [5,000-file writer check](https://github.com/marcd35/lrsalvage/issues/171)
from the candidate's clean checkout in PowerShell with Node.js 22 or later:

```powershell
$candidate = 'https://your-preview.pages.dev/recovery'
node scripts/check-writer-scale.mjs $candidate target/writer-scale 'internal SSD, NTFS'
```

Pass the hosted HTTPS `/recovery` URL and an unused work directory on the local
destination drive. Describe the actual storage in the last argument. Set
`CHROME` if Chrome is installed outside its default locations. The command
builds the CLI reference and opens a foreground Chrome session. Follow the
terminal prompts to pick the catalog, separate preview cache and destination in
each tab. The command drives Save, Stop and Continue after those picks. Keep the
Chrome tab visible during each batch.

Generate one reproducible synthetic vault through the shared `Vault` builder.
Select all 5,000 images in the default mirrored layout. Give each image a
decodable, distinct JPEG payload and distribute the images across nested folders.
Use 4,000 previews around 28 KB and 1,000 around 417 KB, the two sizes referenced
by the [browser spike](../browser-spike.md#3-writing-payloads). Record the actual
size distribution and total payload bytes. Extract the same vault through the
CLI from the candidate commit with matching options and capture timestamps
disabled. Use the CLI output as the reference for both runs.

1. **Uninterrupted run.** Pick a fresh destination and start saving. Observe a
   rendered saved count above zero and below the batch total while the batch is
   active. Record elapsed time from Save to the final successful JPEG close and
   the longest gap between consecutive successful JPEG closes. Compare elapsed
   time with the 120-second performance target. Let all writes proceed without
   deliberate holds.
2. **Stop and continuation run.** Pick another fresh destination. After the
   page shows a positive saved count, hold all eight active JPEG streams just
   before their real closes, then click **Stop** and release the holds. Verify
   that only those paths commit after Stop and that the stopped receipt matches
   the settled files on disk. Select **Continue saving** in the same tab and
   verify that writers open only for the remaining paths. Track real writer opens
   by full relative JPEG path across the run so a byte-identical rewrite fails
   the check. Record performance from the uninterrupted run because the copied
   diagnostics can include the pause before continuation in the longest gap.

For both runs, enumerate the actual destination and compare the complete set of
relative JPEG paths and their SHA-256 hashes to the CLI. Require 5,000 saved
images, zero write failures, and zero wrong, missing, or extra JPEGs. Reconcile
the final receipt against the destination. Record the date, tested commit,
hosted URL, app and core versions, Windows and Chrome versions, destination
storage, fixture size, counts, correctness verdict, and timings in
`writer-scale-result.json` in the work directory. Link that result from the app
release checklist.

## Configure Pages

Set up a [Cloudflare Pages Direct Upload project](https://developers.cloudflare.com/pages/get-started/direct-upload/)
with `main` as its production branch. Create GitHub deployment environments
`app-preview` and `app-production`. In each, set the variable
`CLOUDFLARE_PAGES_PROJECT` to the project name and set the secrets
`CLOUDFLARE_ACCOUNT_ID` and `CLOUDFLARE_API_TOKEN`. The token needs
[Cloudflare Pages Edit access scoped to the account](https://developers.cloudflare.com/pages/how-to/use-direct-upload-with-continuous-integration/).
Use selected deployment branches and tags in GitHub. Allow only the protected
`main` branch to access `app-preview` and only the `app-0.1.0` tag to access
`app-production`. Keep the token in each deployment environment, since Pages
Edit applies across the account. Apply the repository's release approval
process before making the tag.

The release maintainer owns setup and the first preview dispatch. After the
candidate merges, dispatch the **browser app** workflow from trusted `main`.
It uploads to a preview branch named for the commit.
Record the hosted walkthrough and its URL on
[#172](https://github.com/marcd35/lrsalvage/issues/172), before the app release tag. A push of `app-0.1.0` takes the tagged production
path. The deployment job checks the configured project's production branch
before uploading.

The job summary records the verified HTTPS deployment URL and build commit.
Use that exact URL for release evidence. The automated HTTP check compares both
cross-origin isolation headers and compares deployed assets with the staged
site. It checks the JavaScript and WebAssembly content types. In Windows Chrome
and Edge, also check `window.isSecureContext && window.crossOriginIsolated` on the landing
page and the recovery page. Open each practice sample from the guide and compare
the result with the described outcome. Use a separate folder for the working
sample's JPEG export, then open the JPEGs and reconcile the receipt with disk.
Check the guide's instructions and any screenshots against that build. Record
the browser versions, commit, URL and actual results in the release evidence.

## Retain and redeploy the archive

Save the tagged workflow's **browser-app-static** artifact. Keep its versioned
ZIP and `SHA256SUMS` with the app release assets so they remain available
after the workflow artifact expires. The ZIP contains the full Pages output.
To restore a release, verify its checksum and extract it into a clean directory.
For a preview redeploy, use Wrangler's
[Direct Upload branch option](https://developers.cloudflare.com/pages/get-started/direct-upload/#deploy-your-assets)
with a branch other than the project's production branch. For production use the
release's approved workflow or the approved rollback to its original
deployment in Pages. Preserve `_headers` on every deployment. For another
static host, configure the same headers and map `/recovery` to `recovery.html`.
