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

**A slice that belongs to a release has its acceptance criteria in
`docs/release-*.md`.** `grep -l '#<n>' docs/release-*.md` before the first design
round, not after the code disagrees with it: the checklist's box for that slice is
more specific than the ticket — it names the seam the suite must drive, the
boundary a test must assert at equality, the docs the slice must touch, and the
"cannot show" a fixture row must record — and where the two disagree the box is
the newer decision. The ticket keeps the *why*; the box keeps the *done when*.

**A ticket's stated cause is a claim, not evidence.** These tickets are written
from measurements, and a measurement can be misread: #42 said `scan` "exits 3
because each thumbnail pyramid is a payload no image names", when exit 3 comes
from selecting nothing and `unmapped` never moves an exit code. Designing on that
sentence would have decided the wrong problem. Check the mechanism against the
code before the first design round, and correct the ticket in the round where it
first matters — a wrong cause quietly sets the stakes for every choice after it.

A branch cut after #134 names the issue it works, so read that number before the
first commit rather than after the code disagrees with it;
`.github/branch-policy.json` holds the shape, and `AGENTS.md` says to rename the
harness default.
