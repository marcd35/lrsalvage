#!/bin/sh
# The #185 release-scale progress run: one command, from a clean checkout.
#
#   sh scripts/scan-progress-fixture.sh [--chrome <path>] [--work <dir>]
#
# It builds the page's WASM at this checkout, composes the 5,000-image Writer
# Scale vault when the work directory does not already hold one, and drives a
# whole scan and a stopped scan through the page's own entry. The JSON line it
# prints is the evidence; what to record is `docs/agents/browser-app-deployment.md`,
# *Verify matching progress at release scale*. `--work` reuses a vault across runs,
# which matters because composing one takes minutes and the vault never changes.
#
# This is **not a gate**. The gate is `sh scripts/browser-app.sh --only progress` —
# and so `sh scripts/check.sh browser` — which drives the readout with made-up
# reports in seconds; this one takes a scan.
#
# Needs: `cargo`, `node`, `wasm-bindgen` (the version the lock names), the
# `wasm32-unknown-unknown` target, a C compiler for it — the pair `.env.example`
# names — and Chromium or Chrome. The compiler is the one prerequisite that
# otherwise fails as a wall of C-builder output, so it is checked here by name.
set -eu

cd "$(dirname "$0")/.."

require() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'missing %s: %s\n' "$1" "$2" >&2
        exit 1
    fi
}

require cargo 'rustup.rs'
require node 'nodejs.org'
require wasm-bindgen 'cargo install wasm-bindgen-cli --version 0.2.130'
require rustup 'rustup.rs'

if ! rustup target list --installed | grep -qx wasm32-unknown-unknown; then
    printf '%s\n' \
        'wasm32-unknown-unknown is not installed: rustup target add wasm32-unknown-unknown' >&2
    exit 1
fi

. ./scripts/load-env.sh

if [ -z "${CC_wasm32_unknown_unknown:-}" ]; then
    printf '%s\n%s\n%s\n' \
        'CC_wasm32_unknown_unknown is unset, so the wasm32 build has no C compiler.' \
        'Copy .env.example to a gitignored .env and fill the pair in, or put it in' \
        "${XDG_CONFIG_HOME:-$HOME/.config}/lrsalvage/real.env. Read: ${load_env_files:-nothing}" >&2
    exit 1
fi

chrome=''
work='target/scan-progress-fixture'
while [ "$#" -gt 0 ]; do
    case "$1" in
        --chrome)
            shift
            chrome=${1:-}
            [ -n "$chrome" ] || {
                printf '%s\n' '--chrome needs a path' >&2
                exit 2
            }
            ;;
        --work)
            shift
            work=${1:-}
            [ -n "$work" ] || {
                printf '%s\n' '--work needs a directory' >&2
                exit 2
            }
            ;;
        *)
            printf 'unknown argument: %s\n' "$1" >&2
            exit 2
            ;;
    esac
    shift
done

printf '%s\n' '== wasm32 release'
cargo build --release --locked --lib --target wasm32-unknown-unknown -p lrsalvage-app
rm -rf crates/lrsalvage-app/web/pkg
mkdir -p crates/lrsalvage-app/web/pkg
wasm-bindgen \
    --target web \
    --out-dir crates/lrsalvage-app/web/pkg \
    --out-name lrsalvage_app \
    target/wasm32-unknown-unknown/release/lrsalvage_app.wasm

if [ -f "$work/fixture.json" ]; then
    printf '%s\n' "reusing the vault in $work"
else
    printf '%s\n' '== 5,000-image vault'
    cargo run --release --locked -p lrsalvage-app --bin vault-writer-scale -- "$work"
fi

printf '%s\n' '== product scan progress'
# The run leaves the trace its own replay reads: `scripts/scan-progress-trace.mjs`
# reports the estimate's upward revisions from these events without a browser.
trace=${SCAN_PROGRESS_TRACE:-target/scan-progress-trace.json}
SCAN_PROGRESS_TRACE=$trace
export SCAN_PROGRESS_TRACE
if [ -n "$chrome" ]; then
    node scripts/drive-browser.mjs "$work" --chrome "$chrome" --check-scan-progress-fixture
else
    node scripts/drive-browser.mjs "$work" --check-scan-progress-fixture
fi
printf '%s\n' "replay: node scripts/scan-progress-trace.mjs $trace"
