---
id: adr-0013-a-cap-demotes-the-level
type: architecture-design
title: "ADR 0013: A cap demotes the level, it does not exclude the image"
status: active
parent: architecture
references:
  - adr-0005-discover-payloads-by-index
  - adr-0008-a-render-companion-is-not-an-orphan
  - adr-0009-rescue-an-image-from-its-companion
  - adr-0010-what-0-2-promises
  - format-notes
  - glossary
---

# A cap demotes the level, it does not exclude the image

**Status:** accepted · **Date:** 2026-11-16 · **Amended:** 2026-11-18, renumbered
to 0013 when 0.3.0 shipped without it and 0011 and 0012 were both taken.

## Context

`--min-long-edge` is the only thing that decides whether a level is worth writing,
and it says one thing: nothing smaller than N. A cache that built 1:1 previews has
no such symmetry. Its levels are a doubling ladder ending at each original's own
long edge (§5.7), so `extract` writes a 5,760 px JPEG for every image even when
what the user wants is the 1,920 that is sitting in the same pyramid. Issue #67
recorded declining every filter for 0.2 and named `--max-long-edge` as the only
one worth reconsidering first: symmetric with an existing flag, and no naming
question to settle.

The symmetry is less obvious than it looks, for two reasons.

The plan ranks levels and takes the largest as the winner; the floor is applied to
that winner *afterwards*, and nothing in the plan can fall back to a smaller level
(`plan`). A cap written as the floor's mirror — take the winner, exclude it if it
is too big — therefore writes nothing for an image whose 1,920 level is on disk
beside the 5,760 one, which is the case the flag exists for.

And two places choose a level a run actually writes, not one: level files rank by
each candidate's own row, and a container ranks its decoded levels. The render
companion's rescue goes through the first of them, because a companion's levels
are level files too (`adr-0009-rescue-an-image-from-its-companion`); the third
rank, `largest_level`, only names an orphan's payload, which no run ever writes.
A bound honoured in one of the two is a second rule a reader has to be told
about.

## Decision

**A cap chooses a level. It never turns an image into nothing.**

The winner is the largest level present that measures at or below the cap. A
level above it is still an item — `selected: false`, counted as `above_cap`,
unwritten — exactly as a level below the floor is an item today; `--all-levels`
makes every level above the cap an item as well, and without it the cap is
invisible unless it binds, which is how the floor already behaves. An image all of
whose levels exceed the cap is not `failed` and has no new status: it recovers
nothing, the way an image whose only level is below the floor recovers nothing,
and the existing rule already names that run (`3` when nothing at all was
selected).

The cap is applied wherever a level is chosen to be written — the level-file rank
(and so the companion rescue that reuses it) and the container rank — and
`--max-long-edge 0` disables it as `--min-long-edge 0` disables the floor. A cap
below the floor is a legal empty window rather than a usage error: nothing is
selected and the run exits `3`, which is what `--min-long-edge 99999` already
answers (fixture matrix, `fixtures`).

`above_cap` mirrors `below_floor` on every surface that carries it — the terminal
summary, `scan --json`, and the verification report's `stats` — and the report's
`options` echo gains `max_long_edge`. It does not reach `manifest.json`, which
omits `below_floor` for the same reason.

The word is *cap*, not *ceiling*: the README already tells a reader that 1440 × 960
"is the ceiling unless 1:1 previews were built", and a second meaning for one word
in the one file that exists to prevent that is a cost with no benefit. The count
is `above_cap`, beside `below_floor`.

## Consequences

- A container's capped level is judged against the row that declares *its* size,
  not against the pyramid's best row. Keeping the old claim would flag every
  demoted container chunk `suspect` — a false verdict the feature would have
  introduced rather than one found in a cache.
- `plan`'s load-bearing sentence changes. "The floor excludes, it does not fail,
  and nothing else excludes" becomes a statement about two bounds, and "the chosen
  level is the largest one present" becomes the largest one present that clears
  the cap.
- `Options` gains a field that five literal constructions name, and
  `report::summary` stops hard-coding 640 as a default wrapper around
  `summary_with_floor`: the summary takes the run's options, because a summary
  that reported a bound nobody applied would be the same lie the `report` spec
  already forbids for a floor.
- The verification report's frozen list of seventeen `stats` names becomes
  eighteen. That is a decision recorded here, not a field that appeared.
- `adr-0008`'s rule that an option value never reclassifies a cache's shape
  survives untouched: `is_thumbnail_pyramid` keeps its fixed 640 constant, so a
  cap cannot switch a companion back into an orphan.
- Nothing else excludes: a `failed` level is still an item, and an above-cap level
  is now one too, so a cache whose only payload is oversized exits `3` with the
  bytes named rather than reported as an empty cache.
- This is additive surface under `adr-0010`, which is why it is a **0.4.0** minor.
  `0.3.0` shipped without it, as its own records describe, and
  `adr-0012-two-release-lines` D7 has #67 close when this merges rather than move
  to a milestone, and it is already closed: what it declined stays on the issue.

## Considered options

- **Exclude the winner and write nothing.** The floor's exact mirror, and the
  cheapest code: one comparison after the ranking, no change to any ranking path.
  It writes no JPEG for an image whose 1,920 level is on disk and readable — the
  commonest instance of the need — and it would make `--max-long-edge` a flag that
  converts a full recovery into an exit 3.
- **Two flags**: `--max-long-edge` demotes, a second flag excludes on purpose. Two
  contract rows, two help texts, and a README paragraph explaining which to use
  for a distinction the user can act on by deleting the smaller output.
- **Cap by tier name** (`--max-tier standard`). It reads better than a number and
  binds the flag to five boundaries the glossary defines for *expectations*, which
  the tier table says "do not change what is in the cache" — and a user with a
  2,000 px original would have no way to ask for its own size.
- **Exempt the rescue path** because a companion's level is usually a 320
  thumbnail the cap cannot bind. Cheap, and wrong for the reason `adr-0009` gives
  for applying the floor to a rescued level: one rule for a level, whichever
  pyramid held it. Exempting it would buy almost nothing — a classified
  companion's every level measures under 640 px, so no cap at or above the
  default floor reaches one — while a floor lowered with `--min-long-edge 0`
  leaves a cap that does bind, and the exemption would then be a second rule for
  the one case it was written for.
