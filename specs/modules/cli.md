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
  destination, `5` the catalog changed while it was being copied. `scan` and
  `extract` share the "nothing selected" rule, so they agree on `3` for the same
  inputs — every original present, no usable index, or every level outside the
  floor-and-cap window; `scan` never returns `2`. `main.rs`'s unit test asserts
  the six published numbers and one case per code, and its `EXIT_USAGE` arm names
  every remaining variant rather than defaulting, so a new error cannot compile
  until this mapping decides what it means. Which code an *existing* variant
  takes is the arm beside it: moving one is a review change, and so is the rows'
  prose.
- **`--verify-source` is the proof, and it is opt-in on both commands.** The flag
  routes the one open of the user's catalog through `Catalog::open_verified`, so
  the catalog is hashed, copied and the copy hashed, and a pair that disagrees is
  exit `5` with the path and both digests on stderr — never a warning, because
  the whole claim is that this run only read the evidence. It fails before a plan
  exists, so that run writes no JPEG and no report — `extract`'s `--out` may
  already exist, as it does for any run that fails after the destination check.
  `scan` hands the pair to `--report`; `extract` performs the same check and
  publishes nothing, because `report.csv`, `manifest.json` and `--json` are not
  surfaces for a digest. A passing verified run says nothing extra: the exit code
  and, on `scan`, the report are the answer
  (`adr-0010-what-0-2-promises`).
- **A degraded cache is not an error.** A `previews.db` that is missing or cannot
  be read does not stop the run: the plan comes back with every image whose
  original is not on disk as `no-cache-entry` and every payload pyramid as
  `unmapped`, and the command exits `3`
  (nothing was named, so nothing could be selected) with one hint on stderr. No
  SQLite message escapes as a fatal error for a cache that is present but
  damaged; the hint carries the reason instead. A `--previews` path that is not an
  existing directory is the opposite case — `1`, because there is nothing there to
  salvage and a mistyped path must not read as an empty cache. That check runs
  before either store is opened and, on `extract`, before `--out` is touched, so
  a mistyped cache path is never reported as a bad catalog and never leaves a
  directory behind.
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

  An `unknown` layout has no file to name, so its hint names the count instead:
  how many payload-like files matched no shape, that they are counted and never
  written, and that the layout is therefore unknown. It goes to stderr under
  `--json` and moves no exit code: an `unknown` layout selects nothing, so the
  run is already exit `3`. The hint states a fact about this cache and never
  suggests a workaround (`adr-0004-detect-layout-not-version`).
- **A clap parse failure never escapes as `2`**, because `2` already means
  "partial" to a script. Only `--help` / `--version` are successes there, decided
  by whether the error goes to stderr.
- **`scan` writes only what you name.** Without `--contact-sheet` or `--report` it
  writes no file anywhere the user can see. Each artifact it is given a path for is
  validated and claimed on its own; nothing is invented, and no count of artifacts
  is promised — a count is the part of the rule that keeps changing, while "only
  what you name" does not.
- **`--report <file>` writes the verification report, and never moves the exit
  code by itself.** It is written whenever the flag is given and a plan exists —
  on exit `0` and on exit `3` alike — before the "nothing selected" decision is
  taken, exactly as the contact sheet is. A run that exits `1` before a plan
  exists (a `--previews` that is not a directory, an unreadable catalog) writes
  nothing, because there is no plan to describe, and neither does a run whose
  `--verify-source` check failed inside the acquisition (`5`). A report write
  that fails is exit `4` through the same `at_destination` mapping every named
  destination uses, so a later implementer cannot turn a write failure into `3`,
  and the flag cannot change the code the plan decided. What the file contains is
  `report`'s; the path, the claim and the code are this spec's.
- **The report obeys the `--out` destination rule, and its name is claimed.** Its
  parent is checked with `ensure_safe_destination_pending` before the write — never
  inside the catalog, the cache, or `.lrcat-data`. An existing name takes the next
  free number (`scan (2).json`) and the run says so, because a name on disk may
  belong to somebody else.
- **`--json` and `--report` compose without a collision.** The report goes to a
  path and stdout keeps exactly one object; `--verbose` already moves its lines to
  stderr under `--json`, so a caller combining all three still parses stdout with
  one `serde_json::from_str`.
- **`--json` grows by promise, and it keeps its paths.** The one object carries
  the identifiers (`catalog`, `previews`), every image count, the cache's
  `layout`, `db_version`, `crs_version`, its three cache counts and its flat
  `shapes` — each because ADR 0004, ADR 0006 or ADR 0007 promised it a field, and
  no field is added for convenience. It stays the surface with paths, which is
  why the path-free object is `--report` and not a flag on this one. The
  numbers are the plan's: this module formats them and computes none of them.
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
