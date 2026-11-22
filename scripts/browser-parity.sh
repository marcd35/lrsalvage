#!/bin/sh
# The browser half of the parity run: one synthetic vault, two targets, compared.
#
#   sh scripts/browser-parity.sh [--chrome <path>]
#
# **This is not a gate.** `specs/modules/fs.md` (*Where the harness lives*) records
# why, and this script's prerequisites with it; `scripts/check.sh` is the gate list
# and this is the `scripts/probe-cache.sh` shape — a real measurement a person or a
# release check runs.
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

# The machine's own values, which is where the wasm C compiler lives on a machine
# that keeps it off `PATH`. The candidates and their order are
# `scripts/load-env.sh`'s, so a machine that satisfied the `wasm` gate satisfies
# this one too — including which file wins when two name the same variable.
. ./scripts/load-env.sh

chrome=''
# Everything the node driver understands, forwarded as given: `--attach <port>`
# reuses a Chrome that is already running, `--verbose` logs every operation.
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
    # shellcheck disable=SC2086
    node scripts/drive-browser.mjs "$work" --chrome "$chrome" $driver_args
else
    # shellcheck disable=SC2086
    node scripts/drive-browser.mjs "$work" $driver_args
fi
