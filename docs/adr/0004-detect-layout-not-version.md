# Detect the preview layout; never gate on a version

**Status:** accepted · **Date:** 2026-11-14

## Context

Two things version independently: the **catalog format** (`Adobe_DBVersion` in
`Adobe_variablesTable`) and the **preview layout** (replaced in Lightroom Classic
13.3, when one `.lrprev` container became one file per level).

They are not the marketing version, and measuring one catalog proves it. The same
78-image catalog, before and after an in-place upgrade by Lightroom Classic 16.0:

| | Before | After |
|---|---|---|
| `Adobe_DBVersion` | `1200001` | `1504001` |
| `Adobe_CrsVersion` | `15.4.0` (LrC 12.4) | `18.0.0` (LrC 15.0's ACR) |
| Preview layout | `.lrprev` pyramid | one JPEG per level |

A 16.0 application wrote `1504001` and `18.0.0`. Both numbers name a *format
generation* — the release that last changed the format — not the application that
wrote the file. Any gate keyed to the marketing major would therefore mislabel its
first data point, and would refuse a cache the reader already understands.

## Decision

The tool's promise is structural: **it reads any cache whose payload layout it
recognises**, and it reports the catalog's version strings as observations.

- `layout` is one of five words: **`legacy`** (only `.lrprev` containers),
  **`modern`** (only loose payload files), **`mixed`** (both kinds present
  anywhere in the cache), **`none`** (an index and no payload files), **`unknown`**
  (payload-like files exist but none is recognised).
- `mixed` describes the cache, not an image. Per-image supersession stays a
  separate `superseded` count.
- `db_version` and `crs_version` are reported **verbatim** as strings. They are
  never mapped to a Lightroom release, and the text summary shows only the layout.
- No version refuses a run. An `unknown` layout is reported with one hint and
  exits 3 — the same "nothing extracted" code as an empty cache — never a fatal
  SQLite error.
- The layout and both strings surface in the `scan` summary, `--json`, and
  `manifest.json`; `report.csv` stays per-image and unchanged.
- The README's rows assert the **layout** a generation uses, with a `Released`
  column for the user's benefit and a `Catalog format` column carrying only
  measured values or "not measured". "Verified" continues to mean the tool ran
  against real files of that generation.

## Consequences

- There is no version table for the code to enforce, so a new Lightroom release
  cannot silently turn a readable cache into an empty run: it either reads, or it
  reports `unknown` with the shape it saw.
- Every claim in the README is anchored to a measured cache or labelled
  unmeasured. Two data points cannot support a version-to-generation mapping, and
  none is published.
- Tests pin five words, two verbatim strings and an exit code — not a growing set
  of version numbers.
- `unknown` needs a rule for what counts as payload-like. Keep it narrow: files,
  not directories, sniffed for `FF D8`. Widening it is a decision, not a tweak.
- Users who paste a `scan --json` into an issue hand over the layout and both
  version strings, which is what makes a verification report possible without
  full paths.

## Considered options

- **Gate on the Lightroom major version.** Simple to explain and wrong on the
  first measurement: a 16.0 catalog reports a 15.4 format generation.
- **Support a named list of measured versions.** Honest but brittle: every
  release needs a code change before the tool may read a cache it already parses,
  and the list encodes a mapping that does not exist.
- **Fold `unknown` into `none`.** One fewer state, but "this cache holds nothing"
  and "this cache holds something I cannot read" are the two cases a user most
  needs told apart.
- **Counts instead of an enum.** More precise for a maintainer, worse for every
  other reader, and it pushes the interpretation into each caller.
