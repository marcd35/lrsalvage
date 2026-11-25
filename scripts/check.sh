#!/bin/sh
# The one list of gates.
#
# `AGENTS.md` and `README.md` point here instead of repeating these commands, and
# CI runs the Rust gates through this file, so a gate cannot be documented weaker
# than it is enforced — which is exactly how `cargo clippy -- -D warnings` drifted
# from CI's `--all-targets` and stopped linting test code.
#
#   scripts/check.sh              every default gate, in order
#   scripts/check.sh clippy fmt   only those, in the order given
#   scripts/check.sh real         the #[ignore]d tests, against real catalogs
#   scripts/check.sh real-render  the #[ignore]d tests, against a rendered cache
#   scripts/check.sh real-1to1     the #[ignore]d tests, against a 1:1 cache
#   scripts/check.sh real-discard  the #[ignore]d tests, against a discarded cache
#   scripts/check.sh browser      Chromium app and CLI output parity
#
# The four real gates are separate evidence families: a machine can hold a
# converted cache without a rendered one, a rendered one without a 1:1 build, or
# no copy of a discard at all, and each gate fails rather than skipping when its
# own pair is unset. Each runs only its own tests, so adding a family cannot reach
# into another's and break it.
#
# `package` is the slow one — two builds and two test runs — so name it while
# iterating and let the no-argument run carry it at hand-off. The fast four earn
# the same, and in this order:
#
#   scripts/check.sh fmt shell doc typos
#
# They run in seconds, and `doc` is the one a new public item breaks — a doc
# comment that links a private item fails `-D warnings`. `doc` sits after `test`
# in the full list, so a list run pays a test run to learn it and then starts
# over; the five by name cost less than one of those restarts.
#
# `js` joins them because the page and the harness are the one language here that
# `cargo` never parses: a broken string in `web/*.js` otherwise waits for the
# twelve-minute browser run, or for a user.
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
# The four pairs are read from up to three files, in the order and by the rule
# `scripts/load-env.sh` owns: the machine's file, the file
# `lrsalvage.realEnvFile` names, then this worktree's `.env`, all three read so
# the worktree's value wins and a variable set in only one is still found. The
# only gate that verifies a byte-level claim must not be unreachable from a fresh
# worktree, which is what per-worktree configuration made it.
#
#   git config --global lrsalvage.realEnvFile ~/real-lrsalvage.env
#
# `require_pair` prints `env_files` below, which is the list that loader actually
# read, not a second copy of the candidates.
env_files=''
load_env() {
    . ./scripts/load-env.sh
    env_files=$load_env_files
}

gate_fmt() {
    printf '%s\n' '== fmt'
    cargo fmt --all --check
}

