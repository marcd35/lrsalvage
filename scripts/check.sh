#!/bin/sh
# The one list of gates.
#
# `AGENTS.md` and `README.md` point here instead of repeating these commands, and
# CI runs the Rust gates through this file, so a gate cannot be documented weaker
# than it is enforced — which is exactly how `cargo clippy -- -D warnings` drifted
# from CI's `--all-targets` and stopped linting test code.
#
#   scripts/check.sh              every gate, in order
#   scripts/check.sh clippy fmt   only those, in the order given
#   scripts/check.sh real         the #[ignore]d tests, against real catalogs
#
# A missing tool is a failure, never a skip: a green run must mean CI's green.
#
# Two gates are the one exception to "CI calls this file": `typos` and
# `cargo deny` run in CI through their vendor actions (`crate-ci/typos`,
# `EmbarkStudios/cargo-deny-action`), so this file's `typos` and `deny` are the
# local equivalent of the same two commands, not a second policy.
set -eu

cd "$(dirname "$0")/.."

# A gitignored `.env` may name LRSALVAGE_TEST_CATALOG and LRSALVAGE_TEST_PREVIEWS
# so the real-cache tests can be run on the machine that holds them. It is never
# committed: real catalog paths are not a repo artefact.
load_env() {
    if [ -f .env ]; then
        # `set -a` exports what the file assigns: a plain `NAME=value` line is
        # only a shell variable, and the tests are a child process.
        set -a
        # shellcheck disable=SC1091
        . ./.env
        set +a
    fi
}

gate_fmt() {
    printf '%s\n' '== fmt'
    cargo fmt --all --check
}

gate_clippy() {
    printf '%s\n' '== clippy'
    cargo clippy --workspace --all-targets -- -D warnings
}

gate_test() {
    printf '%s\n' '== test'
    cargo test --workspace
}

# A doc comment is prose that compiles, so no other gate sees it: this catches a
# link to an item that is private, renamed or gone, and malformed markup. It is
# the only thing that inspects `///` outside a reader's eye.
gate_doc() {
    printf '%s\n' '== doc'
    RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --workspace
}

gate_build() {
    printf '%s\n' '== build --release'
    cargo build --release --locked
}

gate_typos() {
    printf '%s\n' '== typos'
    require typos 'cargo install typos-cli'
    typos
}

gate_deny() {
    printf '%s\n' '== deny'
    require cargo-deny 'cargo install cargo-deny --locked'
    # The inclusion graphs are a tree per duplicate crate — thousands of lines
    # about `syn` that no gate run needs; the verdict line survives.
    cargo deny check --hide-inclusion-graph
}

require() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'missing %s: %s\n' "$1" "$2" >&2
        exit 1
    fi
}

# The real-cache rows: `#[ignore]`d and reached only from this machine's own
# catalogs. Evidence for `README.md`'s compatibility claims comes from here.
gate_real() {
    printf '%s\n' '== real catalogs'
    load_env
    if [ -z "${LRSALVAGE_TEST_CATALOG:-}" ] || [ -z "${LRSALVAGE_TEST_PREVIEWS:-}" ]; then
        printf 'set LRSALVAGE_TEST_CATALOG and LRSALVAGE_TEST_PREVIEWS (see .env).\n' >&2
        exit 1
    fi
    cargo test --workspace -- --ignored
}

all_gates='fmt clippy test doc build typos deny'

if [ "$#" -eq 0 ]; then
    set -- $all_gates
fi

for gate in "$@"; do
    case "$gate" in
        fmt | clippy | test | doc | build | typos | deny | real) "gate_$gate" ;;
        *)
            printf 'unknown gate: %s (one of: %s real)\n' "$gate" "$all_gates" >&2
            exit 1
            ;;
    esac
done
