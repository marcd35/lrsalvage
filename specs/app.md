---
id: app
type: module-design
title: "app: browser scan, review and export"
status: draft
parent: architecture
depends-on: [cache-recovery, catalog, previews-db, plan, snapshot, fs, error, extract, exif, model, report]
references: [adr-0012-two-release-lines, browser-spike, glossary]
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
`poll_install`, `scan`, and the review, export and report calls the page makes.
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
match the selected preview. A mismatch leaves the file in place and counts
as a conflict.

Use the same destination checks and batch writer. Stop lets in-flight writes
settle, and Continue keeps the selection, paths, and destination fixed. The
receipt separates scan outcomes from the batch's saved, already recovered,
and failed counts and verifies the destination files against their planned
bytes in each folder the run used. The local report CSVs and manifest identify the cache-only mode. The
shareable verification report and copied diagnostics use aggregate counts
without paths, filenames, or cache identities.

The downloadable and in-app practice case contains only a synthetic cache.
It shows a healthy pyramid, a smaller surviving level, and a missing preview.
The native and browser product checks compare generated filenames, JPEG bytes,
scan counts, and batch outcomes on legacy, modern, and mixed layouts. The
catalog-backed browser and CLI output parity remains a release check.

## Preview review

- A completed scan retains the plan in its worker. The
  page receives one row per catalog image, including photos with no preview.
  IDs travel as strings so JavaScript preserves the full SQLite integer.
- Ready and small previews use the plan's default 640 px long-edge floor.
  The checked set starts with the ready previews. Search, grouping, sorting,
  collapsing folders and switching views leave that set intact. Bulk inclusion
  applies to previews visible within the current filters and open groups.
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

## JPEG export

The shared planner reserves case-insensitive paths in catalog order before the
page projects its checked set. Both layouts keep paths stable across selections.
Mirrored folders are the default. Flat layout uses only the planned filename.
Previews below the default floor carry an _small suffix. Their ordinary names
stay reserved so the default mirrored batch retains the CLI paths. The planned byte total
measures the selected payloads before metadata injection. Origin storage estimates
describe browser storage and cannot measure the destination's disk space.

The page checks a picked destination against the source handles in both
directions. This excludes destinations inside or containing the catalog, preview
cache, or other mounted inputs. The worker uses the shared extraction preparation
for every JPEG. The page commits the bytes through a writable file stream and
records success after close. The app verifies existing JPEGs with the same marker and dimensions check as
native extraction. It reports matching files as already recovered. The app refuses foreign files it
finds at a planned path. The browser lock coordinates tabs of the app. Other
applications can change the destination independently. A one-off
save uses the active layout and leaves the checked set alone. Its destination
is suggested for the batch. A remembered handle requires another permission
request from a user gesture when reopened.

The batch fixes its layout, selected paths and destination when started. It
schedules at most eight files at a time. Stop leaves pending work unscheduled and
allows in-flight writes to settle. Continue uses the same batch and committed
paths. The page surfaces exceptions by relative output path and keeps successful
writes in the open run. A finished batch writes the reports the next section
lists into its destination. An occupied report name gets the next free number.

## Receipt, reports and diagnostics

The receipt is a section of the result screen rather than a stage of its own: it
appears when the first file commits and stays until another recovery starts, so a
run that saved one photo ends with the same account a batch does, and a stopped
batch shows its receipt with the continuation beside it. It leads with the count
of catalog images the run **saved** — one per image, however many times a one-off
save and a batch reached it — and names beneath it the images already recovered
at their planned paths, the images it could not save, and how many files the
folder answered for when it was read back. The count is taken from commits: a
file counts when its write has closed, never when it was planned. Both version
numbers a bug report quotes sit on the receipt, in ADR 0012 D2's form. When a
batch finishes the receipt's heading takes focus, and it keeps the page's focus
ring instead of suppressing it: the result title behaves the same way, a
programmatic target with no indicator is worse than a firm one, and changing both
is one piece of work rather than half of it (#169, test drive).

Reconciliation reads the destination instead of trusting the run. When a batch
settles the page lists the destination back through the same handle and compares
the committed paths with what is there, naming a planned path that is missing and
a foreign file where a plan expected one; a read that cannot be finished — a
disconnected folder, a lost grant — says so instead of claiming the files.
**View saved files** shows that listing: the files this run saved or found
already recovered, grouped by the catalog folder they came from, with names and
byte sizes. A directory handle carries a name and nothing else, so the page
cannot learn a folder's path or open one in Explorer; the receipt names the
chosen folder, offers the name for copying, and explains how to find it there.
The prototype's *Open folder* has no page equivalent and the page does not claim
one.

The destination receives the CLI's three report files — `report.csv` from the
plan, `extract.csv` from the file outcomes, `manifest.json` from both — and each
takes the next free number when its name is occupied. The receipt links the files
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
  "writes": {"planned": 120, "written": 118, "already_recovered": 2, "failed": 0,
             "confirmed_on_disk": 120},
  "events": {"stopped": 1, "settled": 1, "continued": 1},
  "failures": {"destination_gone": 0, "permission": 0, "out_of_room": 0,
               "occupied_foreign": 1, "source_unreadable": 0, "write_failed": 0, "other": 0},
  "failure": null }
```

The field list is frozen here. `versions` carries the two release numbers, the
checkout identity when a built page has one, and the browser and platform as the
browser states them — a table mapping a platform version to a marketing name is a
table that rots. `timings` are milliseconds since the page loaded and there is no
absolute timestamp: the object describes a run's shape, not when its author was at
their desk. `scan` carries exactly the eighteen `stats` members
`specs/modules/report.md` freezes, so no second vocabulary is invented for a count
the reports already name. `longest_write_gap_ms` measures consecutive commits
across the whole run, so a stop appears in `events` and never as a slow write.
`catalog_copy_ms` covers the copy, the open and the index copy, because one
synchronous wasm frame owns all three and no clock runs inside it; it ends at the
first unit the plan reports, and `scan_ms` starts there. `failure` is a stage and
one of the entry's own kinds — never the error's message, which can name a path —
and is `null` for a run that got as far as a file. A fact the run did not measure
is `null`, never `""` and never absent, so the same members are on every copy.

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

The shared fixture builder generates the four catalog-backed downloadable ZIPs from
synthetic catalogs and redistributable JPEGs. The ZIP and in-app route for each
case use the same catalog and cache. The native app scan checks each case
during packaging.

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
- The browser app workflow runs `scripts/browser-app.sh` on one headless
  Chromium runner for relevant pull requests. It compares the page's entry
  and native results field by field, including digests, and covers stop,
  recheck, picker cancellation and the capability check.
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
  version are compared exactly.
- `scripts/check-scan-progress.mjs` is the scanning readout's phase of that
  script (`#185`). It drives the page's own progress module with made-up reports
  — the range and its maximum, the 99% ceiling, the estimate's warm-up and its
  refusal to project across a stalled or a more-than-halved rate, the empty
  catalog, the completion frame before result preparation, and a stop that a
  queued report cannot repaint — and reads the accessible progress bar's name,
  value and maximum from Chromium's accessibility tree. The whole 5,000-image
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
