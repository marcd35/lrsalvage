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
references:
  - adr-0004-detect-layout-not-version
  - adr-0006-degrade-without-guessing
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
- **A degraded cache is not an error.** A `previews.db` that is missing or cannot
  be read does not stop the run: the plan comes back with every image whose
  original is not on disk as `no-cache-entry` and every payload pyramid as
  `unmapped`, and the command exits `3`
  (nothing was named, so nothing could be selected) with one hint on stderr. No
  SQLite message escapes as a fatal error for a cache that is present but
  damaged; the hint carries the reason instead. A `--previews` path that is not an
  existing directory is the opposite case — `1`, because there is nothing there to
  salvage and a mistyped path must not read as an empty cache.
- **The degraded hints are one block each, and they name the file.** Missing
  index:

  ```text
  hint: <cache>/previews.db is missing, so <n> payloads cannot be matched to any of the <m> catalog images.
        They are listed as unmapped and never written; scan --verbose prints each path, extract records them in report.csv.
  ```

  An unreadable index prints the same two lines with `could not be read (sqlite
  error: <reason>)` in place of `is missing` and `fix or move the file to name
  them` in place of the last clause. Counts come from the plan; no hint ever says
  the cache holds no previews.
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
- **A degraded run may write its report without selecting anything.**
  `report.csv` and `manifest.json` are written when something was selected **or**
  the plan found unclaimed payloads (`unmapped > 0`), because the list of orphans
  is the one thing such a run has to hand over. A dry run writes none of them, and
  a run with nothing selected and nothing unclaimed writes no file at all: nothing
  selected is not, by itself, something to report on.
