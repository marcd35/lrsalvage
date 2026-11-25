---
id: app
type: module-design
title: "app: browser scan, review and export"
status: draft
parent: architecture
depends-on: [cache-recovery, catalog, previews-db, plan, snapshot, fs, error, extract, exif, model, report]
references: [adr-0012-two-release-lines, adr-0014-recover-legacy-containers-without-an-index, browser-spike, glossary]
---

# app

`crates/lrsalvage-app` — the shipped page (`0.1.0`, ADR 0012 D10) and the one
entry a run goes through. It is not a module of the pipeline: it owns no format,
no status and no naming rule, and every number it shows was observed by a module
with a spec of its own.

## Responsibility

Take a source — a folder holding a Lightroom catalog and its preview cache, and
any further folder a person hands over — and produce **one report**: the copy's
two digests, the index's condition, the cache's facts, one status per catalog
image, and the root folders the catalog recorded. Show it moving while it works,
let it be stopped, and say which of the ways a scan can come back empty happened.

The page is also the entry's *error surface*: a failure is a report
(`{ok: false, problem: {stage, kind, message}}`), never an exception, so a person
gets a sentence about the stage that failed and a page has one code path.

## Boundary

`src/lib.rs` asks `snapshot`, `catalog`, `previews_db` and `plan` in that order,
and nothing else:

```text
capture_verified ─► from_snapshot ─► index copy + open ─► build_reported ─► report
   (the copy)         (the open)        (the index)          (the scan)
```

- **The private copy is the snapshot.** `snapshot::capture_verified` carries the
  catalog and its `-wal`/`-shm` into the origin-private pool, opens it with
  `PRAGMA locking_mode=EXCLUSIVE` before the first WAL access and checkpoints —
  the measured 61 ms path that read 128 of 128 images where a checked import read
  78 (`browser-spike` §2). So `--verify-source`'s two digests are the same two
  digests the CLI reports, and the page prints them.
- **The index is copied where SQLite can open it.** The pool is the only VFS on
  this target, so `previews.db` is copied into the pool like the catalog is; an
  absent index is *not* copied, and `previews_db::open` answers `Missing`, which
  is a mode rather than a failure (`adr-0006-degrade-without-guessing`).
- **The scan has no writable destination.** A virtual output root gives the
  planner a namespace. The save flow validates the real destination handle
  separately and commits only the planner's relative paths.
- **The scan reports and stops** through `plan::Progress` and `plan::Control`
  (`specs/modules/plan.md`, *Decisions*). The page never re-derives a count.

