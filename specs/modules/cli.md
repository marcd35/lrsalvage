---
id: cli
type: module-design
title: "cli: arguments, presentation and exit codes"
status: draft
parent: architecture
depends-on:
  - catalog
  - previews-db
  - plan
  - extract
  - report
  - masks
---

# cli

## Responsibility

The `lrsalvage` binary: three subcommands (`scan`, `extract`, `masks`), their
flags, the progress bar, the files a run claims, and the exit code it returns.

## Boundary

An adapter. It drives the core pipeline in order and presents the result; it
parses no catalog, cache or payload byte and holds no format knowledge. Anything
it needs to know about an image comes from a `Plan`.

## Decisions

- **One exit-code vocabulary**, and it is the CLI's alone: `0` success (including
  `--help` and `--version`), `1` usage or input error, `2` extract finished with
  some failures, `3` nothing selected to do, `4` unsafe or unwritable
  destination. `scan` and `extract` share the "nothing selected" rule, so they
  agree on `3` for the same inputs; `scan` never returns `2`.
- **A clap parse failure never escapes as `2`**, because `2` already means
  "partial" to a script. Only `--help` / `--version` are successes there, decided
  by whether the error goes to stderr.
- **`scan` writes one thing at most.** Without `--contact-sheet` it writes no file
  anywhere the user can see; with it, the sheet is validated against the same
  destination rule as `--out`.
- **Report files are claimed, never assumed.** `report.csv`, `manifest.json` and
  `extract.csv` each take the next free numbered name (`report (2).csv`) and say
  so, because a name on disk may belong to somebody else. A dry run writes none
  of them.
- **A run that selected nothing is not a failure to report on.** Report files are
  written only when something was selected and the run was not a dry run.
