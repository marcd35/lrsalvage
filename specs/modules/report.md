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
