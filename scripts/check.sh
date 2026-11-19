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
#   scripts/check.sh real-1to1     the #[ignore]d tests, against a 1:1 cache
#   scripts/check.sh real-discard  the #[ignore]d tests, against a discarded cache
#
# Those last four are four evidence families, not one: a machine can hold a
# converted cache without a rendered one, a rendered one without a 1:1 build, or
# no copy of a discard at all, and each gate fails rather than skipping when its
# own pair is unset. Each runs only its own tests, so adding a family cannot reach
# into another's and break it.
#
# `package` is the slow one — two builds and two test runs — so name it while
# iterating and let the no-argument run carry it at hand-off.
#
# A missing tool is a failure, never a skip: a green run must mean CI's green.
#
# Two gates are the one exception to "CI calls this file": `typos` and
# `cargo deny` run in CI through their vendor actions (`crate-ci/typos`,
# `EmbarkStudios/cargo-deny-action`), so this file's `typos` and `deny` are the
# local equivalent of the same two commands, not a second policy.
set -eu

cd "$(dirname "$0")/.."

# A gate log names the tree it measured. Two full runs over different trees read
# alike, and a report's claim is about a tree rather than a date — so printing the
# commit and the dirty count here is what lets a reader tell them apart.
measured_tree="$(git rev-parse --short HEAD 2>/dev/null || printf 'unknown') plus $(git status --porcelain 2>/dev/null | wc -l | tr -d ' ') changed"
printf 'tree %s
' "$measured_tree"

