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
  - adr-0007-publish-a-verification-report
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
- **`scan` writes only what you name.** Without `--contact-sheet` or `--report` it
  writes no file anywhere the user can see. Each artifact it is given a path for is
  validated and claimed on its own; nothing is invented, and no count of artifacts
  is promised — a count is the part of the rule that keeps changing, while "only
  what you name" does not.
- **`--report <file>` writes the verification report, and never moves the exit
  code.** It is written whenever the flag is given and a plan exists — on exit `0`
  and on exit `3` alike — before the "nothing selected" decision is taken, exactly
  as the contact sheet is. A run that exits `1` before a plan exists (a
  `--previews` that is not a directory, an unreadable catalog) writes nothing,
  because there is no plan to describe. A report write that fails is exit `4`
  through the same `at_destination` mapping every named destination uses, so a
  later implementer cannot turn a write failure into `3`, and the flag cannot
  change the code the plan decided. What the file contains is `report`'s; the
  path, the claim and the code are this spec's.
- **The report obeys the `--out` destination rule, and its name is claimed.** Its
  parent is checked with `ensure_safe_destination_pending` before the write — never
  inside the catalog, the cache, or `.lrcat-data`. An existing name takes the next
  free number (`scan (2).json`) and the run says so, because a name on disk may
  belong to somebody else.
- **`--json` and `--report` compose without a collision.** The report goes to a
  path and stdout keeps exactly one object; `--verbose` already moves its lines to
  stderr under `--json`, so a caller combining all three still parses stdout with
  one `serde_json::from_str`.
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
