---
id: adr-0008-a-render-companion-is-not-an-orphan
type: architecture-design
title: "ADR 0008: A render's second pyramid is not an orphan"
status: active
parent: architecture
references:
  - adr-0002-map-via-previews-db
  - adr-0005-discover-payloads-by-index
  - adr-0006-degrade-without-guessing
  - adr-0007-publish-a-verification-report
  - format-notes
---

# A render's second pyramid is not an orphan

**Status:** accepted · **Date:** 2026-11-15

## Context

`format-notes` §5.10 measured three caches rendered from scratch and found a shape
the reader had never been built for: **every image has two pyramids.** One is the
preview pyramid an `ImageCacheEntry` claims; the other is a `thumbnail` pyramid —
one 320 px level, `colorProfile sRGB`, a payload with no `Exif`, XMP or ICC
segment — that **no entry names**. The `Auto (3840px)` cache held 20 pyramids for
10 images, and 10 of them were unclaimed by design.

ADR 0005 ¶8 made every unclaimed `(uuid, digest)` one `unmapped` item, and issue
#42 found the consequences: `scan` prints `unmapped payloads 10` — the image count
— on a cache in perfect health, `extract` writes `report.csv` and `manifest.json`
for that cache because ADR 0006 ¶7 hands over what it found when nothing is
selected but orphans exist, and `scan --verbose` prints ten lines about payloads
nobody lost. The count stopped being a signal the moment every fresh render
produced it.

The measurement also narrows what a companion *is*: it is not "any second
pyramid". Its row says `thumbnail`; it sits beside a pyramid an image already
claims; and every level it holds measured 160 or 320 px long. The legacy catalog
of §2.7 has 21 `thumbnail` and 25 `bigThumbnail` rows and **two** pyramids more
than it has cache entries, and which qualities those two carry is still a TODO —
so a rule must be built to keep an unexplained pyramid visible rather than to
assume it is benign.

Two further facts constrain the design. Absence of an index is a mode (ADR 0006
¶1–¶2): with no rows, nothing distinguishes a companion from a genuine orphan, and
that mode's promise — every payload pyramid listed as `unmapped` — must stay
literally true. And a payload's size is the row's claim while the bytes are the
verdict (ADR 0001, ADR 0005 ¶5), so a row that says `thumbnail` over a 3840 px
file is a disagreement to report, never an explanation to accept.

## Decision

**An unclaimed pyramid that is a render's second pyramid is counted as a cache
fact, not listed as an orphan.** It is a **thumbnail pyramid** when all three hold:

1. **A claimed pyramid exists on the same uuid**, in the claim set built from
   live catalog images — so a uuid nothing maps keeps every pyramid it holds
   `unmapped`, tombstoned or not (ADR 0002, ADR 0006).
2. **Its `Pyramid` row's `quality` is exactly `thumbnail`** — no case folding, no
   inference from size, and `bigThumbnail` deliberately excluded until §2.7's two
   stray rows are measured.
3. **Every level it holds measures inside the `thumbnail` tier** (below 640 px), a
   fixed constant rather than `--min-long-edge`, so an option value cannot
   reclassify a cache's shape. A level whose frame cannot be read disqualifies the
   pyramid, and a `.lrprev`-only pyramid is never a companion: the measured state
   is a level-file render, and excluding containers leaves every legacy number in
   the docs true.

Everything else stays `unmapped`, with the row's `quality` and `color_profile`
filled in so a report can say *which kind* of pyramid nobody claims.

- **`unmapped` narrows to genuine orphans.** A companion is never an item, is
  never written, and never appears in `report.csv` or `manifest.json`; ADR 0006
  ¶7's "hand over what it found" path therefore stops firing on a healthy fresh
  cache.
- **The companion count is one cache fact.** `Plan::cache.thumbnail_pyramids`,
  one summary line when it is above zero, one top-level `scan --json` field, and
  the same README table row every `--json` field has. It is not a status, it never
  moves an exit code, and **the verification report, `report.csv` and
  `manifest.json` are unchanged** — ADR 0007's curated set stays frozen, and
  `shapes.level_file` already counts a companion's files.
- **ADR 0005 ¶8 and ADR 0006's no-index sentence are amended in place**, dated,
  as ADR 0006 amended ADR 0005 ¶7: the first said every unclaimed pyramid is an
  item, the second said every payload pyramid is `unmapped`.
- **The pair key is not a new decision.** ADR 0005 ¶1 already says a pyramid is
  identified by `(uuid, digest)`; issue #42 also found the index keyed by uuid
  alone and dropped `PyramidLevel.digest`, so one uuid's level rows supplied
  another pyramid's claim. That is a defect against ¶1, fixed in
  `previews_db::PyramidKey`, and it earns no ADR.

## Consequences

- A fresh render reports `unmapped 0` and `thumbnail pyramids N`, so the count
  means "nobody names this" again, and a script can tell a render's shape from a
  cache with a real orphan without reading the cache.
- A genuinely unclaimed pyramid — no row, another quality, a uuid with no live
  claim, an unreadable level, an oversized level, a `.lrprev` — stays visible,
  because each clause fails toward reporting.
- A lost image whose preview pyramid is gone is not rescued from its surviving
  companion: the 320 px payload is real, below the floor, and `unmapped` no longer
  names it. That is a deferral, not a discovery — it changes status and floor
  semantics, and carries its own fixture and its own "unmeasured in the field"
  label (§5.5).
- A `thumbnail` row that lies about its size is reported rather than trusted,
  which costs the reader one measured pass it already takes for
  `shapes.suffix_matches_sof`.
- The rule is stated once in `plan`'s `is_thumbnail_pyramid` and once in its spec;
  `previews_db` still decides no meaning, only which pyramid a row names.
- §5.10's `0.1.0` run rows keep their numbers — they are what that build printed —
  and a dated block records what the rule reports now, labelled as the fixture's
  evidence rather than as a re-run.

## Considered options

- **Keep listing companions as `unmapped`.** ADR 0005 ¶8 as written, zero code.
  But the count then equals the image count on every fresh render, `--verbose`
  narrates payloads nobody lost, and `extract` writes plan reports for a cache
  with nothing wrong.
- **Classify by `quality` alone.** One clause, row decides — and a stale or
  mislabelled row hides a large unclaimed payload. Rejected on ADR 0005 ¶5.
- **Classify by the floor option.** "Below `--min-long-edge` is not an orphan"
  needs no rows and behaves identically without an index, but `--min-long-edge 0`
  would switch ten companions back into orphans, and a lost photograph's only
  surviving payload would vanish from every report.
- **Give companions their own status.** Keeps each payload in `--verbose` and
  `report.csv`, but breaks the plan's "statuses are a partition over images" and
  touches `extract`, the CSV columns and the report schema to say that nothing
  happened.
- **Count them in the verification report.** A stranger's report would show the
  cache's age at a glance, at the cost of adding a field to a frozen public
  schema for a fact `shapes.level_file` already implies.
- **Prefer a larger unclaimed pyramid over the claimed one.** Maximum recovery,
  and exactly the nearest-neighbour mapping ADR 0002 forbids.
