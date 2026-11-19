---
id: app-distribution
type: architecture-design
title: "App distribution and platforms: engineering notes"
status: active
parent: architecture
tags:
  - reference
  - distribution
references:
  - browser-spike
---

# App distribution and platforms: engineering notes

Facts about **other people's software** that decide how the front end is
distributed. [ADR 0012](adr/0012-two-release-lines.md) D9 and D10 make the
decisions and cite this page; this page holds the numbers, so a decision record
does not have to carry a price list that goes stale.

## How to read this page

- **Every row carries a label and a date it was read.** `verified` means the
  vendor's or the project's own page said it; `reported` means a third party did;
  `unverified` means nobody could confirm it and nobody should build on it.
- **These are not Lightroom claims**, so `format-notes.md`'s evidence standard is
  the *shape* followed here rather than its subject.
- **Re-read before citing.** A price or a browser-support row that predates the
  current release is stale by default: the source is named on every row precisely
  so that re-reading it is a one-minute job, not a research project.
- **This page decides nothing.** If a fact here changes, the decision that relied
  on it is amended in its own record — never by quietly editing a row.

## 1. What signing costs

Read 2026-11-18. **This is now a later-binaries question.**
[ADR 0012](adr/0012-two-release-lines.md) resolved D10 to the browser for `0.1.0`,
so none of these rows apply to the first release: a page has no installer, no
signature, no SmartScreen reputation and no Apple account. They apply to the
deferred shell, and they are kept here because it will be bought back with them.

| Route | Cost | Constraints | Label |
|---|---|---|---|
| **Microsoft Store**, MSIX package | free for individuals since 2025-09-10 (the $19 registration fee was waived) | MSIX only; Microsoft re-signs after certification, so the publisher shown is Microsoft | verified |
| **Microsoft Store**, MSI/EXE submission | free registration | you must Authenticode-sign the installer yourself before submitting; self-signed is refused | verified |
| **Azure Artifact Signing** (renamed from Trusted Signing) | **$9.99/month**, 5,000 signatures; $99.99/month, 100,000 | individuals: **USA and Canada only**; organisations: USA, Canada, EU, UK, and a defined list beyond; reputation still builds over time, "initial warnings expected" | verified |
| **OV certificate** | $150–300/year | the general-purpose paid route | verified |
| **EV certificate** | $400+/year | since 2024 "the same as OV" and no longer recommended specifically for a SmartScreen bypass | verified |
| **SignPath Foundation** | **free** for qualifying open source | OV-level; the certificate is issued to the Foundation, so the **publisher your users see is "SignPath Foundation"**, not the project; every component must be OSI-licensed with no commercial dual-licensing; the project must be actively maintained and released; **each release needs a manual signing approval**; the project must publish a code-signing policy with the attribution line | verified |
| **Certum Open Source** | from €49/year | publisher shows a fixed "Open Source Developer" prefix; non-commercial software only; the certificate is revoked if it signs commercial software | reported |
| **Self-signed** | free | "blocks install for public users" — worse than unsigned, because it looks like tampering | verified |
| **Unsigned** | free | "strong SmartScreen block" | verified |

SignPath's obligations are not free in effort: `LocalSend`'s `CODE_SIGNING.md` is
the worked example — a policy page, the attribution line, and a release that stops
for a human before it publishes.

## 2. SmartScreen is a reputation system, not a switch

Read 2026-11-18.

- Two signals are weighed: **publisher reputation** (is it signed, by a known
  certificate) and **file-hash reputation** (has this exact binary been downloaded
  and run without complaints). — verified
- **A newly signed binary can still warn** until its hash gains reputation, so
  paying does not buy a clean first launch. — verified
- **There is no manual submission** for consumer endpoints: "reputation builds
  organically through download volume", and the submission portal exists for wrong
  malware detections and enterprise admins. — verified
- **Store-distributed packages are re-signed by Microsoft** and are "never subject
  to SmartScreen download warnings" — the only route here that is both free and
  warning-free. — verified
- **How long reputation takes is not published.** Every figure in circulation is
  anecdote. — unverified

The engineering consequence, and it is a judgement rather than a measurement: this
tool's audience is someone who has never opened a terminal and may already be
frightened, so a red "unknown publisher" panel is the screen they close.

## 3. Distribution routes

Read 2026-11-18.

- **GitHub Releases** allows up to 1,000 assets, each under 2 GiB, with no cap on
  the total or on bandwidth — which is what `release.yml` already relies on for the
  core line's three archives and `SHA256SUMS`. — verified
- **Installer formats available to a Rust webview app**: MSI through WiX Toolset
  v3, or an NSIS `-setup.exe`. — verified
