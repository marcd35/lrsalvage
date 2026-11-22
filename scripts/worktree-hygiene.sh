#!/bin/sh
# Report what a checkout has accumulated: every worktree with its branch and its
# dirty count, and every local and remote branch paired with the state of the
# pull request that carries it.
#
#   scripts/worktree-hygiene.sh [--sizes]
#
# This is a report, not a gate. It writes nothing, deletes nothing, and asks
# `git worktree list` plus one `gh pr list`, so it answers in a second however
# many trees the machine holds.
#
# The column that decides a deletion is `pr`. This repository squash-merges, so a
# landed branch's tip is never an ancestor of `main`: `git branch --merged main`
# and `git merge-base --is-ancestor` each answer a different question, and both
# under-report. A branch's content has landed when its pull request says MERGED,
# and only then; `open` means a pull request still carries it, so keep it.
#
# `--sizes` is off by default on purpose. Measuring a worktree walks every file
# under its `target/`, which on Windows costs tens of minutes across a dozen
# trees — buy that number only when the number is the question. Each tree is
# capped at 20 seconds so the option cannot become the slow path it exists to
# avoid, and a tree that hits the cap reads `unmeasured` rather than a number.
set -eu

usage() {
    printf 'usage: %s [--sizes]\n' "${0##*/}" >&2
    exit 2
}

sizes=0
for arg in "$@"; do
    case "$arg" in
        --sizes) sizes=1 ;;
        -h | --help) usage ;;
        *) usage ;;
    esac
done

cd "$(dirname "$0")/.."

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

# --- pull requests, in one call --------------------------------------------
#
# An empty result and a failed command are indistinguishable once the failure is
# swallowed, so a state file that is empty or absent is reported as unknown
# rather than read as "no branch has a pull request".
prs=$tmp/prs.tsv
: >"$prs"
prs_known=1
if ! command -v gh >/dev/null 2>&1; then
    printf 'note: gh is not installed, so no branch below is called landed\n' >&2
    prs_known=0
elif ! gh pr list --state all --limit 500 \
    --json headRefName,state,number \
    --jq '.[] | [.headRefName, .state, (.number | tostring)] | @tsv' >"$prs"; then
    printf 'note: `gh pr list` failed, so no branch below is called landed\n' >&2
    prs_known=0
elif [ ! -s "$prs" ]; then
    printf 'note: `gh pr list` returned nothing, so no branch below is called landed\n' >&2
    prs_known=0
fi

# A branch name can carry more than one pull request (a reused name, a reopened
# run). The state printed is the strongest one, since that is the one a deletion
# decision turns on: an open pull request outranks a merged one.
#
#   pr_state <branch>   prints "state #number<TAB>MERGED|OPEN|CLOSED|-"
pr_state() {
    awk -F'	' -v b="$1" '
        BEGIN { best = 0 }
        $1 == b {
            rank = ($2 == "OPEN") ? 3 : ($2 == "MERGED") ? 2 : 1
            if (rank > best) { best = rank; label = $2 " #" $3; state = $2 }
        }
        END {
            if (best == 0) printf "-\t-"
            else printf "%s\t%s", label, state
        }
    ' "$prs"
}

# --- worktrees -------------------------------------------------------------
#
# One `git worktree list` per run: no directory is walked, which is why this is
# instant where `du` over the same trees is not.
git worktree list --porcelain >"$tmp/porcelain"
awk '
    /^worktree / { if (path != "") print path "|" branch
                   path = substr($0, 10); branch = "detached" }
    /^branch /   { branch = substr($0, 8); sub("^refs/heads/", "", branch) }
    END          { if (path != "") print path "|" branch }
' "$tmp/porcelain" >"$tmp/worktrees.tsv"

main_path=$(sed -n '1s/|.*//p' "$tmp/worktrees.tsv")

printf '== worktrees\n'
printf '%-10s %-12s %-45s %s\n' dirty pr branch path
worktree_branches=$tmp/worktree-branches
: >"$worktree_branches"
clean=0
total=0
landed_held=0
while IFS='|' read -r path branch; do
    total=$((total + 1))
    printf '%s\n' "$branch" >>"$worktree_branches"
    mark=''
    if [ "$path" = "$main_path" ]; then mark=' (main checkout)'; fi
    status=$tmp/status
    if git -C "$path" status --porcelain >"$status" 2>/dev/null; then
        dirty=$(wc -l <"$status" | tr -d ' ')
        if [ "$dirty" = 0 ]; then
            clean=$((clean + 1))
        else
            untracked=$(grep -c '^??' "$status" || true)
            dirty="$dirty ($untracked new)"
        fi
    else
        dirty='missing'
    fi
    state=$(pr_state "$branch")
    label=$(printf '%s' "$state" | cut -f1)
    kind=$(printf '%s' "$state" | cut -f2)
    if [ "$kind" = MERGED ]; then landed_held=$((landed_held + 1)); fi
    printf '%-10s %-12s %-45s %s%s\n' "$dirty" "$label" "$branch" "$path" "$mark"
    if [ "$sizes" -eq 1 ]; then
        if command -v timeout >/dev/null 2>&1; then
            size=$(timeout 20 du -sh "$path" 2>/dev/null | cut -f1 || true)
            printf '%-10s %-12s %-45s %s\n' '' '' '' \
                "size ${size:-unmeasured within 20s}"
        else
            printf '%-10s %-12s %-45s %s\n' '' '' '' \
                'size unmeasured (no timeout on this host)'
        fi
    fi
done <"$tmp/worktrees.tsv"

# --- refs ------------------------------------------------------------------
printf '\n== local branches\n'
printf '%-12s %s\n' pr branch
landed_local=0
open_local=0
for branch in $(git for-each-ref --format='%(refname:short)' --sort=refname refs/heads); do
    state=$(pr_state "$branch")
    label=$(printf '%s' "$state" | cut -f1)
    kind=$(printf '%s' "$state" | cut -f2)
    note=''
    if grep -qxF "$branch" "$worktree_branches"; then note=' (in a worktree)'; fi
    case "$kind" in
        MERGED) landed_local=$((landed_local + 1)) ;;
        OPEN) open_local=$((open_local + 1)) ;;
    esac
    printf '%-12s %s%s\n' "$label" "$branch" "$note"
done

printf '\n== remote refs\n'
printf '%-12s %s\n' pr branch
landed_remote=0
open_remote=0
for ref in $(git for-each-ref --format='%(refname:lstrip=3)' --sort=refname refs/remotes/origin); do
    if [ "$ref" = HEAD ]; then continue; fi
    state=$(pr_state "$ref")
    label=$(printf '%s' "$state" | cut -f1)
    kind=$(printf '%s' "$state" | cut -f2)
    case "$kind" in
        MERGED) landed_remote=$((landed_remote + 1)) ;;
        OPEN) open_remote=$((open_remote + 1)) ;;
    esac
    printf '%-12s %s\n' "$label" "$ref"
done

printf '\n== summary\n'
printf 'worktrees: %s (clean: %s)\n' "$total" "$clean"
printf 'merged local branches: %s\n' "$landed_local"
printf 'merged remote refs: %s\n' "$landed_remote"
printf 'branches an open pull request carries: %s\n' \
    "$((open_local + open_remote))"
if [ "$landed_held" -gt 0 ]; then
    printf 'merged branches held by a worktree (remove it first): %s\n' \
        "$landed_held"
fi
if [ "$prs_known" -eq 0 ]; then
    printf 'pull-request state is unknown this run, so nothing is called landed\n'
fi
exit 0
