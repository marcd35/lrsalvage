#!/bin/sh
# The app's browser run: one entry, two targets, and the flow a person takes.
#
#   sh scripts/browser-app.sh [--chrome <path>] [--attach <port>] [--verbose]
#
# The browser app workflow runs this check on one Chromium runner for relevant
# pull requests. See docs/agents/browser-app-deployment.md for setup and evidence.
#
# What it does:
#
#   1. `vault-app` composes the synthetic vaults and runs `lrsalvage_app::scan`
#      over each on the host, writing `native.json`.
#   2. The same crate, compiled to `wasm32` and wrapped by `wasm-bindgen`, is
#      loaded by the app's page. `web/test.js` drives the page's own entry over the
#      same vault bytes, mounted into origin-private storage.
#   3. `scripts/drive-browser.mjs` serves both, drives headless Chromium over the
#      DevTools Protocol and reports every difference.
#   4. The review case drives index.html after supplying the accepted directory
#      handle at the picker boundary, including browser-native Escape and focus.
#
# What the run proves, and what it cannot: the File System Access *dialog* cannot
# be answered by any automation we have (measured on Chrome 157.0.8103.79,
# 2026-11-19 — see `#149`'s *Testing Decisions*), so the picker's accepted answer
# is supplied as the value a pick returns, and its *cancelled* branch is driven for
# real, because headless answers `AbortError` with no dialog.
#
# Needs: `cargo`, `node`, `wasm-bindgen` (the version the lock names), the
# `wasm32-unknown-unknown` target, a C compiler for it — the same two variables
# `scripts/check.sh wasm` wants — and Chromium or Chrome. Each is required, not
# skipped.
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

# The one gitignored file a machine's own paths may live in, and the file
# `scripts/check.sh` reads for the same reason: the wasm C compiler is not on this
# machine's PATH. `scripts/check.sh` owns the rule; this is the same three
# candidates, so a machine that satisfied the `wasm` gate satisfies this too.
for candidate in \
    ./.env \
    "$(git config --get lrsalvage.realEnvFile 2>/dev/null || true)" \
    "${XDG_CONFIG_HOME:-$HOME/.config}/lrsalvage/real.env"; do
    [ -n "$candidate" ] || continue
    [ -f "$candidate" ] || continue
    set -a
    # shellcheck disable=SC1090,SC1091
    . "$candidate"
    set +a
done

chrome=''
driver_args=''
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
        --attach)
            shift
            driver_args="$driver_args --attach ${1:-}"
            [ -n "${1:-}" ] || {
                printf '%s\n' '--attach needs a debugging port' >&2
                exit 2
            }
            ;;
        --verbose)
            driver_args="$driver_args --verbose"
            ;;
        *)
            printf 'unknown argument: %s\n' "$1" >&2
            exit 2
            ;;
    esac
    shift
done

work=$(mktemp -d)
cleanup() {
    status=$?
    if [ "$status" -ne 0 ]; then
        printf 'kept %s\n' "$work" >&2
    else
        rm -rf "$work"
    fi
    exit "$status"
}
trap cleanup EXIT

printf '%s\n' '== native'
cargo run -q -p lrsalvage-app --bin vault-app -- "$work"

printf '%s\n' '== wasm32'
cargo build -q --lib --locked --target wasm32-unknown-unknown -p lrsalvage-app
rm -rf crates/lrsalvage-app/web/pkg
mkdir -p crates/lrsalvage-app/web/pkg
wasm-bindgen \
    --target web \
    --out-dir crates/lrsalvage-app/web/pkg \
    --out-name lrsalvage_app \
    target/wasm32-unknown-unknown/debug/lrsalvage_app.wasm

printf '%s\n' '== headless Chromium'
# The harness page covers scan parity and picker cancellation. No leading slash:
# `sh` rewrites an argument that starts with one into a Windows path.
page='crates/lrsalvage-app/web/test.html'
if [ -n "$chrome" ]; then
    # shellcheck disable=SC2086
    node scripts/drive-browser.mjs "$work" --chrome "$chrome" --page "$page" $driver_args
else
    # shellcheck disable=SC2086
    node scripts/drive-browser.mjs "$work" --page "$page" $driver_args
fi

printf '%s\n' '== product preview review'
if [ -n "$chrome" ]; then
    # shellcheck disable=SC2086
    node scripts/drive-browser.mjs "$work" --chrome "$chrome" --check-review $driver_args
else
    # shellcheck disable=SC2086
    node scripts/drive-browser.mjs "$work" --check-review $driver_args
fi
