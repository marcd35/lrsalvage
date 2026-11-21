# Build and deploy the browser app

The [app release checklist](../release-app-0.1.md) owns the release go-ahead and
evidence. The [browser app workflow](../../.github/workflows/app-release.yml)
builds the site once and deploys the checked ZIP from that run. The CLI has its
own release workflow and tag.

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

## Configure Pages

Set up a [Cloudflare Pages Direct Upload project](https://developers.cloudflare.com/pages/get-started/direct-upload/)
with `main` as its production branch. Create GitHub deployment environments
`app-preview` and `app-production`. In each, set the variable
`CLOUDFLARE_PAGES_PROJECT` to the project name and set the secrets
`CLOUDFLARE_ACCOUNT_ID` and `CLOUDFLARE_API_TOKEN`. The token needs
[Cloudflare Pages Edit access scoped to the account](https://developers.cloudflare.com/pages/how-to/use-direct-upload-with-continuous-integration/).
Restrict the production environment to tag `app-0.1.0` and apply the repository's
release approval process before making the tag.

Dispatch the **browser app** workflow using the candidate branch or tag. A
dispatch uploads to a preview branch named for the commit. A push of
`app-0.1.0` takes the tagged production path. The deployment job checks the
configured project's production branch before uploading.

The job summary records the verified HTTPS deployment URL and build commit.
Use that exact URL for release evidence. The automated HTTP check compares both
cross-origin isolation headers, the build identity, the WebAssembly response,
and the practice downloads against the staged site. In Windows Chrome and Edge,
also check `window.isSecureContext && window.crossOriginIsolated` on the landing
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