- **A portable ZIP beside the installer** is the precedent worth copying: it lets a
  wary user run the binary without installing anything, and it is what the core
  line already ships.
- **MSIX must be signed before it installs at all**, including outside the Store,
  where App Installer handles updates — so "MSIX" is not a way to avoid signing
  unless it goes through the Store. — verified
- **A page trades a signing route for a hosting one**, and a lapsed host is as dead
  as an expired certificate. There is no purchase to make and no queue to join, so
  the mitigation is structural rather than financial: a static export, a tagged
  deploy per release, and the core line as the artifact that outlives the page.
  — judgement; ADR 0012's D10 resolution is where the risk is accepted

## 4. The browser route's hard constraints

Read 2026-11-18. These are the facts #126's spike measures against, and — since
[ADR 0012](adr/0012-two-release-lines.md) resolved D10 to the browser for `0.1.0` —
the constraints the first release is designed inside rather than weighed against.
[browser-spike](browser-spike.md) has the numbers.

- **`showDirectoryPicker` is Chromium-only**: Chrome and Edge 86+, **not Firefox**
  (through 163 at the time of reading) and **not Safari**. MDN marks it "Limited
  availability… not Baseline", and one browser vendor has filed a negative
  standards position — "very unlikely to ever become Baseline in this current
  form". — verified
- It is available **only in a secure context**, so the page must be HTTPS. — verified
- Handles carry a **per-handle permission model**, and permission is granted per
  session rather than persisted the way a desktop app's file access is. — verified
- **SQLite in a page**: the common builds (`sql.js`, the official WASM build with
  an in-memory database) read the whole file into memory, which fails on a large
  catalog. **Amended 2026-11-18:** this row used to name a *demand-paged custom VFS
  over the picked file handle* as the working pattern, and half of that is wrong.
  A **read-only** VFS can page a picked file synchronously — the File API exposes
  `FileReaderSync` to workers, and Emscripten's `WORKERFS` and SQLite Explorer's
  blob VFS are shipping examples. A **writable** one cannot:
  `FileSystemFileHandle.createSyncAccessHandle()` is usable "only … for files
  within the origin private file system", so no synchronous VFS can journal,
  recover or checkpoint on a picked file, and an asynchronous build (JSPI/Asyncify)
  with the data layer in JavaScript is the only shape that can write there.
  [browser-spike](browser-spike.md) §1 has the specification sentences and §6 the
  route not taken. — verified
- **OPFS** is the alternative, and it is the only persistent one: copy the database
  in, then query it from the origin's private storage through the `sahpool` VFS.
  The cost is the copy time and the origin's quota. `navigator.storage.persist()`
  asks the browser not to evict it. Exact quota figures vary by browser, profile
  and free disk. — reported
- **Measured 2026-11-18** ([browser-spike](browser-spike.md)): a 1 GiB copy ran at
  86–178 MB/s; the quota was 10.7–12.0 GB and moved between sessions;
  `persist()` was refused while the data nonetheless survived a restart; a picked
  folder's grant survived a reload and became `prompt` after a restart. The
  crate's *checked* import copies the main file only and resets the header to
  rollback mode, so a live `-wal`'s committed rows are lost without an error — 78
  images read where 128 exist. That is the import's design, not a platform limit:
  importing the sidecar byte-preserving and opening with exclusive locking
  recovered **128 of 128** with every digest equal to a native run, since SQLite
  needs no `-shm` for a single owner. The desktop `snapshot` copies `-wal`/`-shm`
  and checkpoints, which is why the same file reads 128 there. — measured
- **A page writes into a picked folder at 27–29 files/s**, if it keeps eight to
  sixteen streams in flight: 1 000 payloads in 34 s with a p95 of 619 ms and no gap
  over 570 ms, 200 large payloads at 26.6/s, saturation between 8 and 16 streams
  (64 streams buys latency, p50 2.4 s, and no rate). Sequentially it is 7–8 files/s
  on a good session and 3.5 on a bad one. That is 2–4× a *bare* native write and at
  parity with a native write plus `sync_all`, which is what the shipped writer
  does; the same JavaScript into OPFS runs at 120–142 files/s. — measured
- **A page writes into a picked folder slowly** when it writes one file at a time,
  and the record above has the concurrent number that fixes it. Sequential runs
  through `createWritable()` gave 1.8–8.1 files/s depending on the session, with a
  p95 of 0.6–1.4 s and a worst single file of 3 s–26 s. — measured
- **What a page cannot do**: open Explorer at the destination, integrate with the
  shell, keep running after the tab closes, or survive an OOM kill mid-write.

## 5. Toolkit shortlist

Read 2026-11-18. Sizes are installer-size order of magnitude, which is what the
decision needs; release dates are the maintenance signal.

