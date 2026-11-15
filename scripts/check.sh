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
#   scripts/check.sh real-render  the #[ignore]d tests, against a rendered cache
#
# `real` and `real-render` are two evidence families, not one: a machine can hold
# a converted cache without a rendered one, and each gate fails rather than
# skipping when its own pair is unset.
#
# A missing tool is a failure, never a skip: a green run must mean CI's green.
#
# Two gates are the one exception to "CI calls this file": `typos` and
# `cargo deny` run in CI through their vendor actions (`crate-ci/typos`,
# `EmbarkStudios/cargo-deny-action`), so this file's `typos` and `deny` are the
# local equivalent of the same two commands, not a second policy.
set -eu

cd "$(dirname "$0")/.."

# A gitignored `.env` may name LRSALVAGE_TEST_CATALOG/LRSALVAGE_TEST_PREVIEWS
# (a converted cache) and LRSALVAGE_RENDER_CATALOG/LRSALVAGE_RENDER_PREVIEWS (a
# rendered one) so the real-cache tests can be run on the machine that holds
# them. It is never committed: real catalog paths are not a repo artefact.
#
# A worktree's own `.env` wins, and the machine's one file is the fallback — the
# only gate that verifies a byte-level claim must not be unreachable from a fresh
# worktree, which is what per-worktree configuration made it.
#
#   git config --global lrsalvage.realEnvFile ~/real-lrsalvage.env
#   ~/.config/lrsalvage/real.env          (the default, when the setting is unset)
load_env() {
    for candidate in \
        .env \
        "$(git config --get lrsalvage.realEnvFile 2>/dev/null || true)" \
        "${XDG_CONFIG_HOME:-$HOME/.config}/lrsalvage/real.env"; do
        [ -n "$candidate" ] || continue
        [ -f "$candidate" ] || continue
        # `set -a` exports what the file assigns: a plain `NAME=value` line is
        # only a shell variable, and the tests are a child process.
        set -a
        # shellcheck disable=SC1090,SC1091
        . "$candidate"
        set +a
        return 0
    done
}

gate_fmt() {
    printf '%s\n' '== fmt'
    cargo fmt --all --check
}

# The one language this repository writes outside Rust is shell, and nothing else
# reads it: `cargo` never parses `scripts/*.sh`, and a broken quote or heredoc in
# a gate script fails the day someone runs it. `sh -n` parses without executing,
# needs no dependency, and runs on all three CI systems.
gate_shell() {
    printf '%s\n' '== shell'
    for script in scripts/*.sh .githooks/pre-commit; do
        [ -f "$script" ] || continue
        sh -n "$script"
    done
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
        printf '%s\n' \
            'set LRSALVAGE_TEST_CATALOG and LRSALVAGE_TEST_PREVIEWS, in this' \
            "worktree's .env or in ${XDG_CONFIG_HOME:-$HOME/.config}/lrsalvage/real.env" \
            '(see .env.example).' >&2
        exit 1
    fi
    # The rendered-cache row needs its own pair of names and its own gate;
    # `--ignored` would otherwise run it here and fail on a machine that holds a
    # converted cache and no rendered one.
    cargo test --workspace -- --ignored --skip a_fresh_render
}

# A cache Lightroom rendered from scratch: two pyramids per image, which is the
# state `format-notes` §5.10 measured and ADR 0008 decides a rule for. Its own
# pair of names, because it is its own evidence.
gate_real_render() {
    printf '%s\n' '== rendered cache'
    load_env
    if [ -z "${LRSALVAGE_RENDER_CATALOG:-}" ] || [ -z "${LRSALVAGE_RENDER_PREVIEWS:-}" ]; then
        printf '%s\n' \
            'set LRSALVAGE_RENDER_CATALOG and LRSALVAGE_RENDER_PREVIEWS to a cache' \
            "Lightroom rendered, in this worktree's .env or in" \
            "${XDG_CONFIG_HOME:-$HOME/.config}/lrsalvage/real.env (see .env.example)." >&2
        exit 1
    fi
    cargo test -p lrsalvage-cli --test render -- --ignored
}

all_gates='fmt shell clippy test doc build typos deny'

if [ "$#" -eq 0 ]; then
    set -- $all_gates
fi

for gate in "$@"; do
    case "$gate" in
        real-render) gate_real_render ;;
        fmt | shell | clippy | test | doc | build | typos | deny | real) "gate_$gate" ;;
        *)
            printf 'unknown gate: %s (one of: %s real real-render)\n' "$gate" "$all_gates" >&2
            exit 1
            ;;
    esac
done