# The one language this repository writes outside Rust is shell, and nothing else
# reads it: `cargo` never parses `scripts/*.sh`, and a broken quote or heredoc in
# a gate script fails the day someone runs it. `sh -n` parses without executing,
# needs no dependency, and runs on all three CI systems.
#
# The same files hold the gate list itself, and CI is where most of it runs — so
# this gate reads the second thing as well: every name in `all_gates` has a
# `ci.yml` step that runs it, or it runs nowhere but the machine of whoever
# remembers. `typos` and `deny` are the two whose CI is a vendor action, named
# here so the exemption is written down rather than discovered.
gate_shell() {
    printf '%s\n' '== shell'
    for script in scripts/*.sh .githooks/pre-commit; do
        [ -f "$script" ] || continue
        sh -n "$script"
    done
    ci_gates=$(sed -n 's/^[[:space:]]*run:[[:space:]]*sh scripts\/check\.sh //p' \
        .github/workflows/ci.yml | tr '\n' ' ')
    for gate in $all_gates; do
        case "$gate" in
            typos | deny) continue ;;
        esac
        case " $ci_gates " in
            *" $gate "*) ;;
            *)
                printf '%s\n' \
                    "$gate is in all_gates and no ci.yml step runs it," \
                    'so it only runs where somebody remembers it:' \
                    "  sh scripts/check.sh $gate" >&2
                exit 1
                ;;
        esac
    done
}

# The page and the harness are JavaScript, and nothing else here parses them:
# `cargo` never sees `web/*.js`, and the `shell` gate stops at `sh -n`. So a
# broken string in a page module waits for the twelve-minute browser run or for a
# user. Parsing a module needs no dependency and takes a second; `check-js.mjs`
# carries the rest of its contract, including what it cannot see.
gate_js() {
    printf '%s\n' '== js'
    if ! command -v node >/dev/null 2>&1; then
        printf '%s\n' 'missing node: nodejs.org' >&2
        exit 1
    fi
    node --no-warnings --experimental-vm-modules scripts/check-js.mjs
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

# The second target. `lrsalvage-core` builds for `wasm32-unknown-unknown` and must
# keep doing so: ADR 0012 D10 makes the target a fourth one the gate list owes, and
# the browser app (#117) links this crate unchanged. The two `publish = false`
# crates are here too, because they are what a page loads and no other gate builds
# them for the target: `lrsalvage-app` is the shipped page's, `lrsalvage-web` is
# the parity harness's.
#
# Linted as well as compiled: `--lib` is the surface an app links (the test targets
# need the native-only `lrsalvage-fixtures`), and before #144 no gate built the
# browser half at all. CI's `wasm` job supplies the target and `clippy`; the two
# compiler variables a machine may point somewhere else are `.env.example`'s to
# document and this gate's to read.
#
# The compiler those variables name is checked before cargo runs: a path that has
# moved otherwise fails inside `cc`, after every gate before this one has already
# spent its time, and reports only `gate failed: wasm`. The check is
# `scripts/load-env.sh`'s, because `scripts/browser-app.sh` needs the same one.
gate_wasm() {
    printf '%s\n' '== wasm32-unknown-unknown'
    require rustup 'rustup.rs'
    if ! rustup target list --installed | grep -qx wasm32-unknown-unknown; then
        printf '%s\n' \
            'wasm32-unknown-unknown is not installed: rustup target add wasm32-unknown-unknown' >&2
        exit 1
    fi
    load_env
    require_wasm_compiler || exit 1
    cargo clippy --lib --locked --target wasm32-unknown-unknown \
        -p lrsalvage-core -p lrsalvage-app -p lrsalvage-web -- -D warnings
}

# The five files a release attaches beside its archives — four completion
# scripts and one man page — are **generated** from the `clap` definition, so the
# committed copies are a claim about this binary, and no other gate reads them:
# `clippy` and `test` lint the definition, not the five files beside it. This
# regenerates them into a scratch directory and diffs the result against the
# committed tree, so a flag, a default or a help string that changed without the
# artifacts changing fails a run rather than a user who installed a script naming
# a flag this binary no longer has.
#
# `diff -r` rather than a list of names, so this gate holds no inventory of its
# own: the directory the generator writes and the committed directory are the two
# sides, and a file the generator stopped writing, or one somebody added by hand,
# fails here too. Where the five names *are* written — `release.yml`'s upload
# list and its `gh release create` arguments, and the Artifact box in
# `docs/release-0.4.md` — those are explicit paths by design, and each fails
# loudly when a name is wrong: `if-no-files-found: error` there, `gh` here.
#
# The red arm of this gate was proved once, in a scratch copy of the tree with a
# flag renamed in `src/cli.rs`: `#62`'s pull request records the run.
#
# The same five names are written a second time in `release.yml`, because a
# workflow cannot read prose, so this gate holds that copy to the generator: the
# literal `dist/<name>` paths there — the upload list and the arguments to
# `gh release create` — must be exactly what was just written here. Globs and
# `${{ }}` expansions are skipped, because they name no file. What it does not
# tell apart is the two lists inside `release.yml`; a name dropped from one and
# kept in the other is `if-no-files-found` and `gh`'s own refusal, at tag time.
gate_completions() {
    printf '%s\n' '== completions'
    staged_dir=$(mktemp -d)
    generated=$(cargo run --quiet -p lrsalvage-cli --example artifacts -- "$staged_dir" | sort)
    printf '%s\n' "$generated"
    if ! diff -ru crates/lrsalvage-cli/artifacts "$staged_dir"; then
        printf '%s\n' \
            'the committed completions and man page are not what this binary generates;' \
            'regenerate them with: cargo run -p lrsalvage-cli --example artifacts -- crates/lrsalvage-cli/artifacts' >&2
        exit 1
    fi
    attached=$(grep -v '[$*]' .github/workflows/release.yml \
        | grep -oE 'dist/[A-Za-z0-9._-]+' | sed 's|^dist/||' | sort -u)
    if [ "$generated" != "$attached" ]; then
        printf '%s\n' \
            'release.yml does not attach what this generator writes:' \
            "  generated: $(printf '%s ' $generated)" \
            "  attached:  $(printf '%s ' $attached)" >&2
        exit 1
    fi
    # The comparison above is only as good as the checkout: `core.autocrlf`
    # rewrites a text file that `.gitattributes` does not mark, and the diff then
    # blames the generator. `git check-attr` reads the rule git itself applies.
    unmarked=$(git check-attr text -- crates/lrsalvage-cli/artifacts/* \
        | grep -v 'text: unset' || true)
    if [ -n "$unmarked" ]; then
        printf '%s\n' \
            'these generated files are not marked `-text` in .gitattributes, so a' \
            'checkout may rewrite the bytes this gate compares:' \
            "$unmarked" >&2
        exit 1
    fi
    rm -rf "$staged_dir"
    staged_dir=''
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
        "to $3, in ./.env, the file named by \`git config lrsalvage.realEnvFile\`," \
        "or ${XDG_CONFIG_HOME:-$HOME/.config}/lrsalvage/real.env (see .env.example)." \
        "read: ${env_files:-no env file}" >&2
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

all_gates='fmt shell js clippy test doc wasm build completions package typos deny msrv'

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
        browser)
            sh scripts/browser-app.sh
            ;;
        real-render) gate_real_render ;;
        real-1to1) gate_real_1to1 ;;
        real-discard) gate_real_discard ;;
        fmt | shell | js | clippy | test | doc | wasm | build | completions | package | typos | deny | msrv | real) "gate_$gate" ;;
        *)
            # A usage error, not a gate that ran and failed.
            gate_in_flight=''
            printf 'unknown gate: %s (one of: %s real real-render real-1to1 real-discard browser)\n' "$gate" "$all_gates" >&2
            exit 1
            ;;
    esac
done

printf 'all gates green: %s (tree %s)\n' "$*" "$measured_tree"