# A gitignored `.env` may name LRSALVAGE_TEST_CATALOG/LRSALVAGE_TEST_PREVIEWS
# (a converted cache), LRSALVAGE_RENDER_CATALOG/LRSALVAGE_RENDER_PREVIEWS (a
# rendered one), LRSALVAGE_ONE_TO_ONE_CATALOG/LRSALVAGE_ONE_TO_ONE_PREVIEWS (a
# cache Lightroom built 1:1 previews for) and
# LRSALVAGE_DISCARD_CATALOG/LRSALVAGE_DISCARD_PREVIEWS (one it discarded the
# standard and 1:1 previews of) so the real-cache tests can be run on the machine
# that holds them. It is never committed: real catalog paths are not a repo
# artefact.
#
# A worktree's own `.env` wins, and the machine's one file is the fallback — the
# only gate that verifies a byte-level claim must not be unreachable from a fresh
# worktree, which is what per-worktree configuration made it.
#
#   git config --global lrsalvage.realEnvFile ~/real-lrsalvage.env
#   ~/.config/lrsalvage/real.env          (the default, when the setting is unset)
load_env() {
    # A relative operand needs its slash: a POSIX `.` searches `PATH` for a name
    # like `.env` and fails on the file that is right here, which made the
    # worktree's own `.env` — the route `.env.example` documents first — dead on
    # every shell that follows the standard.
    for candidate in \
        ./.env \
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

# The second target. `lrsalvage-core` builds for `wasm32-unknown-unknown` and
# must keep doing so: ADR 0012 D10 makes the target a fourth one the gate list
# owes, and the browser app (#117) links this crate unchanged. The gate is in
# scripts/check.sh with every other one, so a local run and this job are the
# same command; what this job adds is the two things that target needs — the
# rust target, and the C compiler `sqlite-wasm-rs` uses for SQLite's
# amalgamation, which is on the runner's PATH as `clang`.
#
# `lrsalvage-web` is here too: it is the crate a page loads, so it is where the
# seam's browser half is compiled and linted for this target. It is not shipped
# (`publish = false`), and nothing else in the list builds it for `wasm32`.
#
# It lints as well as compiles. `--lib` is the surface the app links — the test
# targets need `lrsalvage-fixtures`, which is native-only — and until this slice
# the browser half of the seam was not compiled by any gate at all, so a lint
# there was a lint nobody ran.
#
# Two tools have to be there and neither is skipped. The target is checked here
# and fails with the command that installs it. The C compiler is not checked
# here, because `cc` finds it for this target on PATH (`clang`, which CI's runner
# has) or from `CC_wasm32_unknown_unknown`, which a machine may point anywhere —
# a machine-local path this file must not guess. A machine without one fails the
# build with `cc` naming the tool it could not run, which is loud; the variable
# is documented in `.env.example`.
gate_wasm() {
    printf '%s\n' '== wasm32-unknown-unknown'
    require rustup 'rustup.rs'
    if ! rustup target list --installed | grep -qx wasm32-unknown-unknown; then
        printf '%s\n' \
            'wasm32-unknown-unknown is not installed: rustup target add wasm32-unknown-unknown' >&2
        exit 1
    fi
    load_env
    cargo clippy --lib --locked --target wasm32-unknown-unknown \
        -p lrsalvage-core -p lrsalvage-web -- -D warnings
}

# The artifact, not the tree. Every other gate reads this checkout, so a package
# that cannot build from its own generated manifest — a test tree the published
# metadata drops, a dev-dependency Cargo strips, a README the crate page points at
# and the tarball lacks — is invisible to all of them. `#96` is that failure
# measured: an unpacked crate whose `cargo test` cannot compile because
# `lrsalvage-fixtures` is a `path`-only dev-dependency the published manifest
# drops. So every tarball is built **and** tested where it is unpacked, with no
# checkout in reach: a crate that only builds is a package that has not been
# proved, and `#123` is that half measured (`src/`'s `#[cfg(test)]` sites, which
# no `exclude` reaches, kept core's suite out of its tarball).
#
# The cli's tarball is verified against the core tarball this run built, by a
# `patch` on the command line. Its generated manifest names `lrsalvage-core` by
# version, and that version is not on crates.io until the release after this one,
# so without the patch verification resolves whatever core *was* published. That
# passed only while the published copy still matched this tree: the first change
# to the seam between the two crates — #63's `--verify-source`, which adds core
# API the cli calls — made the gate verify an older core and fail on the new
# tree. Aimed at the tarball beside it, the gate proves the pair a release
# publishes rather than a pair nobody ships together.
#
# The slowest gate here (two builds and two test runs), and it takes
# `--allow-dirty` so it measures a working tree — which is exactly when it is
# worth running. Name it while iterating, and let the no-argument run carry it at
# hand-off.
gate_package() {
    printf '%s\n' '== package'
    # Outside the checkout, because that is what an auditor has: unpacked in this
    # tree, a shipped test could walk up to `specs/` or to another member and the
    # gate would still pass while theirs failed. `mktemp -d` is the shell's; the
    # one EXIT trap removes whatever is left of it, pass or fail.
    staged_dir=$(mktemp -d)
    core_root=''
    for crate in lrsalvage-core lrsalvage-cli; do
        patch=''
        if [ -n "$core_root" ]; then
            # `native_path` translates the shell's spelling into one `cargo` reads:
            # the staged tree is no longer under the repository, so it is absolute
            # and, on msys, `C:`-less until cygpath rewrites it.
            patch=$(native_path "$core_root")
            cargo package --allow-dirty -p "$crate" \
                --config "patch.crates-io.lrsalvage-core.path=\"$patch\""
        else
            cargo package --allow-dirty -p "$crate"
        fi
        listed=$(cargo package --list --allow-dirty -p "$crate")
        printf '%s\n' "$listed" | grep -qx 'README.md' || {
            printf '%s\n' "$crate: the package must carry the README.md its manifest points at" >&2
            exit 1
        }
        # A packaged test file that imports a stripped dev-dependency cannot
        # build where the package lands, so the exclude list has a hole: the gate
        # that would have caught #96's original shape.
        for path in $(printf '%s\n' "$listed" | grep '^tests/.*\.rs$'); do
            if grep -q 'lrsalvage_fixtures' "crates/$crate/$path"; then
                printf '%s\n' \
                    "$crate: $path ships in the package and imports lrsalvage-fixtures," \
                    'which the published manifest drops — add it to [package] exclude' >&2
                exit 1
            fi
        done
        archive=$(ls -t "target/package/$crate"-*.crate | head -1)
        unpacked="$staged_dir/$crate"
        mkdir -p "$unpacked"
        tar -xzf "$archive" -C "$unpacked"
        root="$unpacked/$(tar -tzf "$archive" | head -1 | cut -d/ -f1)"
        # Every tarball is built with `--all-targets` and then run: the compile
        # of the test targets is one half of what an auditor does, and the run is
        # the half that proves anything. Core needs no patch — it is its own
        # package — and is unpacked before the cli is packaged, so the cli's
        # generated manifest can be aimed at the core tarball beside it.
        if [ "$crate" = lrsalvage-cli ]; then
            (cd "$root" && cargo build --all-targets \
                --config "patch.crates-io.lrsalvage-core.path=\"$patch\"" \
                && cargo test --config "patch.crates-io.lrsalvage-core.path=\"$patch\"")
        else
            core_root=$root
            (cd "$root" && cargo build --all-targets && cargo test)
        fi
    done
    # Removed on the success path too, so a full gate run does not carry two
    # target directories to the end; clearing the name leaves the trap nothing to
    # do here.
    rm -rf "$staged_dir"
    staged_dir=''
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

# The MSRV the manifest publishes, proven rather than asserted. The number is
# `rust-version` in `Cargo.toml` — every member inherits it — and this gate reads
# it back through `cargo metadata` instead of repeating it, so raising the number
# is the whole edit and this command follows. CI's `msrv` job runs this file, not
# a private copy of the command.
gate_msrv() {
    printf '%s\n' '== msrv'
    require rustup 'rustup.rs'
    # The metadata read is toolchain-independent, so it runs on `stable` rather
    # than on whatever `rust-toolchain.toml` pins: a checkout's pin would
    # otherwise download the *build* toolchain just to answer a question about a
    # manifest, on a gate whose whole point is a different compiler.
    version=$(RUSTUP_TOOLCHAIN=stable cargo metadata --no-deps --format-version 1 \
        | grep -o '"rust_version":"[0-9][0-9.]*"' \
        | sed 's/[^0-9.]//g' \
        | awk -F. '{ printf "%.0f %s\n", $1 * 1000000 + $2 * 1000 + $3, $0 }' \
        | sort -n | tail -1 | cut -d' ' -f2)
    if [ -z "$version" ]; then
        printf '%s\n' 'Cargo.toml publishes no rust-version to prove' >&2
        exit 1
    fi
    printf 'rust-version = %s\n' "$version"
    rustup toolchain install "$version" --profile minimal
    # Its own target directory: sharing `target/` with the pinned toolchain would
    # make every alternating run rebuild the dependency tree from scratch, once
    # here and once in the next gate.
    CARGO_TARGET_DIR=target/msrv cargo "+$version" check --workspace --all-targets --locked
}

require() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'missing %s: %s\n' "$1" "$2" >&2
        exit 1
    fi
}

# A path `cargo` can read when the shell's spelling is not the platform's: an
# msys/git-bash path is `C:`-less, and `cygpath -m` is the one translation that
# keeps forward slashes (a TOML string cannot carry a raw backslash). On every
# other system the path already means what it says.
native_path() {
    if command -v cygpath >/dev/null 2>&1; then
        cygpath -m "$1"
    else
        printf '%s' "$1"
    fi
}

# A family whose own pair is unset fails and names it, rather than skipping or
# borrowing another family's evidence. One guard for all four families, so a fix
# to this message cannot leave one gate telling a user a different path.
#
#   require_pair <catalog name> <previews name> <what the cache is>
require_pair() {
    # POSIX sh has no `${!name}`, and `set -u` makes the read matter: the two
    # names arrive as strings and are dereferenced indirectly.
    eval "_catalog=\${$1:-}"
    eval "_previews=\${$2:-}"
    if [ -n "$_catalog" ] && [ -n "$_previews" ]; then
        return 0
    fi
    printf '%s\n' \
        "set $1 and $2" \
        "to $3, in this worktree's .env or in" \
        "${XDG_CONFIG_HOME:-$HOME/.config}/lrsalvage/real.env (see .env.example)." >&2
    return 1
}

# The real-cache rows: `#[ignore]`d and reached only from this machine's own
# catalogs. Evidence for `README.md`'s compatibility claims comes from here.
gate_real() {
    printf '%s\n' '== real catalogs'
    load_env
    require_pair LRSALVAGE_TEST_CATALOG LRSALVAGE_TEST_PREVIEWS \
        'a converted cache' || exit 1
    # This gate's own rows, not every ignored test in the workspace: `--ignored`
    # over the whole workspace would make each evidence family depend on the
    # other's names (and on its env pair being set on this machine).
    cargo test -p lrsalvage-core --lib -- --ignored
}

# A cache Lightroom rendered from scratch: two pyramids per image, which is the
# state `format-notes` §5.10 measured and ADR 0008 decides a rule for. Its own
# pair of names, because it is its own evidence.
gate_real_render() {
    printf '%s\n' '== rendered cache'
    load_env
    require_pair LRSALVAGE_RENDER_CATALOG LRSALVAGE_RENDER_PREVIEWS \
        'a cache Lightroom rendered from scratch' || exit 1
    # Scoped to the rendered-cache row, for the same reason `real` is scoped.
    cargo test -p lrsalvage-cli --test render -- --ignored
}

# A cache Lightroom built 1:1 previews for: one pyramid per image and no
# companion, which is `format-notes` §5.10's catalog C, and the absence case the
# companion rule (ADR 0008, ADR 0009) only ever decides about. Its own pair of
# names, because it is its own evidence.
gate_real_1to1() {
    printf '%s\n' '== 1:1 cache'
    load_env
    require_pair LRSALVAGE_ONE_TO_ONE_CATALOG LRSALVAGE_ONE_TO_ONE_PREVIEWS \
        'a cache Lightroom built 1:1 previews for' || exit 1
    # Scoped to the 1:1 row, for the same reason the other three gates are scoped.
    cargo test -p lrsalvage-cli --test one_to_one -- --ignored
}

# A cache Lightroom discarded the standard and 1:1 previews of: §5.11's state,
# where every preview pyramid shrank rather than emptied, no companion is left,
# and `Pyramid.quality` was rewritten to describe the levels a pyramid now holds.
# Its own pair of names, because it is its own evidence — and the only family
# whose copy Lightroom wrote a deletion into.
gate_real_discard() {
    printf '%s\n' '== discarded cache'
    load_env
    require_pair LRSALVAGE_DISCARD_CATALOG LRSALVAGE_DISCARD_PREVIEWS \
        'a cache Lightroom discarded the standard and 1:1 previews of' || exit 1
    # Scoped to the discard row, for the same reason the other gates are scoped.
    cargo test -p lrsalvage-cli --test discard -- --ignored
}

all_gates='fmt shell clippy test doc wasm build package typos deny msrv'

if [ "$#" -eq 0 ]; then
    set -- $all_gates
fi

# `set -e` aborts inside a gate, so the log ends in whatever cargo said last with
# no line naming the gate that failed — and a green run leaves a reader counting
# `==` headers, or running the whole thing again to prove it. The trap names the
# failure; the last line names the success.
gate_in_flight=''
# A gate that stages files outside the checkout registers its directory here, so
# this one trap removes it whether the gate passed, failed or aborted.
staged_dir=''
on_exit() {
    status=$?
    if [ -n "$staged_dir" ]; then
        rm -rf "$staged_dir"
    fi
    if [ "$status" -ne 0 ] && [ -n "$gate_in_flight" ]; then
        printf 'gate failed: %s\n' "$gate_in_flight" >&2
    fi
    exit "$status"
}
trap on_exit EXIT

for gate in "$@"; do
    gate_in_flight=$gate
    case "$gate" in
        real-render) gate_real_render ;;
        real-1to1) gate_real_1to1 ;;
        real-discard) gate_real_discard ;;
        fmt | shell | clippy | test | doc | wasm | build | package | typos | deny | msrv | real) "gate_$gate" ;;
        *)
            # A usage error, not a gate that ran and failed.
            gate_in_flight=''
            printf 'unknown gate: %s (one of: %s real real-render real-1to1 real-discard)\n' "$gate" "$all_gates" >&2
            exit 1
            ;;
    esac
done

printf 'all gates green: %s (tree %s)\n' "$*" "$measured_tree"
