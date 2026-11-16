# Changelog

All notable changes to `lrsalvage` are recorded in this file.

It follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). What it
promises is not SemVer's pre-1.0 licence:
[ADR 0010](https://github.com/marcd35/lrsalvage/blob/main/docs/adr/0010-what-0-2-promises.md)
names the surfaces a script can see, lets a minor add to them, and requires a new
minor saying so here before one is removed, renamed or given a different meaning.

## [Unreleased]

## [0.2.0]

0.2.0 is the first release a user can install rather than build —
[ADR 0010](https://github.com/marcd35/lrsalvage/blob/main/docs/adr/0010-what-0-2-promises.md)
calls 0.1.0 a tag on a tree — and it is `v0.1.0` plus the whole reader for the
preview layout Lightroom Classic 13.3 and later writes. For a 0.1.0 user that
means the modern layout, five `layout` words, `--json`, `--report`, the cache
counts and the companion rescue; for a new one, nothing here replaced anything.

**The release changes nothing on top of the tree it ships.** 0.2.0 is a tag on
today's tree, so the tree and the artifact are one: no status word, exit code,
flag, field, column or output name differs between them, and the surfaces
[ADR 0010](https://github.com/marcd35/lrsalvage/blob/main/docs/adr/0010-what-0-2-promises.md)
freezes are the ones this artifact already has.

**A cache that reported nothing under 0.1.0 may now recover.** 0.1.0 looked for
one `.lrprev` container per image, so a 13.3+ cache gave every image
`missing-payload` and exited 3. 0.2 decides the layout from the tree the cache
actually holds and discovers payloads from `previews.db`'s index instead of from
a container suffix, so the same cache reports `modern` and its images are
recovered (ADR 0004, ADR 0005;
[`format-notes`](https://github.com/marcd35/lrsalvage/blob/main/docs/format-notes.md)
§5).

**A JPEG this release writes differs from a 0.1.0 one in one place.** On a cache
0.1.0 could read, the payload's compressed bytes are copied verbatim by both and
the metadata around them is the set ADR 0003 records; the one difference is the
`Software` marker 0.2 writes into our EXIF segment, which 0.1.0 did not write at
all. Resume compares that marker's `lrsalvage ` prefix rather than the version
inside it, so a later release keeps the files an earlier one wrote — but it
cannot recognise a file from 0.1.0, which carries no marker to compare. The
first 0.2 run over a destination 0.1.0 filled therefore reports those files as
not ours and leaves them byte-identical; `--force` is the explicit way to replace
one, and leaving it is often right, because the file is already the recovered
photograph (ADR 0003).

### Added

- **Reading the caches Lightroom Classic 13.3 and later writes** — one
  extensionless JPEG per level and no `.lrprev` container. A pyramid is identified
  by the `(uuid, digest)` pair the index records, its levels are derived from the
  `PyramidLevel` rows and widened only inside that pyramid's own identity, and each
  payload is certified by its own frame and end-of-image marker before anything is
  written (ADR 0005;
  [`format-notes`](https://github.com/marcd35/lrsalvage/blob/main/docs/format-notes.md)
  §5.2–§5.4).
- **A `layout` word in every scan** — `legacy`, `modern`, `mixed`, `none` or
  `unknown`, decided from the tree and never from a version, in the text summary,
  `scan --json` and `manifest.json`. A cache whose files match no known shape
  reports `unknown` with one hint naming how many matched nothing, and a cache
  holding an index but no payload files reports `none`; both exit 3 rather than
  failing on a version check (ADR 0004).
- **`db_version` and `crs_version`** in `scan --json` and `manifest.json`, printed
  exactly as the catalog stores them: two observations about a file, never a
  mapping from a release name (ADR 0004).
- **A cache with no usable index degrades instead of stopping.** A missing or
  unreadable `previews.db` is a mode, not an error: every image whose original is
  not on disk reports `no-cache-entry`, every payload pyramid is listed as
  `unmapped` and never written, one hint names the file, and the run exits 3. A
  `--previews` path that is not a directory stays a usage error, exit 1, because a
  typo must not read as an empty cache (ADR 0006).
- **`scan --report <file>` writes the verification report** — one JSON object under
  the schema `lrsalvage.verification` carrying the layout, both catalog version
  strings, the options the run applied, its counts and the shape classes it saw,
  with every name a `<uuid…>`, `<digest>` or `<longedge>` template, so a run can be
  handed to somebody else without publishing a path. It obeys the destination rule,
  takes the next free numbered name instead of overwriting, is written whenever the
  flag is given and a plan exists, and never moves the exit code (ADR 0007).
- **Cache counts the run did not report before**: `superseded` pyramids held both
  ways, `deleted` missing-payload images the index has a `DeletedPyramid` row for,
  `unrecognised` payload-like files that match no shape, and `thumbnail_pyramids`,
  the second pyramid a render keeps. `shapes` counts the files by class and how
  many level files name the long edge their own frame measured. None of these is
  an image's status, and none moves an exit code: they describe the cache the run
  found (ADR 0006, ADR 0007, ADR 0008).
- **A `payload` column in `report.csv`, a `payload` member in `manifest.json`, and
  `payload_metadata`** — the file a preview was read from, and which metadata
  segments that payload already carried of its own. For an `unmapped` payload the
  path is the only identifier there is (ADR 0006; ADR 0003).
- **The companion rescue.** An image whose own pyramid holds no file anywhere is
  recovered from the thumbnail pyramid Lightroom rendered beside it, judged against
  that pyramid's own row, subject to the same floor, and written under the image's
  own name. Only a pyramid that meets every clause of ADR 0008 may rescue; any
  other sibling stays out (ADR 0009).
- **Every output names itself.** A recovered JPEG carries a `Software` marker,
  `lrsalvage <version>`, in its EXIF segment, so a later run can prove authorship
  rather than plausibility (ADR 0003).

### Changed

- **A payload's own metadata is kept where it is the stronger evidence.** An ICC
  profile the payload carries is copied verbatim and none is rebuilt from the
  catalog's profile name; its XMP packet is left byte-identical; its own
  orientation tag wins, because that value describes *these* pixels, and
  `orientation` in both machine surfaces is the value actually written. Our EXIF
  segment still supersedes the payload's, and the per-file note says which segments
  were kept and what was dropped (ADR 0003).
- **`unmapped` narrowed to genuine orphans**, and counts a pyramid rather than a
  file. A freshly rendered cache produces none: the second, thumbnail-sized pyramid
  each image carries is counted `thumbnail_pyramids` instead of listed, on its own
  summary line and in `--json`. An unmapped pyramid that does have a row now reports
  its `quality` and `color_profile`, so a report can say which kind of pyramid
  nobody claims (ADR 0008; ADR 0005).
- **`missing-payload` narrowed** to "no pyramid of this image holds a file", because
  a surviving companion is recovered rather than reported gone. A pyramid whose row
  is stale but which still holds a level is recovered at the largest level present,
  with a note (ADR 0009; ADR 0005).
- **Resume demands the marker.** A destination is skipped only when it carries
  `lrsalvage <version>`, is a complete JPEG and matches the planned dimensions. A
  same-size file another tool wrote is left byte-identical and reported failed;
  `--force` is the explicit way to replace it (ADR 0003).
- **`--no-metadata` output is outside verification.** It writes the payload's own
  bytes, so it carries no marker and can never be proven ours; a later run reports
  the destination as not a verified extraction and asks for `--force`. The flag's
  help says so rather than letting it be discovered (ADR 0003).
- **`scan` writes only what you name, and reports what it found.** A run that
  selected nothing but found payloads no image claims now writes `report.csv` and
  `manifest.json`, because that orphan list is the only thing such a run has to hand
  over; a dry run still writes none of them (ADR 0006, ADR 0007).
- **The minimum Rust version is now 1.89, up from 1.85** — `rust-version` in
  `Cargo.toml`, which `scripts/check.sh msrv` proves by building with that
  toolchain rather than restating the number.

### Fixed

- **A damaged preview no longer passes as success.** An item that failed
  certification was left out of the selection, so a run whose best levels were all
  broken printed `failed` in its summary and still exited 0. A failed item is now
  selected, evaluated and reported with its reason: it writes nothing and `extract`
  exits 2. A cache whose previews are all damaged therefore exits 2 with a reason
  per file, where 0.1.0 exited 3 as "nothing to do" (`specs/modules/plan.md`;
  `specs/goal-and-requirements.md` ¶5).
- **The index keyed a pyramid by uuid alone**, and read `PyramidLevel.digest`
  without using it, so one pyramid's level row could answer for a sibling of the
  same image whenever the two shared a long edge — a wrong `declared` size, a wrong
  `color_profile`, or `suspect` on a payload that was fine. Both maps key on the
  `(uuid, digest)` pair ADR 0005 names as the identity (ADR 0005).
- **A `previews.db` without a `DeletedPyramid` table is an index with no
  tombstones**, not a fatal error: a cache whose payloads are all readable used to
  exit 1 over one absent table (ADR 0006).
- **The false warning "this cache holds no previews" is gone.** No index is no
  *mapping*, not no previews, and the run now names the file it could not read and
  what it did about it (ADR 0006).