| Toolkit | UI layer | Size | Accessibility | Label |
|---|---|---|---|---|
| **Tauri 2.12.7** | HTML/JS/CSS (any framework, or none) | vendor claims a minimal app under 600 KB; Windows installers reported at 8–15 MB; WebView2 is preinstalled on Windows 11 and the installer can bootstrap it (embedded copy reportedly adds ~150 MB) | WebView2's accessibility tree is exposed through UIA, but host-integration bugs are long-standing (issues filed 2022–2023) | verified / reported |
| **egui / eframe 0.37** (2026-10-15) | Rust | single binary, no webview | AccessKit is enabled by default on native platforms; 0.37 changed widget types to `accesskit::Role` | verified |
| **Slint 1.19** (2026-11-16) | Slint DSL | single binary | AccessKit through the winit backend | verified in code; its docs page for accessibility returned 404 — unverified |
| **iced 0.14** (2025-12-07) | Rust | single binary | **documented gap**: "Implement accessibility support" has been open since 2020-10, and a 2026 audit states it does not expose a native screen-reader tree | verified |
| **WinUI 3 + `windows-rs`** (74, 2026-09-03) | C#/XAML or C++/WinRT; Rust via projections | framework-dependent | native UIA by construction — the strongest accessibility story, and the most manual interop with a Rust core | verified |
| **Flutter + flutter_rust_bridge** (2.14.0-beta.2, 2026-09-12) | Dart | framework | **documented gap**: the Windows engine uses MSAA/`IAccessible` rather than UIA; the tracking issue has been open since 2021-12 | verified |
| **Electron 45.3.4** (2026-11-17) | HTML/JS/CSS | installers reported at 100–120 MB | Chromium's UIA tree, same class of host bug as WebView2 | verified / reported |

The pattern worth recording: **the accessibility promise and the toolkit choice are
the same decision.** Two of these have open, dated gaps that would make #68's
"keyboard and screen-reader verification" impossible to keep.

## 6. How peers ship a CLI and a GUI

Read 2026-11-18, as precedent rather than authority.

| Project | Shape | Numbers | Distribution |
|---|---|---|---|
| **PhotoRec / QPhotoRec** | one engine, two front ends | share **7.2** (2024-02-22) | plain ZIPs, no installer |
| **ExifTool / ExifToolGui** | thin GUI over a mature CLI | **6.3.12** over **13.58**, independent | installer that may fetch the core at install time, plus a portable single-file exe (15–21 MB) |
| **LibRaw / RawDigger** | library and commercial app, one vendor | **0.22.3** (2026-10-26) under **1.4.10** (2026-02-11) | vendor installer, paid |
| **ripgrep** | binary plus a published `grep` crate from one repo | **15.2.0** over **0.4.1** | GitHub Releases, archives |
| **LrToolbox** | commercial Windows GUI doing the same job as this tool | calendar versioning, **2026.0.1.29** | vendor installer, paid licence |
| **Recuva** | commercial freemium recovery, the "non-technical" comparator | **1.55.133** (~4 MB installer, Pro $29.95) | vendor installer; its download page recommends NVDA for screen-reader users |

Two shapes exist in the wild: **one number** when the two front ends are one
release, and **skewed numbers** when they are two artifacts. This project is the
second, which is what ADR 0012 D1–D2 records.

## 7. Versioning a library and an application

Read 2026-11-18.

- **SemVer 2.0.0 does not distinguish a library from an application.** Rule 1
  requires the software to "declare a public API", rule 4 says a `0.y.z` series is
  initial development, and the FAQ says software used in production "should
  probably already be 1.0.0". It says nothing about two artifacts from one project,
  so an app with its own number is not a deviation — as long as the app declares
  its own public surface. — verified
- That surface is already declared here: ADR 0010's eight items, which both lines
  are held to, plus `docs/versions.md` for how each line's numbers map onto them.

## 8. Could not confirm

Recorded rather than guessed, because each one would otherwise be re-researched
from scratch by whoever reads this next:

- Whether Tauri, NSIS or WiX sign **sidecar binaries** as well as the app
  executable and the installer. The Tauri signing page covers the app and the
  installers and never mentions sidecars, and no general Microsoft guidance was
  found.
- **How much download volume SmartScreen reputation takes**, in either direction.
  Microsoft publishes that it is volumetric and that no review mechanism exists;
  every specific number is anecdote.
- Azure Artifact Signing's **per-signature overage** ($0.005 beyond quota). It
  appears on a third-party page and on no Microsoft page found.
- Whether a **Microsoft Store listing** is realistic for a niche recovery tool.
  The policies exist; no data on acceptance rates for this category.
- **Slint's** accessibility status from its own documentation — the page 404s, so
  the finding rests on the source and the merge that added AccessKit support.