`src/wasm.rs` is the only file that mentions JavaScript: it declares the two host
calls (the seam's `fs_call` and `scan_progress`) and exports `start_install`,
`poll_install`, `scan`, `scan_cache`, `scan_legacy`, and the review, export and
report calls the page makes.
`web/` holds the page and the seam's page-side JavaScript. `src/vault.rs` and
`src/bin/vault-app.rs` are native-only and compose
the vaults the browser run needs, from `lrsalvage-fixtures`.

`crates/lrsalvage-web` links this crate — harness depends on app, never the
reverse — so the seam is registered one way for both pages.

## Decisions

- **The entry unit is the catalog's folder, not the catalog file.** `FileSystemHandle`
  exposes a name and a kind and no path, so a picked *file* cannot reach its
  siblings: the `-wal` SQLite needs and the cache beside it are both unreachable
  from it. One `showDirectoryPicker` for the backup folder, from which the page
  reads `<name>.lrcat` and `<name> Previews.lrdata`; a split backup is a second
  folder pick. This corrects #117's "pick the catalog with a file picker", and the
  correction is forced by the API rather than by taste (`#149`, DEC-S1).
- **The suggested cache is the catalog's own previews folder, or nothing.** The
  folder listing collects every `.lrdata`, and the suggestion is the exact
  `<stem> Previews.lrdata` for the chosen catalog — never one of Adobe's companion
  folders (`<stem> Smart Previews.lrdata`, `<stem> Helper.lrdata`), which carry no
  `previews.db` and are not offered even when they are the only `.lrdata` there
  is. With no exact cache, a single remaining candidate is still offered, because
  a renamed or copied backup is a real case; anything more ambiguous suggests
  nothing and the step says the cache folder must be chosen. The rule is
  `web/folders.js`, one pure function, and a folder a person picks by hand answers
  to it too (`#191`).
- **The source is a value, and the picker is one way to make one.** `start(source)`
  takes mount paths and the handles that answer for them; the pickers, and the
  page's own reading of the folder, are what a click makes of them. That is what
  makes the flow testable at all: no automation we have can answer a File System
  Access dialog (`scripts/browser-app.sh`'s header records the measurement), so a
  browser test supplies the value a pick returns — an origin-private
  `FileSystemDirectoryHandle` — and drives everything after it for real.
- **A stop is memory, not a message.** The scan runs inside a dedicated worker
  behind a synchronous wasm frame, so a `postMessage` cannot reach it until it
  ends. The page writes a flag into a four-byte `SharedArrayBuffer` and the
  worker's `scan_progress` host call reads it with `Atomics.load` on every unit —
  the same mechanism the seam's bridge uses, for the same reason
  (`specs/modules/fs.md`, *The bridge*). A stop therefore lands on a unit
  boundary: one cache file, one level file, or one catalog image.
- **Progress is reported per unit and painted at the host's rate.** The Rust half
  calls `scan_progress` for every unit; the worker posts to the page at most ten
  times a second, and always on a stage change and on a stage's last unit. A
  100,000-image catalog crosses the boundary that many times and a DOM update does
  not cost what a `u64` does (`specs/modules/plan.md`).
- **Matching progress measures catalog images (#185).** For a known positive
  total the bar ranges from zero to that total. Matching shows the processed
  count and a percentage that reaches 100% on the last image and never before it:
  below completion the ceiling is 99%. The estimate is the average matching
  throughput in worker time, checked against the recent rate; *Calculating time
  remaining…* stands in during warm-up, after a stall, and while the recent rate
  differs from the average by more than a factor of two, so a revision upwards is
  the workload changing rather than the measuring window moving. An empty catalog
  says *No catalog images to match* with neither a bar nor an estimate. The last
  image's count is the end of the matching stage: one browser frame later the
  page clears the count and the estimate and says *Preparing scan results…* with
  the bar indeterminate, until the report arrives. The page reads that second
  boundary from the last stage's completion rather than from an event, because
  `Progress` reports no finalization (`specs/modules/plan.md`, *Decisions*). A
  stop clears the
  readout immediately and a report queued behind it cannot repaint it; a stop
  that arrives once the last image's count has been reported cannot land, so the
  completed result opens. The bar is named by the stage sentence and described by
  the count and the estimate, and one polite live region announces each stage,
  each matching quarter and the first estimate, and matching completion.
- **The stages that are one call are named, not pretended to move.** The copy, the
  index copy and the SQLite open have no unit inside them, so the page says which
  stage it is in and offers no stop for it. The measured copy is 6–10 s per GiB
  (`browser-spike` §2), which is why it is named.
- **Every condition is a count, and "nothing found" is never the message.** Absent
  index, unreadable index (with SQLite's own words), an index with no payload
  files, payload-like files matching no shape, images with no index row, rows with
  no payload, damaged payloads, payloads no image claims — each is one sentence
  with the number behind it, and "what was checked" is a list of the facts the
  report carries. A page that said "no previews found" for all of them would have
  failed #117's user stories 4 and 5.
- **The recheck mounts the folder where the catalog says it was.** A recorded root
  is `AgLibraryRootFolder.absolutePath`'s own spelling
  (`format-notes` §6.1: POSIX separators even on Windows), so a picked folder
  mounted at that path makes the seam's namespace answer the question
  `plan` already asks — is the original still there? — and the next scan reports
  those images as `original-present`. The app is stateless between runs, so the
  recheck pays one more copy; that is the trade for an entry that is one function
  in and one report out (`#149`, DEC-S7).
- **No framework or JavaScript bundler.** The page uses plain HTML, CSS and
  ES modules. The static build packages them with the compiled WebAssembly and
  practice files. Serve the page cross-origin isolated for the bridge's
  `SharedArrayBuffer`. The browser capability check directs users to the CLI
  when the folder picker is unavailable.
- **This slice's page is its own design; porting #117's prototype is a later
  decision, not an oversight.** #117 carries a prototype — one self-contained HTML
  file, recorded by ADR 0012 — and it was written for the **Windows executable**
  that the browser route replaced. Its shell (a titlebar, a three-step nav, a
  footer action bar), its two-row entry form ("Lightroom catalog" / "Preview
  folder · found beside catalog") and its copy for these screens were deliberately
  **not** carried into this page: the page was written from the user stories and
  from the counts the report carries, and the prototype lived outside this tree
  when the page was built, so no checkout could hold a port to it. Porting the shell and the copy is a design
  decision waiting for someone to make it — not a rewrite, because every string a
  person reads lives in `web/view.js` and nothing else in the slice depends on it:
  the browser run asserts the report, so its evidence stays valid either way. The
  prototype itself is in this tree as `docs/prototype/` (#149), so the port has a
  reference a checkout can open.

The scanning heading shows the chosen catalog or cache path relative to what
the browser exposes through the picked folder handle.

## Recover from preview cache

The start screen offers **Recover from preview cache** and asks for the cache
folder. The same action appears after an unreadable-catalog error, reusing the
selected cache when available. The [cache recovery module](modules/cache-recovery.md)
requires a supported index and plans one output per recoverable indexed pyramid.
The page shows separate missing, invalid, and unsupported counts, and gives a
supported index with zero entries its own empty result.

Review uses a flat list with dimensions and includes every recoverable preview
in the initial selection, including small ones. The user can deselect individual
previews. Before either save action the page explains that the JPEGs are previews
and that originals, original filenames and folders, and catalog metadata are
unavailable. Use the core's deterministic filenames and write the JPEGs byte
for byte. An occupied path counts as already recovered only when its bytes
match the selected preview. Both matching and conflicting files use the
destination conflict prompt.

Use the same destination checks and batch writer. Stop lets in-flight writes
settle, and Continue keeps the selection, paths, and destination fixed. The
receipt separates scan outcomes from the batch's saved, already recovered,
and failed counts and verifies the destination files against their planned
bytes in each folder the run used. The local report CSVs and manifest identify the cache-only mode, and the three published files come from the same encoders the catalog-backed run uses, under the same schema tags with `mode` naming the run ([report](modules/report.md)). The
shareable verification report and copied diagnostics use aggregate counts
without paths, filenames, or cache identities.

The indexed cache-only practice case shows a healthy pyramid, a smaller
surviving level, and a missing preview. The native and browser product checks
compare generated filenames, JPEG bytes, scan counts, and batch outcomes on
legacy, modern, and mixed layouts. The catalog-backed browser and CLI output
parity remains a release check.

## Recover from a cache without an index

[ADR 0014](../docs/adr/0014-recover-legacy-containers-without-an-index.md)
records the decisions behind this operation.

The start screen offers **Recover from a cache without an index…** as its own
choice (#198), beside the two that read an index. It asks for one cache folder
and nothing else: no catalog, no `previews.db`, no index copy and no open. A
run of the indexed mode that fails at `cache-index` (missing, unreadable, or
unsupported) and a catalog-backed scan that reports an index problem or cannot
copy the index each offer **Recover from this cache without an index** beside
the explanation. The catalog-backed scan still shows its existing result. The offer explains that
legacy recovery reads `.lrprev` containers. Focus starts on the result heading
and the button is reachable by keyboard. Choosing the button starts the legacy
scan with the handle already held by the page, or opens a folder picker when
that handle is unavailable. Any catalog folder already selected remains protected
by the destination checks.

The scan is the [cache recovery module](modules/cache-recovery.md)'s indexless
plan. The page reports one outcome per `.lrprev` container the walk found:
**usable**, **invalid**, or **unsupported**, and gives a folder holding no
container its own empty result with no save offered. Review is the same flat
list as the indexed mode, with every usable container checked; a candidate's
name is the core's generated one, and the page never invents one.

Everything the indexed mode does after review is shared unchanged: the
destination checks in both directions, the batch writer, Stop and Continue, the
destination conflict choices, the already-recovered outcome, the receipt
and the destination reconciliation. The three report files come from the same
encoders under a `legacy` mode word and the container counts, with
`report.csv`'s own frozen column set (`[report](modules/report.md)`).

## Preview review

- A completed scan retains the plan in its worker. The
  page receives one row per catalog image, including photos with no preview.
  IDs travel as strings so JavaScript preserves the full SQLite integer.
- Ready and small previews use the plan's default 640 px long-edge floor.
  The checked set starts with the ready previews. The **Resolution** dropdown
  lists each distinct JPEG width and height. A resolution starts visible if any
  preview at that size starts checked. Changes apply immediately, and **Show all
  resolutions** toggles all sizes. Photos without a preview remain eligible for
  display. Filtering leaves the checked set and batch selection intact.
  **Date captured** (catalog mode) uses the capture date from catalog metadata.
  **Preview file modified** uses the modification time of the file holding the
  selected JPEG, including its container when applicable. The first use of
  that sort requests the timestamps in batches from the completed review.
  Both sorts put newer
  dates first within the current groups, unknown dates last, and ties in
  filename order. Search, grouping, sorting,
  collapsing folders and switching views leave that set intact. Bulk inclusion
  applies to previews visible within the current filters and open groups.
  When grouped by original folder, the Previews folder checkbox includes or
  excludes all ready previews in that folder, including filtered-out rows.
  The checkbox shows a mixed state for a partial selection.
- Gallery and List share the same rows and selection. The optional Preview pane
  and enlarged dialog read one photo by its ID. The review allows two
  thumbnail reads at a time and drops queued tiles when they leave the
  viewport. A tile action brings the stacked pane into view on narrow screens.
- The worker calls `extract::read_preview` for the exact level the plan chose.
  When the payload has no EXIF orientation and the catalog records a rotation,
  review adds that orientation to a display copy. The compressed bytes and
  source files stay intact. The page shows the JPEG's raw dimensions and
  distinguishes index dimension disagreements, size changes since scan, read
  errors and browser decode errors. Fit contains the image inside the dialog. Actual size shows the
  decoded pixels at their natural CSS dimensions and makes them scrollable.
- The worker releases the SQLite pool when the scan closes its connections.
  Starting over or checking an original folder again ends the old worker's
  review session. Dismissing the enlarged view restores focus to the same
  photo or pane control when it can receive focus. Otherwise focus moves to
  search.
- The floor that left a preview unchecked is stated where those previews are: the
  small-previews section carries one sentence saying they are below the 640 px
  long edge and start unchecked so they can be included deliberately (`#192`). A
  folder the page could not look in says that in the same words — the page can
  only see a folder it was handed — rather than describing a connection.
- The save controls also sit above the gallery: one sticky bar carrying the
  checked count, the destination and the start or continue action, sharing one
  state with the panel below it, so scrolling the grid does not put saving out of
  reach.

## JPEG export

The shared planner reserves case-insensitive paths in catalog order before the
page projects its checked set. Both layouts keep paths stable across selections.
Mirrored folders are the default. Flat layout uses only the planned filename.
Previews below the default floor carry an _small suffix. Their ordinary names
stay reserved so the default mirrored batch retains the CLI paths. The planned byte total
measures the selected payloads before metadata injection, and every size the page
shows is decimal units to two places — `web/size.js`, one rounding rule for the
batch total and the receipt, because a binary reading of the same batch is a
different number from the one a disk reports. Origin storage estimates
describe browser storage and cannot measure the destination's disk space.

The page checks a picked destination against the source handles in both
directions. This excludes destinations inside or containing the catalog, preview
cache, or other mounted inputs. A refusal names the tree the folder landed in and
renders as its own block inside the export panel — the sentence and the button
that reopens the picker — with focus moved into it, so the way out is one press
away rather than a control to go looking for. The destination picker opens at the
catalog's folder, which is the handle the page already holds; a platform that
refuses to open a read-write picker there is retried once without it, since the
start location is a convenience and never a failure. The worker uses the shared extraction preparation
for every JPEG. The page commits the bytes through a writable file stream and
records success after close. Before writing, both save actions list occupied
planned paths and ask for **Overwrite**, **Skip**, or **Keep both**. **Skip** has
initial focus and leaves existing entries in place. An existing file counts as
already recovered only when it passes the mode's verification. An unverified
file skipped by choice is omitted. **Overwrite** replaces the listed files
after checking that they still match the contents shown for approval.
**Keep both** saves to the next available numbered name outside the paths
reserved by the batch. A directory at a planned file path cannot be
overwritten. The browser lock coordinates tabs of the app. Other applications
can change the destination independently. A one-off save uses the active
layout and leaves the checked set alone. Its destination
is suggested for the batch. A remembered handle requires another permission
request from a user gesture when reopened.

The batch fixes its layout, selected paths and destination when started. It
schedules at most eight files at a time. Its readout is a percentage from
completed writes, a bar at the same figure, and an estimate from observed
throughput using `web/rate.js`'s warm-up, steadiness and stall rules rather than a
second estimator; saved, already recovered and failed counts stay visible beside
it, and stopping is said in words rather than implied by a stopped bar. One polite
live region announces each quarter, completion and a stop request — never each
write, which on a 2,659-file batch would be thousands of announcements. Stop
leaves pending work unscheduled and
allows in-flight writes to settle. A new conflict stops scheduling until
in-flight writes settle, then asks again. Cancel before the first write
returns to review. Cancel at a later prompt keeps completed files and leaves
Continue available. Continue uses the same batch and committed paths. The page surfaces exceptions by relative output path and keeps successful
writes in the open run. A finished batch writes the reports the next section
lists into its destination. An occupied report name gets the next free number.

**The planned paths are a bounded, searchable dialog.** It opens with a summary —
count, layout, destination and how many paths were numbered to avoid a clash — and
renders at most two hundred matching paths inside a scroll region, saying so when
there are more. Search filters the planned rows. Closing it returns focus to the
button that opened it, which is the browser's own behaviour for a modal dialog.

## Receipt, reports and diagnostics

The receipt gives successful saves one headline with the count and the
destination folder name. It adds a breakdown for already recovered files,
files skipped by choice, save failures, or newly numbered files. A partial
batch states how many selected files were verified. It lists exceptions by
output path and explains when it cannot read the destination. **Show file list**
reveals saved files and verified existing files under their actual output names.
The receipt's diagnostic counts count each catalog image once across one-off
saves and batches.

A completed batch that saved at least one file and verified all selected files
shows a dialog such as **5 of 5 files recovered in 2.4 s**, with the destination
folder name. **OK** dismisses it and returns focus to the receipt. The time
covers active save work through destination verification. It excludes time
waiting for conflict choices, time between Stop and Continue, and report
writing. A batch consisting entirely of verified existing files shows its
result in the receipt.

The destination receives the CLI's three report files — `report.csv` from the
plan, `extract.csv` from the file outcomes, `manifest.json` from both — and each
takes the next free number when its name is occupied. The outcome CSV and
manifest report the numbered JPEG name used by **Keep both**. A user-skipped
unverified collision has the outcome `omitted`. The receipt links the files
it actually wrote, opening each from the bytes read back through the handle, and
reports one that failed to write as a failure rather than as a link. The
path-free verification report is offered separately, because it is the artifact a
person chooses to share: **Save verification report** writes `verification.json`
from the plan, the options the run applied and the copy's two digests — the field
set `specs/modules/report.md` freezes for `scan --report`. Its arguments have to
be true rather than convenient, so the entry retains the `Options` it planned
with and the digests `capture_verified` measured, instead of the copy's strings
alone. The plan CSV rides the report export a batch already calls; the
verification report and the compiled version pair are the two new worker exports
this needs.

Diagnostics are kept in memory for the current run, never written into a
destination, never uploaded, and copied only by a click. One serializer produces
them for the receipt and for a failure view, so a copy is available before any
receipt exists — the log starts at page load, which is what makes a failure at
the catalog copy shareable. The object is:

```json
{ "schema": "lrsalvage.diagnostics",
  "mode": "catalog",
  "versions": {"app": "0.1.0", "core": "0.4.0", "build": "4606440a1b2c",
               "browser": "Google Chrome 133.0.6943.99", "platform": "Windows", "platform_version": "15.0.0"},
  "timings": {"storage_setup_ms": 812, "catalog_copy_ms": 2140, "scan_ms": 9100,
              "first_thumbnail_ms": 9310, "first_saved_jpeg_ms": 12300,
              "longest_write_gap_ms": 74, "elapsed_ms": 15600},
  "scan": {"images": 5000, "recovered": 4998, "suspect": 0, "missing_payload": 2,
           "no_cache_entry": 0, "unmapped": 0, "failed": 0, "original_present": 0,
           "below_floor": 2, "above_cap": 0, "bytes": 918273645, "levels": 5002,
           "selected_items": 120, "selected_images": 120,
           "tiers": {"full": 120, "large": 3800, "standard": 1078, "small": 2, "thumbnail": 0},
           "superseded": 0, "deleted": 0, "unrecognised": 0},
  "cache_scan": {"indexed": null, "recoverable": null, "missing": null,
                 "invalid": null, "unsupported": null},
  "writes": {"planned": 120, "written": 118, "already_recovered": 2, "failed": 0,
             "confirmed_on_disk": 120},
  "events": {"stopped": 1, "settled": 1, "continued": 1},
  "failures": {"destination_gone": 0, "permission": 0, "out_of_room": 0,
               "occupied_foreign": 1, "source_unreadable": 0, "write_failed": 0, "other": 0},
  "failure": null }
```

The field list is frozen here. `mode` names the run that produced the copy —
`catalog`, `cache-only` or `legacy` — so a reader knows which scan object to
read. `versions` carries the two release numbers, the
checkout identity when a built page has one, and the browser and platform as the
browser states them — a table mapping a platform version to a marketing name is a
table that rots. `timings` are milliseconds since the page loaded and there is no
absolute timestamp: the object describes a run's shape, not when its author was at
their desk. `scan` carries exactly the eighteen `stats` members
`specs/modules/report.md` freezes, so no second vocabulary is invented for a count
the reports already name. A cache-only run measured none of those, so its `scan`
is `null` and `cache_scan` carries the five counts it did measure — the same five
names `specs/modules/report.md` freezes for that mode, never a cache count under a
catalog name. An indexless run's `scan` is `null` too. An indexless run's
`cache_scan` carries `containers`, `usable`, `invalid` and `unsupported` — the
four names `specs/modules/report.md` freezes for that mode, never a translation of
another mode's. On a catalog copy `cache_scan` is the five members at `null`,
because a fact that run did not measure is `null`, never absent.
`longest_write_gap_ms` measures consecutive commits
across the whole run, so a stop appears in `events` and never as a slow write.
`catalog_copy_ms` covers the copy, the open and the index copy, because one
synchronous wasm frame owns all three and no clock runs inside it; it ends at the
first unit the plan reports, and `scan_ms` starts there. `failure` is a stage and
one of the entry's own kinds — never the error's message, which can name a path —
and is `null` for a run that got as far as a file. A fact the run did not measure
is `null`, never `""` and never absent: every top-level member above is on every
copy, and the names inside `cache_scan` are the ones the mode that produced the
copy measured.

The list is curated at its source rather than filtered afterwards: the serializer
has no field a path, filename, identifier or message could occupy, which is what
leaves the page nothing to redact and nobody to remember what to remove — ADR
0007's discipline applied to the second artifact that leaves a user's machine.

## Practice and distribution

The guide opens the recovery page with a named practice case. A same-origin
manifest identifies the bundled files and their byte revision. The app
installs those files in a practice-only area of origin-private storage, then
hands that directory to the same folder inspection and scan as a picked
backup. A lock serializes installation of each revision. Once installed, its
files stay available to other tabs using the same sample.

The shared fixture builder generates the catalog-backed and cache-only samples
from synthetic data and redistributable JPEGs. For each sample, packaging uses
one set of files for the ZIP and for the in-app route. The native app scan checks
each case during packaging.

The legacy practice sample starts in indexed cache-only recovery, which reports
the missing index. The user can then choose legacy recovery. Its ZIP contains the
same four containers with no catalog or index. To rehearse the handoff from the
ZIP, extract it and pick the sample folder with **Recover from preview cache**.
See the [fixture spec](fixtures.md#legacy-app-recovery-without-an-index) for the
container composition.

[The static builder](../scripts/build-browser-app.py) packages tracked runtime
assets, builds the WebAssembly module, and writes the app and core versions and
checkout identity. [The deployment runbook](../docs/agents/browser-app-deployment.md)
records how the same archived bytes reach Cloudflare Pages and how to check
the deployed candidate.

## Testing

- `crates/lrsalvage-app/tests/scan.rs` is the gate: one report per condition over
  the seven vaults, a stopped build with no plan, and an indexless cache still
  counting its payloads. It composes `lrsalvage-fixtures` vaults through
  `src/vault.rs`, so no fixture bytes live in this crate.
- `crates/lrsalvage-app/tests/legacy.rs` is the third mode's gate: the report's
  mode word and container counts, one JPEG per usable container exported byte for
  byte, the published files' `legacy` mode, an empty folder with no save offered,
  and a stopped scan that holds no plan.
- The browser app workflow runs `scripts/browser-app.sh` on one headless
  Chromium runner for relevant pull requests. It compares the page's entry
  and native results field by field, including digests, and covers stop,
  recheck, picker cancellation and the capability check. The cache suggestion is
  asserted over a real directory holding a catalog with all three `.lrdata`
  siblings, so the page's reading of the folder is held to the rule and not only
  the rule to itself (`#191`).
- `web/folders.js` is the cache-suggestion rule in one pure function, and
  `scripts/check-js.mjs` runs it on made-up name lists in milliseconds: the
  companion folders rejected, the exact cache winning between same-stem
  siblings, one renamed candidate still offered, and two ambiguous ones
  suggesting nothing.
- `web/size.js` is the one size formatter (`#192`), and the same gate runs it on
  the boundaries: bytes below a kilobyte, each unit's two places, and a number
  the page could not have never reaching a person as `NaN`.
- What the browser run cannot prove, and does not claim: that a human's accepted
  pick returns a handle. The mechanism is the browser's, one `await` deep, and the
  value it returns is the value the test supplies.
- The same script drives the actual product page for #167. The test installs
  an origin-private directory handle at the picker boundary, clicks the app's
  controls, and compares the visible rows, counts, decoded JPEG dimensions,
  error messages, keyboard navigation, and focus. The review fixture uses
  deliberately generated raster patterns and a structural JPEG that Chromium
  cannot decode. An accepted native dialog, the experience on a physical small
  laptop and operation with a screen reader remain manual checks.

- The browser gate also drives one-off and batch saves through the product,
  exercising path collisions, selection independence, stop and continuation,
  directory loss, and overlap with input trees. It compares the default mirrored
  export against the CLI on identical fixture bytes. The JPEG tree and each JPEG
  byte must agree. In reports the check maps the sandbox output and cache roots
  to their browser mount paths and ignores the generation timestamp. The embedded
  core version is the same in both builds, so the JPEG marker, hash and manifest
  version are compared exactly. The same run asserts the surfaces #192 added: the
  save bar agreeing with the panel below it, the planned-path dialog's summary,
  its search and its row cap, and the batch readout reaching 100% with the
  milestone announced.
- `scripts/check-legacy.mjs` is the indexless mode's phase of that script
  (#198). It drives the third start-screen choice over five synthetic caches —
  multiple sizes, an equal-area tie, a usable prefix before damage, a damaged and
  an unsupported container, an empty folder, and containers outside the shard
  folders — and compares the saved names and bytes with the native
  `scan_legacy` run over the same fixture. It asserts the destination refusal, a
  stopped and continued batch, the already-recovered and conflict outcomes, the
  empty result offering no save, the missing-index hand-off into this mode, and
  an unreadable folder reported as an inventory error rather than a partial
  list. The published files and copied diagnostics are read back and held to
  `legacy` and the four container counts. The browser gate also checks both
  catalog-backed index findings, a refused index copy, and all three indexed
  cache-only failures, then
  exercises the legacy handoff without a second source pick.
- The packaged product check follows the guide's sample link through the error,
  the handoff, and two saves to the same destination. It checks selected
  dimensions, generated names, JPEG bytes, and the reconciled receipt. The
  artifact verifier compares the ZIP with the in-app sample files.
- `scripts/check-scan-progress.mjs` is the scanning readout's phase of that
  script (`#185`). It drives the page's own progress module with made-up reports
  — the range and its maximum, the 99% ceiling, the estimate's warm-up and its
  refusal to project across a stalled or a more-than-halved rate, the empty
  catalog, the completion frame before result preparation, and a stop that a
  queued report cannot repaint — and reads the accessible progress bar's name,
  value and maximum from Chromium's accessibility tree. The estimate's
  arithmetic is `web/rate.js`, which `scripts/check-js.mjs` also runs directly in
  milliseconds; `scripts/scan-progress-trace.mjs` replays a recorded run through
  it, so the property that matters — the displayed estimate not climbing while
  the rate holds — is checkable from a trace rather than only from a watch. The
  whole 5,000-image
  scan, which takes minutes, is release evidence rather than a gate: its protocol
  is `docs/agents/browser-app-deployment.md`, *Verify matching progress at
  release scale*.
- `scripts/check-receipt.mjs` is the receipt's own phase of that script. It
drives the product through a batch that is stopped and continued with a one-off
save overlapping it, then lists the destination back and compares the three
report files and the offered verification report with the CLI's on the same
vault. It reads the clipboard after granting clipboard permission over DevTools
and asserts the exact field list of the copied diagnostics plus the absence of
every synthetic path, filename and uuid in it — once from the receipt and once
from a failure that happened before any receipt existed.
