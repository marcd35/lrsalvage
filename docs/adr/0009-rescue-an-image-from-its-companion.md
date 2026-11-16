---
id: adr-0009-rescue-an-image-from-its-companion
type: architecture-design
title: "ADR 0009: Rescue an image from its render companion"
status: active
parent: architecture
references:
  - adr-0002-map-via-previews-db
  - adr-0005-discover-payloads-by-index
  - adr-0006-degrade-without-guessing
  - adr-0007-publish-a-verification-report
  - adr-0008-a-render-companion-is-not-an-orphan
  - format-notes
---

# Rescue an image from its render companion

**Status:** accepted · **Date:** 2026-11-16

## Context

ADR 0008 settled what a render's second pyramid is *when the image is fine*: an
unclaimed `thumbnail` pyramid beside the one an entry claims is a cache fact
(`thumbnail_pyramids`), never an orphan, never an item. That decision left an
asymmetry at the other end of the same cache.

An image whose claimed pyramid holds only a 101 px level next to a stale 1616 row
is `recovered` at 101 px, tier `thumbnail`, counted `below_floor` (fixture `M12`,
which is a measured state, §5.7). An image whose claimed pyramid has **no file at
all** — because 14.0+ pruned it, or the cache was partially deleted — while its own
companion still holds the 320 px render, is `missing-payload`. Same bytes on disk,
opposite verdict, and the item says a payload is gone when one is right there.
ADR 0005 ¶5 wrote that rule when a pyramid was the only thing an image could have;
§5.10 then showed that a render always gives the image two.

What is known and what is not: §5.10 measured the pair (same uuid, `quality
thumbnail`, one small level, sRGB, bare payload) on three caches rendered from
scratch. §5.5 records that a prune having *reached the whole pyramid* is
unmeasured in the field — so this is a fixture state with an honest `unmeasured`
label, not a claim that Lightroom leaves it often.

**Measured 2026-11-16, and it narrows the case rather than widening it.** The
deletion this build can trigger — `Library > Previews > Discard Standard and 1:1
Previews` — was run on a copy of the §5.10 cache (§5.11). It removes the
standard-size level *and* the image's `thumbnail` pyramid, keeps the levels below
the standard size, and leaves the index agreeing with the files. No deletion
measured here produces this ADR's state, and the background purge is reported to
exempt thumbnails and sub-standard levels as well. The rescue therefore guards a
cache that was partially deleted, a file the user removed, or a prune nobody has
caught in the act — not an observed Lightroom behaviour.

## Decision

**When the pyramid a cache entry names has no file anywhere, the plan reads the
image's own render companion instead of reporting the payload gone.**

1. **Only a classified companion may rescue.** The three clauses of ADR 0008, with
   no widening: a claimed pyramid on the same uuid, `quality` exactly `thumbnail`,
   and every level it holds inside the `thumbnail` tier. That is the sibling §5.10
   measured a render writing beside the preview one, so the rescue uses an
   observed relationship rather than inferring a mapping from a shared uuid —
   ADR 0002 is intact. A `bigThumbnail` sibling, a `final` one, an oversized
   level or a uuid nothing maps stays out; each is a state no measurement covers.
2. **The companion's own row is the claim.** Its largest level is ranked and
   certified exactly as a claimed pyramid's is, so `suspect` stays the honest
   verdict on a size disagreement, and the item reports the companion's
   `quality` and `color_profile` — `thumbnail` and `sRGB` — because that is the
   row the bytes came from. The note says the payload came from the image's
   render companion, in words and without a path
   (`adr-0007-publish-a-verification-report`).
3. **`missing-payload` narrows to "no pyramid of this image holds a file".** If
   the claimed pyramid has *some* file, nothing changes: the plan recovers the
   largest level present and notes the stale row. The fallback runs only when
   there is no file at all.
4. **The floor still applies.** A rescued 320 px payload is `recovered`,
   `below_floor` and unselected; `--min-long-edge 0` writes it. One rule for a
   small payload, whichever pyramid held it (M12), and the floor keeps its promise
   that nothing under 640 px is written without being asked.
5. **`thumbnail_pyramids` is unchanged by the rescue.** The count is a property of
   the cache (ADR 0008), not of what the plan did with one pyramid; a used
   companion is still counted, and the image's item is where the use shows up.

ADR 0005 ¶5 is amended in place with a dated line. There is no flag: a second way
to answer "what would extract write" was rejected for ADR 0006 and is rejected
here on the same grounds.

## Consequences

- A lost photograph whose preview pyramid is gone but whose companion survives is
  no longer reported as unrecoverable. It is recovered at 320 px, counted below
  the floor, and written on request — the last copy, under the image's own name,
  with the metadata treatment any other payload gets.
- `missing-payload` means what it says again. A cache that reports it has nothing
  left to offer for that image.
- The state's frequency is unknown (§5.5): the rule is tested by fixtures `M10f`
  and `M10g`, and the notes label it `unmeasured` rather than implying a prune
  leaves companions behind often.
- A companion used as a payload is both counted and used. That reads oddly for one
  row of a report and correctly everywhere else: the cache does hold a second
  pyramid, and the plan did read it.
- No new exit code, no new status, no new output namespace, and the contact sheet
  needs nothing: it already asks the item for its smallest level.
- `bigThumbnail` is a quality a **claimed** pyramid can carry: a discard rewrites
  `standard` to it when the top level goes (§5.11). The sibling rule above keeps
  comparing against `thumbnail` exactly, which is why that value still cannot
  rescue an image.

## Considered options

- **Leave it and document it.** Cheapest, and ADR 0005 ¶5 as written. But the
  tool then says "no payload" about a payload it counted one decision earlier.
- **Note only, no recovery.** Keep `missing-payload` and name the surviving
  companion in the note. Honest, but the user still gets no file from the only
  copy that exists.
- **Rescue from any unclaimed pyramid on the uuid.** Maximum recovery, and a
  mapping invented from a shared uuid: unmeasured, and exactly what ADR 0002
  exists to prevent.
- **Exempt the rescue from the floor.** A surviving companion is the last copy,
  so write it regardless. That makes a 160 px render write itself, changes what
  "nothing below 640 is written" promises, and turns exit 3 into 0 for caches
  where the user asked for no such thing.
- **A `--rescue-thumbnails` flag.** Default stays byte-identical, at the cost of
  two answers to one question and a default that keeps the pessimistic wrong
  answer.
- **Write rescued companions under `_unmapped/<uuid>-<digest>_<n>.jpg`.** The
  namespace ADR 0006 ruled out, for the same reason: a name is not a mapping.
