---
id: versions
type: architecture-design
title: "What the versions mean"
status: active
parent: architecture
tags:
  - reference
---

# What the versions mean

This project ships **two artifacts that carry version numbers**, and they move
independently. [ADR 0012](adr/0012-two-release-lines.md) decides that; this page
explains it to the three people who need it — someone deciding what to download,
someone reporting a problem, and someone changing the code.

## The two lines

| | **Core line** | **App line** |
|---|---|---|
| What it ships | `lrsalvage-core` on crates.io, and `lrsalvage-cli` as a binary plus three platform archives | **the browser app**, a static page deployed per app tag. [ADR 0012](adr/0012-two-release-lines.md) D10 records the release decision. |
| Where it comes from | `cargo install lrsalvage-cli`, a release archive, or a package manager | its URL, or the same files deployed to any static host |
| What a number means | the artifacts, the checksums and the contract surfaces below | the app's own screens and files |
| Who owns the promise | [ADR 0010](adr/0010-what-0-2-promises.md), both lines alike | the same record |

The core line is the one that exists today: `lrsalvage 0.4.0` is current. The app
line has not released anything yet; when it
does, the artifact is the page — [ADR 0012](adr/0012-two-release-lines.md) resolved
D10 that way on 2026-11-18, and its third test says why a page is a front end over a
durable engine rather than the other way round.

**The numbers never imply each other.** `core 0.4.x` does not mean the app is
0.4, and an app patch does not produce a core patch. A release on one line never
waits for a release on the other — that is ADR 0012's operative sentence, and it
holds in both directions.

## Which version do you have

- **The core line**: `lrsalvage --version` prints `lrsalvage <version>`. The same
  version appears as `version` in `manifest.json` and in the verification report.
- **The app line**: the app shows both numbers, in the form
  `lrsalvage app <app> · core <core>`, and the same pair is on the receipt's
  diagnostics line. On the page it is the About panel in the footer; a shell, if one
  ever ships, carries the same pair.
- **A file this tool wrote**: every recovered JPEG carries an EXIF `Software`
  marker reading `lrsalvage <core version>`, which any EXIF reader shows. That
  version is the **core** line's, whichever front end wrote the file, because the
  marker is what makes a later run recognise its own output.

When reporting a problem, quote the pairs above and nothing from the catalog: the
path-free verification report exists so a run's shape can be shared without a
single one of the user's paths.

## What a number promises

The rules are [ADR 0010](adr/0010-what-0-2-promises.md)'s and apply to both lines:

- **A patch** changes nothing in ADR 0010's list of surfaces. If it does, it was
  mislabelled.
- **A minor** may *add* to them — a field, a stat, a flag, a status word, an exit
  code — as long as no existing one changes meaning.
- **A removal, a rename or a changed meaning** needs a new minor that the
  preceding release announces, and never arrives in a patch.

The app adds one rule of its own: **an app release may require a newer core, never
an older one**, and the core it ships with is named in its changelog entry.

## Service and support

- **The latest release of a line is the one that receives fixes.** There is no
  backport branch and no supported older version: a defect in `0.4.0` is fixed in
  whatever the core line releases next, and the published tarball of `0.4.0` stays
  as it is.
- **A defect is fixed in a patch only when it changes nothing in the contract.**
  That route is available and has been passed over deliberately — ADR 0012 D5
  records why `0.3.1` does not exist.
- **The app line does not backport either.** One supported app version at a time.

## What each line can do

Capabilities, not flags: the flag list belongs to `lrsalvage --help` and the README
section that `tests/readme.rs` holds to the binary. Where a capability *is* one
flag, the cell names it and links there rather than restating what it does.

*Verified at core `0.4.0`, 2026-11-20. The app column is the intended shape of
`0.1.0` — [#117](https://github.com/marcd35/lrsalvage/issues/117) owns the
acceptance list, and [ADR 0012](adr/0012-two-release-lines.md) D10's resolution says
the artifact is the page — and not a claim about a released artifact.*

| Capability | Core line (CLI) | App line |
|---|---|---|
| Recover every usable preview in one run | yes | intended for `0.1.0` |
| Choose where the JPEGs go | yes, with the destination safety rules in [Safety](../README.md#safety) | intended, through a folder picker |
| Choose a folder layout | the catalog's tree only | the tree, or one flat folder, previewed before writing |
| Take previews under the 640 px floor | [`--min-long-edge`](../README.md#quick-start) | intended, as an explicit small-preview group |
| Cap the level that gets written | yes, [`--max-long-edge`](../README.md#quick-start) | not planned |
| See a preview before deciding to keep it | no — no pixels are ever decoded | intended: thumbnails and an enlarged view |
| Save one preview on its own | no — a run writes its plan | intended, from the enlarged view |
| Predict what would be written, writing nothing | [`--dry-run`](../README.md#quick-start) | intended, as a final-paths preview |
| Machine-readable output | [`--json`](../README.md#quick-start), `report.csv`, `extract.csv`, `manifest.json` | not planned |
| An exit code a script can branch on | yes, [six of them](../README.md#exit-codes) | not applicable |
| Prove the catalog was not touched | [`--verify-source`](../README.md#quick-start) | intended: the copy the app makes *is* the snapshot, and the same two digests are reported |
| A receipt with counts, failures and report links | the terminal summary and the reports it writes | intended, plus **Open folder** |
| Stop a long operation and continue it | no — rerun it; a verified destination is skipped | intended, stopping at a file boundary |
| Turn a preview cache into a contact sheet | [`--contact-sheet`](../README.md#quick-start) | a follow-up |
| Export AI masks | `lrsalvage masks` | not planned |
| Search, filter and sort results | no — `manifest.json` carries every fact a script needs to filter | intended |
| Group results by original folder | no | intended, collapsible |
| Reserve a destination with not enough free space | no — the run refuses with exit `4` before it writes anything, naming the shortfall ([exit codes](../README.md#exit-codes)) | intended, and it depends on that slice |
| Keyboard-only operation | yes — it is a terminal | target, and it is verified |
| Screen-reader usable | whatever the terminal provides | target: [WCAG 2.2 AA](adr/0012-two-release-lines.md) |
| Scriptable, usable in CI | yes | no |
| Runs with no network at all | yes | not on the first visit, and only from a cached page afterwards — an offline copy opened from `file://` cannot run the recovery path at all ([browser-spike](browser-spike.md) §5) |
| macOS and Linux | yes, two archives and `cargo install` | not tested or claimed for `0.1.0` — a browser page may run there; nothing here will say it does until somebody measures it |

## What this page will not become

It carries no list of milestones, issues or release dates. Those live in the
tracker and change weekly; a copy here would be stale before it was read, which is
the failure [`CODING_STANDARDS.md`](../CODING_STANDARDS.md) calls a second home for
a fact. What this page holds is the part that changes when a *decision* changes,
and the decisions are dated records in [`docs/adr/`](adr/).
