#!/bin/sh
# The browser half of the parity run: one synthetic vault, two targets, compared.
#
#   sh scripts/browser-parity.sh [--chrome <path>]
#
# **This is not a gate, and deliberately so.** `scripts/check.sh` is the gate list
# and a missing tool there is a failure rather than a skip, so a gate here would
# make every CI runner install a browser to prove a claim about one. It is instead
# the same shape as `scripts/probe-cache.sh`: a real measurement, run by a person
# or a release check, that says what it measured and in which tree
# (`specs/modules/fs.md`, *Where the harness lives*).
#
# What it does:
#
#   1. `vault-parity` builds the two vaults, runs the driver natively, and writes
#      `native.json` beside them.
#   2. The same driver, compiled to `wasm32` and wrapped by `wasm-bindgen`, is
#      loaded by a page that mounts the same vault bytes into origin-private
#      storage and runs the same modules.
#   3. `scripts/drive-browser.mjs` serves both, drives headless Chromium over the
#      DevTools Protocol, and compares counts, statuses and JPEG digests with
#      `MAIN_SEPARATOR` normalised (`docs/adr/0012-two-release-lines.md` D12).
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
# A relative name needs a slash for `sh`: a POSIX `.` searches `PATH` for a
# operand like `.env` and fails on a file that is right there.
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
        *)
            printf 'unknown argument: %s\n' "$1" >&2
            exit 2
            ;;
    esac
    shift
done

work=$(mktemp -d)
keep=''
cleanup() {
    status=$?
    if [ "$status" -ne 0 ] || [ -n "$keep" ]; then
        printf 'kept %s\n' "$work" >&2
    else
        rm -rf "$work"
    fi
    exit "$status"
}
trap cleanup EXIT

printf '%s\n' '== native'
cargo run -q -p lrsalvage-web --bin vault-parity -- "$work"

printf '%s\n' '== wasm32'
cargo build -q --lib --locked --target wasm32-unknown-unknown -p lrsalvage-web
rm -rf crates/lrsalvage-web/web/pkg
mkdir -p crates/lrsalvage-web/web/pkg
wasm-bindgen \
    --target web \
    --out-dir crates/lrsalvage-web/web/pkg \
    --out-name lrsalvage_web \
    target/wasm32-unknown-unknown/debug/lrsalvage_web.wasm

printf '%s\n' '== headless Chromium'
if [ -n "$chrome" ]; then
    node scripts/drive-browser.mjs "$work" --chrome "$chrome"
else
    node scripts/drive-browser.mjs "$work"
fi
