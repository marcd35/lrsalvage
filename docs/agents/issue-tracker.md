# Reading a ticket

Issues live at <https://github.com/marcd35/lrsalvage/issues>. Read one with:

```console
gh issue view <n> --json number,title,body,labels,milestone,state
```

Never fetch the issue's HTML page — the personal `AGENTS.md` carries that rule and
the size it costs. `gh issue view` cannot combine `--comments` with `--json`, so
run it twice when the comments matter.

A ticket's own **Decisions (recorded — do not re-open)** section is an accepted
design, not a proposal: it names the specs and ADRs for the slice, and the code
follows them rather than re-arguing them.

This repo commits no `.github/branch-policy.json`, so a branch carries no issue
number; the workspace's branch is its own.
