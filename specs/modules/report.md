---
id: report
type: module-design
title: "report: the terminal summary, the CSV, the manifest, the contact sheet"
status: draft
parent: architecture
depends-on:
  - plan
  - lrprev
  - jpeg
  - model
references:
  - adr-0005-discover-payloads-by-index
  - adr-0006-degrade-without-guessing
---

# report

## Responsibility

Say what a plan means, for a human and for a script: the one-block summary, one
line per plan item, `report.csv`, `manifest.json`, and a self-contained HTML
contact sheet.

## Boundary

Reads a plan and, for the contact sheet, the payload files it needs. Writes to the
path its caller named and invents no path of its own; the numbered-name rule for
report files belongs to `cli`. It decides nothing and adds no metadata.

## Decisions

- **A degraded run says what it saw, in one hint.** Every line that talks about a
  payload the run could not name is a fact about this run — counts filled from the
  plan, never hard-coded — and no line calls an unclaimed payload recovered,
  recoverable or salvage. The hint names the missing or unreadable file, says that
  unmapped payloads are never written, and points at the surface that lists their
  paths (`adr-0006-degrade-without-guessing`).
- **An unclaimed payload's path is a field, not prose.** `manifest.json` carries a
  `payload` member for every item, `report.csv` carries `payload` as a trailing
  column after `note`, and the one-line-per-item view shows it where an image's
  relative output path would otherwise print `-`. A script triaging orphans must
  not have to parse an English sentence out of a note.
- **`missing-payload` names the missing thing, not a suspect.** Its note says which
  pyramid was declared and that no file of it is on disk, and quotes a
  `DeletedPyramid` timestamp when Lightroom recorded one. A stale level row inside
  a surviving pyramid says which level is stale and which level was used instead.
  Neither claims a cause.
- **`superseded` is one summary line, only when it is non-zero.** `superseded  3
  legacy .lrprev ignored; level files used`, mirrored as `superseded` in `--json`
  and `manifest.json` and never in `report.csv`, which stays per-image. It is a
  property of the cache, not of an item: a superseded container always coexists
  with the level files its pyramid was written from, so the number reports what
  was set aside, never a loss.
- **The summary counts images and names the floor actually applied.** The default
  summary assumes 640, so a caller that changed the floor calls
  `summary_with_floor`; a summary that reports a `below_floor` count for a floor
  nobody applied would be a lie about the run.
- **Machine output keeps a fixed shape.** Fields are separated by two spaces and
  an unknown fact prints as `-`, so the five columns stay put for a script;
  `--json` replaces the table with one object rather than mixing the two.
- **CSV escaping lives here once.** `escape_csv` is public so the run's per-file
  CSV is escaped by the same rules instead of growing a second dialect.
- **The contact sheet is recognition, not resolution.** Each image is shown with
  the *smallest* level in its pyramid, and the page is one self-contained HTML
  file with data URIs — a sheet that needs a folder of sidecar files would be a
  worse bug than a small picture.
- **A missing or unparseable payload makes the sheet skip the image**, not fail
  the run; the sheet is a convenience over a plan that has already been made.
- **The manifest carries a schema version** so a consumer can tell which facts a
  file is expected to contain.
- **`payload_metadata` is one token list, in a fixed order.** `exif,xmp,icc` for
  the segments the payload itself carried, empty when it is bare: a field on the
  item, an optional manifest member, and a trailing CSV column. `color_profile`
  stays the catalog's claim, so a reconstruction is never implied where the file
  carries the payload's own profile; the prose reasons live in the item's note,
  which is for humans and is never the field a fixture asserts against.
