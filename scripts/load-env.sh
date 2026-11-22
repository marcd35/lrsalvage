#!/bin/sh
# The one list of the files a machine's own values live in — sourced, not run.
#
# `scripts/check.sh`, `scripts/browser-app.sh` and
# `scripts/scan-progress-fixture.sh` all read the same three candidates in the
# same order. A copy of the rule is a copy that drifts: `browser-app.sh` held the
# same three in the opposite order, so a worktree's `.env` lost to the machine's
# file there while `check.sh` promised it won.
#
# The caller sets the working directory to the repository root first; a relative
# `.env` is this checkout's.
#
# The order is the machine's file, the file `lrsalvage.realEnvFile` names, then
# this worktree's `.env` — and **every candidate is read**, the machine's first
# and the worktree's last, so a variable set in two files takes the worktree's
# value and one set in only one is still found. Stopping at the first file that
# exists is what let a worktree `.env` holding only the wasm C compiler hide the
# catalogue paths on the same machine.
#
#   git config --global lrsalvage.realEnvFile ~/real-lrsalvage.env
#   ~/.config/lrsalvage/real.env          (the default, when the setting is unset)
#   ./.env                                (this worktree's, and it wins)
#
# A name left at `=""` is assigned, not unset, so an empty line in a later file
# wins over a value in an earlier one. Leave the name out rather than copying
# `.env.example`'s empty line into a file that also needs the machine's value.
#
# Sets `load_env_files` to the comma-separated list it read, so a failure can name
# the files it looked in.
load_env_files=''
for _load_env_candidate in \
    "${XDG_CONFIG_HOME:-$HOME/.config}/lrsalvage/real.env" \
    "$(git config --get lrsalvage.realEnvFile 2>/dev/null || true)" \
    ./.env; do
    [ -n "$_load_env_candidate" ] || continue
    # A relative operand needs its slash: a POSIX `.` searches `PATH` for a name
    # like `.env` and fails on the file that is right here.
    [ -f "$_load_env_candidate" ] || continue
    # `set -a` exports what the file assigns: a plain `NAME=value` line is only a
    # shell variable, and every consumer is a child process.
    set -a
    # shellcheck disable=SC1090,SC1091
    . "$_load_env_candidate"
    set +a
    load_env_files="${load_env_files:+$load_env_files, }$_load_env_candidate"
done
unset _load_env_candidate
