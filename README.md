# lrsalvage

Recover JPEG previews from an Adobe Lightroom Classic catalog when the original
RAW or JPEG files are gone.

You have a `.lrcat` and a `* Previews.lrdata` directory on an old drive. The
photos they point at are not there any more. Lightroom still has a rendered copy
of each one in its preview cache, and this tool writes those copies out as
ordinary JPEG files, with the catalog's metadata attached.

Version 0.3.0: both crates are on crates.io at that version, the archives are on
the [`v0.3.0` release](https://github.com/marcd35/lrsalvage/releases/tag/v0.3.0),
and [the 0.3 release checklist](docs/release-0.3.md) is what the number covers.
[Install](#install) names all four routes, and
[ADR 0010](docs/adr/0010-what-0-2-promises.md) is the surface list a script can
lean on. 0.2 was the first release with something to download rather than a tree
to build, and 0.3.0 adds one flag while keeping every promise that release made.
Still early, and deliberately conservative: it would rather tell you it found
nothing than hand you a folder of wrong images.

## Lightroom is not required

You do not need Lightroom installed, licensed, or running. `lrsalvage` is a file
reader, not a plugin: it opens the catalog (a SQLite database), `previews.db`, and
the `.lrprev` payload files, and writes JPEGs. It never talks to Adobe software,
uses no Lightroom SDK or COM interface, and reads nothing outside the two paths
you give it.

That is checked rather than claimed:

- The release binary imports only system libraries — `kernel32`, `ntdll`,
  `bcryptprimitives`, `VCRUNTIME140` and the C runtime API sets — plus bundled
  SQLite. No Adobe module is in its dependency table.
- A full run with Lightroom Classic installed but not running loads 12 modules,
  none of them from Adobe.
- It runs happily against a catalog copied from another machine, including
  originals that point at drives which no longer exist (`K:`, `G:`). Only the
  cache is read.

So a `.lrcat` and a `* Previews.lrdata` recovered from a dead laptop, an external
drive, or a backup folder are enough on their own, and no Adobe licence is needed
to run this tool. What you give up without Lightroom is downstream of recovery
anyway: re-rendering develop edits, regenerating previews that were never built,
and anything above the resolution that was cached. See
[What this cannot do](#what-this-cannot-do).

One practical note: close Lightroom before extracting. The tool copies the
catalog together with its `-wal` and `-shm` sidecars and opens the copy read-only,
so an open catalog cannot corrupt a run — a closed one simply gives a cleaner
snapshot.

## The hard truth, before anything else

These are **previews**, not originals.

- A standard preview is 1440 px on the long edge — typically 1440 × 960. That is
  what most catalogs contain, and 1440 px is the ceiling for those images.
- Only images whose 1:1 previews were built have anything larger. In one real
  3,420-entry cache, 53 images had 5,760 × 3,840 previews; the rest topped out at
  1,440 × 960 or smaller.
- Previews are lossy JPEGs. Nothing here can add back detail that was never
  stored.
- A recovered preview is **not** a restored photo. You get a usable picture; you
  do not get your RAW file back.

If that is still worth having — and after a drive failure, it usually is — read
on.

## Install

Four routes to the same `lrsalvage` binary: the platform archive for your
machine, `cargo install` if you would rather have cargo build it, `cargo
binstall` to fetch a prebuilt binary instead of building one, or a source build
for a platform this release ships no archive for. [Compatibility](#compatibility)
says whether it reads your catalog at all, before you download anything.

This release ships **no installer and no notarised bundle**. Both are decisions
rather than gaps — [What this repository is not](#what-this-repository-is-not) has
the cost behind each — so what follows is you unpacking an archive and putting the
binary where you want it.

### Release archives

One archive per platform, attached to the release for the tag alongside
`SHA256SUMS`:

| Platform | Asset |
|---|---|
| Linux x64 (glibc 2.35 or newer) | `lrsalvage-x86_64-unknown-linux-gnu-v0.3.0.tgz` |
| Windows x64 | `lrsalvage-x86_64-pc-windows-msvc-v0.3.0.zip` |
| macOS, Apple Silicon only | `lrsalvage-aarch64-apple-darwin-v0.3.0.tgz` |
| checksums for all three | `SHA256SUMS` |

Those four assets are on the
[`v0.3.0` release](https://github.com/marcd35/lrsalvage/releases/tag/v0.3.0).
The [`v0.2.0-rc.1` pre-release](https://github.com/marcd35/lrsalvage/releases/tag/v0.2.0-rc.1)
proved the workflow first, under that tag's spelling (`…-v0.2.0-rc.1.tgz`) — the
tag is the version in every asset name, which is how one workflow builds both a
pre-release and the final one. What builds, proves and
publishes them is
[`.github/workflows/release.yml`](.github/workflows/release.yml) — its own step
list is the description, and the cheapest proof it offers is that the archive you
download was unpacked and run before it was attached. An archive holds the binary
and both licence texts at its root, and nothing else.

The `…-{target}-v{version}.{tgz|zip}` spelling is `cargo-binstall`'s own default
template rather than a scheme invented here, which is what leaves the binstall
route below open without a second upload.

`SHA256SUMS` lists all three archives, so the checks below read the one line for
the file you downloaded rather than checking the whole file.

Linux — verify, unpack, run:

```console
grep lrsalvage-x86_64-unknown-linux-gnu-v0.3.0.tgz SHA256SUMS | sha256sum -c
tar -xzf lrsalvage-x86_64-unknown-linux-gnu-v0.3.0.tgz
./lrsalvage --version
```

macOS — the same, with the tool macOS ships in place of `sha256sum`'s name:

```console
grep lrsalvage-aarch64-apple-darwin-v0.3.0.tgz SHA256SUMS | shasum -a 256 -c
tar -xzf lrsalvage-aarch64-apple-darwin-v0.3.0.tgz
./lrsalvage --version
```

Windows, in PowerShell — compare the two hashes, which must be equal, then
unpack with `Expand-Archive` (or right-click → Extract All):

```powershell
Select-String lrsalvage-x86_64-pc-windows-msvc-v0.3.0.zip SHA256SUMS
(Get-FileHash lrsalvage-x86_64-pc-windows-msvc-v0.3.0.zip -Algorithm SHA256).Hash.ToLower()
Expand-Archive lrsalvage-x86_64-pc-windows-msvc-v0.3.0.zip -DestinationPath .
.\lrsalvage.exe --version
```

**macOS: one command before the first run.** The binary is unsigned and not
notarised, so Gatekeeper quarantines a downloaded copy and refuses it. Clear the
attribute once, on the copy you unpacked:

```console
xattr -d com.apple.quarantine ./lrsalvage
```

Neither macOS command has been run on a Mac against a published artifact. The
Linux and Windows routes were proved that way, command by command, against
`v0.2.0-rc.1`; at `v0.2.0` all three archives were downloaded and verified
against `SHA256SUMS`, and the Linux archive unpacked and its contents listed, but
no route was re-run end to end at the final tag. The macOS pair — `shasum -a 256
-c` above and this `xattr` line — has never run, because no Mac was available to
the release. They are what this page would run first on one;
`docs/release-0.2.md` is where the gap is written down, nothing here is upgraded
to proved, and [#93](https://github.com/marcd35/lrsalvage/issues/93) stays open
until a Mac has run them. At `v0.3.0` all four assets were checked against
`SHA256SUMS` again — the checksums, not the routes — and
[the 0.3 checklist](docs/release-0.3.md) holds that measurement.

**macOS is Apple Silicon only.** There is one macOS archive and its target is
`aarch64-apple-darwin`; on an Intel Mac, build from source (a few lines below) or
use `cargo install`, which compiles for the machine it runs on.

### cargo install

```console
cargo install lrsalvage-cli --version 0.3.0
```

The package is `lrsalvage-cli`; the binary it installs is `lrsalvage`, so the
command you run afterwards is still `lrsalvage`. Both names are on crates.io as
0.3.0, and the route was proved by running that command where no checkout of this
repository was in scope: cargo built both crates from the registry and the
installed binary answered `lrsalvage 0.3.0`
([the 0.3 checklist](docs/release-0.3.md) records the run).

### cargo binstall

[`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall) downloads the
platform archive instead of building it. It is a cargo subcommand you install
once — `cargo install cargo-binstall`. Both forms have been proved, and the
paragraph below each one says against which tag:

```console
cargo binstall lrsalvage-cli
cargo binstall --git https://github.com/marcd35/lrsalvage lrsalvage-cli
```

The bare form resolves through the crates.io index; the `--git` form clones this
tree and needs no registry entry. The bare form was run against `0.3.0` and fetched
the prebuilt archive for the tag rather than falling back to `cargo install`, in
seconds; the `--git` form was proved the same way against `v0.2.0`. Neither form
changes the archives: they are already named to `cargo-binstall`'s own default
template.

### From source

```console
cargo build --release
```

This is the route for an Intel Mac, for a platform this release ships no archive
for, or for reading the code before trusting it. Output path and requirements are
in [Building](#building).

## Compatibility

Two things version independently here, and conflating them is how a "supports
v13+" claim goes wrong: the **catalog format** (`Adobe_DBVersion`, a SQLite
schema Lightroom has changed slowly) and the **preview layout** (which Lightroom
replaced in 13.3, when one `.lrprev` container per image became one JPEG file per
level). `scan` reports both, and neither of them gates a run: `layout` is one of
five words, and the two version strings are printed exactly as the catalog stores
them.

| Released | Preview layout | Catalog format | Tool run |
|---|---|---|---|
| LrC 4 – 9 | `legacy` — one `.lrprev` pyramid per image | not measured | same reader, **untested** |
| LrC 10 – 12 | `legacy` | `1200001` (see below) | **v11 verified** on real catalogs |
| LrC 13.0 – 13.2 | `legacy` | not measured | same reader, **untested** |
| LrC 13.3 and later | `modern` — one extensionless JPEG per level, no `.lrprev` | `1504001` (see below) | **verified**: 78 of 78 recovered, then written |

Exactly two `Adobe_DBVersion` values have been read off real files: `1200001`,
and `1504001` on that same catalog after Lightroom Classic 16.0 upgraded it in
place. Both are *that catalog's* numbers at two moments — not a table keyed by
release, since a 16.0 application wrote `1504001`. So compare your own
`scan --json` against them knowing you are looking at two data points, not a
mapping: an `Adobe_DBVersion` you have not seen here is not "unsupported", it is
a cache whose **layout** the tool still reports and reads.

`Released` is for you, not for the code: no version decides whether a cache is
read. The layout does, and it accounts for two caches a release column cannot
name: one upgraded in place, which *can* hold `mixed` — both representations, one
pyramid at a time, though the upgrade measured here converted every `.lrprev` and
reports `modern`; and one holding files that match no layout at all, which is
`unknown`:

| `layout` | Cache holds | `scan` |
|---|---|---|
| `legacy` | only `.lrprev` containers | reads each pyramid from its container |
| `modern` | only loose payload files | reads each level file, certifying it against its `PyramidLevel` row |
| `mixed` | both kinds, anywhere in the cache | resolves per pyramid: a level file wins, its container is never read |
| `none` | an index and no payload files | every image is `no-cache-entry`, exit 3 |
| `unknown` | payload-like files, none of a known shape | one hint naming how many matched no shape, exit 3 |

A sixth state sits beside those five and is not a layout word: a cache whose
`previews.db` is **missing or unreadable**. No index is no *mapping*, not no
previews, so the run does not stop. Every image whose original is not on disk
comes back `no-cache-entry`, every payload pyramid is listed as `unmapped`, the
layout is still detected from the tree, and the run exits 3 with one hint naming
the file:

```text
hint: D:/old/Photos/Photos Previews.lrdata/previews.db is missing, so 2 payloads cannot be matched to any of the 2 catalog images.
      They are listed as unmapped and never written; scan --verbose prints each path, extract records them in report.csv.
```

A damaged file degrades identically and prints `could not be read (sqlite error:
…)` in place of `is missing`. No JPEG is ever written for a payload no catalog
image names. A `--previews` path that is not a directory is the opposite case: a
usage error, exit 1, because a typo must not read as an empty cache. See
[ADR 0006](docs/adr/0006-degrade-without-guessing.md).

What those rows mean in practice:

- **Verified** for 11: Lightroom Classic 11 catalogs, the ones this tool was
  built and tested against — a 3,416-image catalog (2,659 previews recovered)
  and a 78-image one (78/78).
- **Verified** for 13.3 and later means the tool read *and wrote* a real cache of
  that generation: the 78-image catalog upgraded in place by Lightroom Classic
  16.0, where `scan` reports `layout modern`, 78 of 78 images recovered, no
  failure and no unmapped payload, and a shape histogram of 390 level files whose
  leaves all name the long edge their own frames declare. `extract` then wrote 78
  JPEGs, a second run found all 78 already ours and skipped them, and
  `--all-levels` wrote the 156 levels at or above the floor (section 5.9 of the
  notes). A **freshly built** 16 cache was then measured too: three catalogs
  rendered from scratch report the same layout, every level file certifying
  against its own row and its own frame, and payloads that carry Adobe's own
  `Exif`, XMP and ICC profile — which `extract` keeps (section 5.10). A cache
  Lightroom had **discarded** was measured as well (section 5.11): it removes the
  standard-size and 1:1 levels and the image's thumbnail pyramid, keeps the levels
  below the standard size, and every image still came back `recovered` — at
  1920 px in that cache. What none of these runs covers: a re-render of an image
  the catalog already holds.
- **Same reader, untested**: 4 – 10, 12 and 13.0 – 13.2 use the `.lrprev`
  pyramid this tool parses. Nothing known stops them working; nobody has run it
  on one.
- **No version refuses a run.** A newer Lightroom release cannot silently turn a
  readable cache into an empty run: the tool either reads it, or reports
  `unknown` with the shape it saw. See
  [ADR 0004](docs/adr/0004-detect-layout-not-version.md).

Where this README says "verified", it means someone ran the tool against real
files of that generation. Every claim below that with a byte-level basis carries
an evidence label in [docs/format-notes.md](docs/format-notes.md): **verified**,
**reported**, or **unverified**.

## Why not an existing tool?

There are several ways to get previews out of a Lightroom catalog. They differ in
what they need installed, which storage formats they understand, and how much
they give back.

| | Needs Lightroom installed? | Reads 13.3+ previews? | Restores metadata? | Approach |
|---|---|---|---|---|
| `lrsalvage` | No | Yes, one file per level | EXIF, ICC profile, star rating, capture-date mtime | Parses the container, then the level files |
| Friedl "Extract Cached Image Previews" | Yes (plugin) | Yes, since build 20240611.65 | Yes, via bundled ExifTool | Runs inside Lightroom |
| Adobe `ExtractPreviews.lua` | Yes (script) | Updated after 13.3 | No metadata, no ICC profile | Runs inside Lightroom |
| Python `.lrprev` extractors | No | No | Rarely | Scan for `FFD8`…`FFD9` |

The practical difference is the last column. Scanning for start/end markers
returns whatever JPEG-like blob appears last in the file — which can be the
thumbnail embedded *inside* a preview level, not the level itself. This tool
walks the declared chunk lengths and checks the decoded frame, so a wrong-sized
answer fails loudly instead of quietly becoming a 160 px file with your name on
it.

## Quick start

```console
# 1. See what is there. Writes nothing.
lrsalvage scan ^
  --catalog "D:\old\Photos\Photos.lrcat" ^
  --previews "D:\old\Photos\Photos Previews.lrdata"

# 2. Write the recoverable JPEGs.
lrsalvage extract ^
  --catalog "D:\old\Photos\Photos.lrcat" ^
  --previews "D:\old\Photos\Photos Previews.lrdata" ^
  --out "E:\rescued"
```

On macOS or Linux, use a trailing backslash instead of `^` to continue a line.

`scan` is the safe first move: it opens everything read-only, joins the catalog
against the preview index, and prints totals — recoverable images, their size
tiers, and a count for each other state that occurs. It writes no files unless
you name one. Give `--contact-sheet` a path to get a self-contained HTML page of
the images `extract` would write, each shown with the smallest level in its
pyramid — a contact sheet wants recognition, not resolution.

A count is not a verdict. `--verbose` adds one line per plan item — per image at
its best level, plus one per extra level with `--all-levels`, plus one per
unmapped payload:

```text
status  tier  WxH  profile  relative-output
recovered  standard  1616x1077  sRGB  12.31.23/DSC01652.jpg
```

Fields are separated by two spaces, and a fact the plan does not know prints as
`-`, so the five columns stay put for a script. `--json` replaces the table with
one machine-readable object; `--verbose` lines follow it.

`scan --json` emits these fields, and only these:

| Field | Meaning |
|---|---|
| `catalog`, `previews` | The two input paths, with `/` separators. |
| `layout` | What the cache holds: `legacy`, `modern`, `mixed`, `none` or `unknown`. Decided from the tree, never from a version. |
| `db_version`, `crs_version` | `Adobe_DBVersion` and `Adobe_CrsVersion`, exactly as the catalog stores them, or `null` when the catalog has no such row. Never interpreted. |
| `images` | Catalog images examined. |
| `recovered` | Images whose parsed payload agrees with the size `previews.db` declares. |
| `bytes` | Payload bytes of the items `extract` would write, before metadata is added. |
| `suspect` | Recovered, but the decoded size disagrees with `previews.db`. |
| `missing_payload` | `previews.db` has a row; neither the payload file it names nor any pyramid of the same image is on disk. |
| `no_cache_entry` | The catalog has the image; `previews.db` has no row. |
| `unmapped` | A pyramid that exists in the cache and that no catalogue image claims — and that the cache does not account for as a render's second pyramid. |
| `failed` | A payload that is truncated, unreadable, or holds no complete JPEG level. |
| `original_present` | Images skipped because the original file is still on disk. Always `0` with `--include-present`. |
| `below_floor` | Items the floor excluded. |
| `superseded` | Claimed pyramids held twice — level files and a legacy `.lrprev` — where only the level files are read. |
| `deleted` | `missing-payload` images whose pyramid `previews.db` has a `DeletedPyramid` row for. Lightroom's own record; it never claims who removed the file. |
| `unrecognised` | Payload-like files that matched no shape. They appear here and nowhere else: no status, no item, no exit code. |
| `thumbnail_pyramids` | A render's second pyramid: no cache entry claims it, its `Pyramid` row says `thumbnail`, and none of its levels reaches the floor. Counted here, never listed as `unmapped` and never written. |
| `tiers` | Object keyed `full`, `large`, `standard`, `small`, `thumbnail`; absent tiers are absent keys. |
| `shapes` | Object of three counts over the cache's files: `level_file`, how many of those leaves name the long edge their own frame measured (`suffix_matches_sof`), and `container`. |

None of `superseded`, `deleted`, `unrecognised` or `thumbnail_pyramids` is an
image's status, and none of them changes the exit code: they describe the cache
the run found, not what
became of any photograph.

`--json` keeps the two input paths, because stdout is for you or a script on your
own machine. When you want to hand a run to somebody else, use
`--report <file>`: it writes the **verification report**, one JSON object with
schema `lrsalvage.verification`, the tool `version` and `generated_unix`, the
`layout` and both version strings, the `options` the run applied (`min_long_edge`,
`all_levels`, `include_present`), the same `stats` counts as above, and `shapes`
as one object per class — `level_file` with its `count`, how many of those leaves
name the long edge their own frame measures (`suffix_matches_sof`) and the
grammar they follow, `container` with its `count` and grammar. A class the cache
showed no file for reports `count: 0` and carries no grammar, so an absent
template means "no such file", never a broken one. The only names in
the file are templates with `<uuid…>`, `<digest>` and `<longedge>` in place of the
real ones, so it carries no catalog or cache path, no folder, no uuid and no file
name. It is written whenever you ask for it and a plan exists — a run that
recovered nothing included — and it never moves the exit code. When you asked
`--verify-source` it also carries the proof: `source_sha256` and
`snapshot_sha256`, the catalog's digest and our private copy's, equal lowercase
hex, or `null` for both when the flag was not given — so the object is the same
eleven members either way — with `options.verify_source` beside the other knobs
the run applied. There is no
redaction flag on `--json`: two ways to produce one object would only be a flag
to forget. To let a maintainer read a cache they cannot see, paste the file into
the [verification report form](https://github.com/marcd35/lrsalvage/issues/new?template=verification-report.yml),
which asks for the report and what the run was against, and for nothing else.
Nothing there asks where your library lives, because the report has no path to
give.

`extract` takes the same two paths plus a destination. It shares
`--min-long-edge`, `--all-levels`, and `--include-present` with `scan`; the table
names which command each flag belongs to:

| Flag | Effect |
|---|---|
| `--min-long-edge 640` | Ignore levels below this long edge (default 640; `0` disables the floor). |
| `--all-levels` | Export every intermediate level, not just the best one. |
| `--include-present` | Also include images whose original file still exists on disk. |
| `--verbose` | `scan` only: print one line per plan item instead of only totals. |
| `--json` | `scan` only: emit one machine-readable object instead of the table. |
| `--report <file>` | `scan` only: write the path-free verification report to this file. Written whenever the flag is given and a plan exists, exit `3` included; its parent must not be inside the catalog, the cache or `.lrcat-data`; an existing name takes the next free number (`scan (2).json`). |
| `--verify-source` | `scan` and `extract`: prove the catalog was only read. It is hashed, copied, and the copy hashed; a pair that disagrees stops the run with exit `5` and a stated reason, before a plan exists. `scan --report` publishes both digests; `report.csv`, `manifest.json` and `--json` are unchanged. Opt-in because it is one extra read pass over the catalog, and the cache is never hashed. See *Read-only* under [Safety](#safety). |
| `--dry-run` | Do everything except write files, report files included. |
| `--force` | Replace a destination file that does not verify as ours. A file is ours only when it carries the `Software` marker `lrsalvage <version>` (the prefix is what is compared, not the version), is a complete JPEG, and its dimensions match the planned item. Anything else — a same-size file another tool wrote, or one of our own outputs from before the marker existed — is reported as failed and left byte-identical. In a metadata-bearing run it never replaces a file that *does* verify as ours; a `--no-metadata` run verifies nothing — its output carries no proof of authorship — so there `--force` replaces whatever it finds. See *Resumable* for the case where that matters. |
| `--no-metadata` | Do not add any metadata to the recovered JPEGs: no EXIF (so no orientation or rating), no rating XMP packet, no ICC profile, and no marker. Such output carries no proof of authorship, so a later run reports every destination as not a verified extraction and leaves it alone unless `--force` is given. |
| `--no-timestamps` | Leave the file modification time at the time of writing, instead of setting it to the capture time. |

### What lands on disk

The output tree starts at the **last segment of
`AgLibraryRootFolder.absolutePath`**, then follows `pathFromRoot`, then the file
name:

```
C:/Users/marcc/Pictures/2023/12.31.23/DSC01652.ARW
  → E:\rescued\12.31.23\DSC01652.jpg
```

It does not mirror the whole absolute path, so it never reproduces
`Users\marcc\Pictures\2023\...` under the destination. Real catalogs commonly
point `absolutePath` at the folder that was imported — here `.../12.31.23/` —
and leave `pathFromRoot` empty. Where a catalog instead uses a drive-level root
and puts the subfolders in `pathFromRoot`, the first directory is that root's
last segment: `C:/Users/marcc/Pictures/` plus `pathFromRoot = 2023/12.31.23/`
becomes `Pictures\2023\12.31.23\...`. A bare drive root (`C:/`) has no usable
last segment, so that level is dropped rather than turned into a `C:` folder.

Virtual copies get the copy name appended: `DSC01652 - Black and white.jpg`.
Output JPEGs are never named after a uuid.

The largest level per image wins, and "largest" is decided by decoded JPEG
dimensions, not by byte count — a small file with a big header is still small.
`--all-levels` writes the rest beside it, with ` - 1200x800` added before `.jpg`
so the extra levels do not collide with the best one.

Three report files are written in the output root, beside the recovered tree:
`report.csv` (one row per plan item), `manifest.json` (the same facts with a
schema and version), and `extract.csv` (one row per attempted write, with its
SHA-256 and any per-file note). A dry run writes none of them. `extract.csv`
needs a run that selected something; `report.csv` and `manifest.json` are also
written when the run selected nothing but found payloads no image claims, because
that orphan list is the only thing such a run has to hand over. None of them ever
overwrites an existing file: if `report.csv` is already there, the run writes
`report (2).csv` and says so. The numbered name applies to all three.

`report.csv` ends with a `payload` column and then `payload_metadata`, and every
item in `manifest.json` has a `payload` member: the file a preview was read from.
For a `unmapped` payload that is the only path there is — no image, no output
name — so it is a column to sort on rather than a sentence to parse out of
`note`. `payload_metadata` names the segments the payload itself already carried
— `exif,xmp,icc`, in that order, empty when it is bare — and is absent from
`manifest.json` when there are none. It is what tells a reader whether a file's
colour profile is the payload's own or one rebuilt from the catalog's name.

The run ends with a line on stderr such as
`78 evaluated, 78 written, 0 skipped, 0 failed, 25.3 MiB`. Sizes there are
mebibytes (1 MiB = 1,048,576 bytes). The reports keep exact bytes:
`manifest.json` as a total over the selected payloads, `extract.csv` per written
file. `extract.csv` also carries a SHA-256 for every file it wrote — a whole-file
digest no peer preview-recovery tool publishes, so an output stays checkable
long after the catalog is gone.

## Safety

The catalog is the only copy of the map. It is treated as read-only evidence:

- **Read-only.** Neither the `.lrcat` nor the cache is ever opened for writing.
  `--verify-source` is the evidence rather than the claim: it hashes the
  catalog, copies it, hashes the copy, and stops the run (exit `5`, naming both
  digests) if the two disagree, so a torn read is a failed run rather than a
  quiet one. `scan --report` publishes the pair. It is opt-in because the proof
  costs a read pass over the catalog, and asking for it is the only thing that
  pays.
- **Snapshot.** The catalog is copied to a temporary directory before querying,
  WAL included, so a Lightroom session left open on the same catalog cannot see
  half a query and a half-written WAL cannot corrupt the read.
- **Refuses dangerous destinations.** `--out` inside the catalog's own directory
  or inside the preview cache is rejected before anything is written. Pointing
  the tool at `My Catalog Previews.lrdata` and telling it to fill that folder is
  how a recovery becomes a second data loss. The same check covers the report
  files in the output root, a requested `--contact-sheet` and a requested
  `--report`, each validated before its own file is written.
- **`scan` writes only what you name.** Without a flag that gives it a path, it
  writes no file anywhere the user can see; every file it does write obeys the
  destination rule above and takes a free numbered name instead of overwriting
  one. The rule is "only what you name", not a count of artifacts, because the
  count is the part that keeps changing.
- **Atomic writes.** Each JPEG is written to a temporary file, flushed, and
  renamed into place, so an interrupted run leaves no half-JPEG masquerading as
  a recovered one.
- **Resumable.** Re-running `extract` skips an output only when it proves it is
  ours: it carries the `Software` marker `lrsalvage <version>`, is a complete
  JPEG, and its dimensions match the planned item. What is compared is the
  marker's `lrsalvage ` prefix, not the version inside it, so a later release
  keeps the files an earlier 0.2 one wrote. A same-size file somebody else wrote,
  or one written before the marker existed — anything a 0.1.0 run produced — is
  left byte-identical and reported as failed; `--force` is the explicit way to
  replace it. Output written with `--no-metadata` can never be proven ours, by
  design. Interrupting a run costs nothing.
  One blind spot comes with it: identity is the marker, the JPEG and the size,
  not the photograph. Two catalogs recovered into one destination can collide,
  and a different image that derives the same output name at the same dimensions
  is skipped as already recovered — and in a metadata-bearing run `--force` will
  not replace it either, because `--force` only replaces a file that does *not*
  verify as ours. A `--no-metadata` run cannot make that distinction and will
  overwrite it; see the `--force` row. Keep one destination per catalog, and
  delete the collided file if you need the other image.

## What the statuses mean

Every catalog image ends up in exactly one of these states; a payload that no
live catalog image claims is listed as `unmapped`.

| Status | Meaning |
|---|---|
| **recovered** | A payload was found and parsed, and its decoded dimensions agree with `previews.db`. `extract` writes it if it clears `--min-long-edge`; a recovered image below the floor is counted but not selected. |
| **suspect** | The payload is a real, complete photo whose decoded size disagrees with the size `previews.db` recorded. It is still extracted, subject to the floor, and flagged in every report: a real photo at the wrong declared size is worth recovering. |
| **original-present** | The original RAW/JPEG/TIFF is still on disk, so there is nothing to salvage. Skipped by default; `--include-present` turns these into ordinary recovery candidates. |
| **no-cache-entry** | The catalog has the image, but `previews.db` has no row for it. Nothing was ever rendered, or the row is gone. |
| **missing-payload** | `previews.db` has a row, but the payload file it names is not on disk — and no pyramid of the same image holds one either. A surviving render companion makes the image `recovered` at that size instead: below the floor, so counted but written only with `--min-long-edge 0`. That rescue is defensive rather than measured: Lightroom's own discard removes the companion too (section 5.11 of the notes). |
| **failed** | A payload exists but is truncated, unreadable, not a `.lrprev`, contains no readable JPEG level, or the level does not end at an end-of-image marker. |
| **unmapped** | A pyramid exists in the cache but no live catalog image claims it, and nothing in the index accounts for it. It is listed with its payload path — and its `quality` and `color_profile`, when its pair has a row — in `report.csv` and `manifest.json`, and never written: a payload with no catalog row has no file name to give it. A **freshly rendered** cache produces none of these: the second, thumbnail-sized pyramid Lightroom keeps beside the one each image claims is counted as `thumbnail_pyramids` instead, never as an orphan. |

`missing-payload` is worth reading twice. It means Lightroom believed a preview
existed and it is not on disk — the cache was partially deleted, a background
prune took the level, or the file never made it to this drive. When `previews.db`
still holds a `DeletedPyramid` row for that pyramid, `scan` counts it under
`deleted` and the note quotes Lightroom's own deletion time; that is a record of
what Lightroom did to its index, not an explanation of the missing file. It is a
different problem from "no preview was ever built", which is why the report keeps
them apart.

## Metadata restored

The preview's pixels are only half of what the catalog knows. With metadata on
(the default), `extract` rebuilds three things around the untouched payload:

- **EXIF** — capture time, camera make and model, lens, ISO, exposure time,
  f-number, focal length, GPS latitude and longitude when present, the orientation
  that makes the picture display upright, and the star rating as EXIF tag
  `0x4746`.
- **XMP** — a small packet carrying the same rating as `xmp:Rating`, but only when
  the payload carries no XMP packet of its own. Merging into a packet the payload
  already has is not attempted: that packet is left byte-identical and the
  omission is recorded in the run's per-file note, while the rating still lands
  in EXIF.
- **ICC** — unless the payload carries a profile of its own, in which case that
  profile is copied verbatim and none is rebuilt. Otherwise, a profile
  reconstructed from `Pyramid.colorProfile`. The catalog
  records the profile's name and never its bytes, so `sRGB` becomes a
  2,084-byte `sRGB IEC61966-2.1` profile and `AdobeRGB` a 560-byte
  `Adobe RGB (1998)` profile. Names are matched case-insensitively and ignoring
  spaces and punctuation. Any other name is reported — `colour profile 'ACR' has
  no built-in ICC profile; none embedded` — and none is embedded: a wrong profile
  is worse than none for colour-critical work.

That ICC profile identifies the colour space; when it was rebuilt, it is a
reconstruction rather than the profile the original file carried, and should not
be presented as the original.

A payload is sometimes a rendered JPEG that already carries metadata of its own,
and the stronger evidence wins: its own orientation tag describes *its* pixels,
so that value is written rather than the catalog's; its own ICC profile is kept
and ours is not embedded beside it; its own XMP packet is left byte-identical,
with the rating in EXIF instead. What the catalog knows better is replaced rather
than merged: our EXIF segment supersedes the payload's, so render-time tags such
as maker notes are lost, and the run's per-file note says so. `payload_metadata`
reports what the payload carried, and `orientation` in both machine surfaces is
the value actually written.

Only what the catalog actually recorded is written. Missing fields stay missing —
no guessed timestamps, no invented camera names. An explicit `xmp:Rating = 0` is
a recorded rating, and is written as such.

The per-file note is the `note` column of `extract.csv`. It is not echoed for a
successful write, and a dry run writes no report at all, so a dry run's notes are
not shown; check a real run's `extract.csv` when colour matters.

Unless `--no-timestamps` is given, the file's modification time is set to the
capture time; if that date is missing, unparseable, or ambiguous in the machine's
zone — a DST fold reads it twice, a spring-forward gap never — the mtime is left
alone.

The compressed image data is copied byte for byte. The only bytes added or
replaced are metadata segments: EXIF and XMP APP1, ICC APP2. A payload's EXIF
segment is replaced by ours, never duplicated; its ICC and XMP segments are kept
as they are. Every output carries our `Software` marker (`lrsalvage <version>`),
which is how a later run proves it wrote the file. With `--no-metadata`, the
payload is written unchanged and carries no marker. See
[ADR 0003](docs/adr/0003-preserve-preview-bytes.md).

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success. Also what `--help` and `--version` return. |
| 1 | Usage or input error: bad arguments, a catalog that is missing, unreadable or not a catalog, or a `--previews` path that is not a directory. |
| 2 | `extract` completed with some failures: at least one selected image could not be written. The run did real work and also hit something it could not do. `scan` never returns 2. |
| 3 | Nothing selected to do: no image was selected to write — for example, every original is still present, every preview is below the floor, or the cache has no readable `previews.db` so no payload could be named. `scan` and `extract` use the same test, so they agree on this code. `masks` returns 3 when it finds no `.dat` file or decodes none. |
| 4 | Unsafe or unwritable destination: `--out` (or the contact sheet's or verification report's directory) resolves inside the catalog or preview cache — or, for `masks`, inside the `.lrcat-data` directory — or cannot be created or written. |
| 5 | The catalog changed while it was being read: `--verify-source` hashed it, copied it, and the copy is not the catalog. Nothing is planned from that copy, so no JPEG and no report is written. Retry, with Lightroom closed if it was open on this catalog. |

`2` is the one to pay attention to in scripts: the report says which items failed
and why. `3` is not an error — a catalog where every original is still on disk
legitimately has nothing to salvage. `5` is not retryable unchanged: the run's
premise is that the evidence was only ever read, so a disagreement stops there.

## The compatibility contract

[ADR 0010](docs/adr/0010-what-0-2-promises.md) is the list of surfaces this tool
promises a script, across versions. This project versions two artifacts — the core
line and the app line — independently; what a number promises on each, and what
each artifact can currently do, is [docs/versions.md](docs/versions.md). The table
below says where each surface is stated
in this file and what holds it: a **gate** is a test in this tree that pins the
surface in the binary, so changing it fails `cargo test`; **review** means no test
pins it and a person is the only check. The two are named apart wherever they part
company.

The table is a map, not a second copy. This page keeps no list of its own, and
where a detail lives only in the code, the row says so rather than inventing a
home for it.

| Surface | Where this README states it | Held by |
|---|---|---|
| The seven status words: six an image can end in, plus the state for a payload no image claims | [What the statuses mean](#what-the-statuses-mean) | Gate for the states — `crates/lrsalvage-core/tests/matrix.rs` asserts the status each image of a vault came out with. Review for the words: nothing compares `no-cache-entry` or `missing-payload` to this page. |
| The exit codes `0`–`5` | [Exit codes](#exit-codes) | Gate for the numbers and for the arms — `main.rs`'s unit test asserts the six codes and one case per code, and `exit_code_for` names every error variant rather than defaulting, so a new variant does not compile until the mapping gives it one. Review for the rows' prose and for moving an existing variant to another code, which nothing sees. `scan`'s and `extract`'s codes are asserted per invocation in the CLI matrix (`crates/lrsalvage-cli/tests/matrix.rs`) and `tests/cli.rs`, and nothing runs `masks`, so its two clauses stay the same kind of promise. |
| `scan --json`'s field set, "these fields, and only these" | The field table under [Quick start](#quick-start) | Gate — `crates/lrsalvage-cli/tests/readme.rs` compares the table to the object the binary prints, field for field. |
| The verification report's key set under `lrsalvage.verification`, and `lrsalvage.manifest`'s envelope | The `--report` paragraph under [Quick start](#quick-start) spells out the verification report's members; [What lands on disk](#what-lands-on-disk) names `manifest.json` as the same facts with a schema and version | Gate — the CLI matrix asserts the verification object's exact eleven keys and `report.rs`'s own tests hold the manifest's; both schema strings are asserted as literals, and each file's `version` against `CARGO_PKG_VERSION`, so a bump that misses a surface fails. |
| `report.csv` and `extract.csv`: their columns, in order, and what `payload_metadata` means | [What lands on disk](#what-lands-on-disk) states the tail's order and what each file holds, and the header the run writes is the list of columns | Gate for the tail — `tests/readme.rs` asserts the header ends `,note,payload,payload_metadata`, and the CLI matrix pins the field count and those columns' values. Review for the rest of the header: renaming or swapping a middle column passes. |
| Output naming: the tree, an extra level's suffix, a virtual copy's name, a report name already taken | [What lands on disk](#what-lands-on-disk) | Review — `model.rs`'s tests pin the naming code and the CLI matrix pins the numbered `--report` name, but nothing reads these rules off the page. |
| The flags, and which command takes each | The flag table under [Quick start](#quick-start) | Gate that a flag exists — `tests/readme.rs` fails when the table names one no command has. Review for which command takes it: `--help` is read from all three at once, so a `scan` only flag listed under `extract` would pass. |
| The `Software` marker `lrsalvage <version>`, and that only its prefix is compared | The *Resumable* bullet under [Safety](#safety), and [Metadata restored](#metadata-restored) | Gate — `extract`'s `a_preexisting_verified_file_is_skipped_and_its_bytes_untouched` marks a destination `lrsalvage 0.1.0` and expects any later binary to skip it. |

Two tiers govern what may change. **Additive** changes — a new field, stat, flag,
status word or exit code, provided no existing one changes meaning — may land in a
minor release; a sixth exit code is a decision rather than a bump, and `5` was
taken on that basis by 0.3.0. **A removal, a
rename or a changed meaning** needs a new minor preceded by a deprecation stated
in the release before it, and never arrives in a patch: a patch changes nothing in
this list.

The version inside the marker is not part of the contract, which is what the
prefix rule buys: resume compares `lrsalvage ` and stops there, so a later release
keeps the files an earlier one wrote instead of reporting a whole library as
somebody else's.

A review-held row is a gap stated rather than hidden: those are the promises the
frozen-surface review in [the 0.3 checklist](docs/release-0.3.md) reads by hand,
because no test can. Where a row says gate, the test it names is the authority and
this page is the summary.

## What this cannot do

- **Recover originals.** No tool can. A preview is a derivative, and the RAW is
  not hiding inside it.
- **Beat the cache.** 1440 × 960 is the ceiling unless 1:1 previews were built.
  Expect 5,760 px only for the images whose 1:1 previews were.
- **Re-render your edits at full size.** The preview has Lightroom's adjustments
  baked into its pixels. That is often exactly what you want to look at, but
  there is no full-resolution render hiding in the cache, and this tool does not
  export the develop settings themselves.
- **Recover video.** Motion previews are poster frames, not the clip.
- **Recover images that never had previews.** Discarded previews, images imported
  but never rendered, and catalogs that were only ever used in Library grid view
  may have nothing but grid thumbnails.
- **Undo compression.** A JPEG that was re-encoded once cannot be made lossless
  again. `lrsalvage` never re-encodes, so at least it does not add a second
  generation of loss.

One confusion worth heading off: a `.lrcat-data` directory sitting next to the
catalog is **not** preview data. It holds AI mask alpha channels (Select Subject,
Select Sky). Nothing recoverable lives there. If you want those masks,
`lrsalvage masks --data "...\Photos.lrcat-data" --out "E:\masks"` decodes them to
PNG; it is a separate command precisely because they are not photographs. See
[docs/format-notes.md](docs/format-notes.md) section 3.

## Building

Requires Rust 1.89 or newer — `rust-version` in `Cargo.toml`, which
`scripts/check.sh msrv` proves.

```console
cargo build --release
```

The binary is `target/release/lrsalvage.exe` on Windows, `target/release/lrsalvage`
elsewhere. No Lightroom installation is needed, and no network access.

```console
cargo test
cargo clippy -- -D warnings
```

Tests that need a real catalog are `#[ignore]`d and read their fixtures from
environment variables, so the default `cargo test` runs against synthetic
fixtures only.

### Checks

The toolchain is pinned in `rust-toolchain.toml`, so `cargo` builds with the same
compiler here and in CI. **`sh scripts/check.sh` is the one list of gates** — CI runs
the same file, so this page does not repeat them:

```console
sh scripts/check.sh              # every gate
sh scripts/check.sh clippy       # just one
sh scripts/check.sh real         # the #[ignore]d tests, against a converted cache
sh scripts/check.sh real-render  # them, against a cache Lightroom rendered from scratch
sh scripts/check.sh real-1to1    # them, against a cache it built 1:1 previews for
sh scripts/check.sh real-discard # them, against a cache its previews were discarded from
```

Those last four are four evidence families rather than one: a machine can hold a
converted cache without a rendered one, a rendered one without a 1:1 build, or no
copy of a discard at all. Each gate fails rather than skipping when its own pair
of names is unset, and `.env.example` names all four pairs.

It covers formatting, clippy with `--all-targets`, the workspace tests, the
release build, spelling (config in `typos.toml`) and the licence/advisory policy
in `deny.toml`. A missing tool fails rather than skipping, so a green run here
means a green CI.

`cargo deny check` is what keeps the dual MIT/Apache-2.0 licence honest: a new
dependency that is not permissively licensed fails the build rather than being
discovered by someone redistributing the binary. Dependency updates arrive as
Dependabot pull requests.

Nothing in CI reads this README, so two of its tables are asserted by tests
instead: `lrsalvage-cli`'s `tests/readme.rs` checks the flag list and the
`--json` field list against the binary itself, and `lrsalvage-core`'s
`tests/spec_graph.rs` checks each module's spec against the modules it reaches.

### What this repository is not

`lrsalvage` is a command-line binary. This tree does release it: a `v*` tag runs
[`.github/workflows/release.yml`](.github/workflows/release.yml), which is where
the archive set, the `SHA256SUMS` file and the changelog-derived release body are
described. The commit a tag is cut on is chosen by
[the 0.3 checklist](docs/release-0.3.md), not by the tag itself.

What that release deliberately does not do. Each is a decision with a cost, and
the cost is why it stays out:

- **No installer.** You unpack an archive and put the binary where you want it.
  Nothing writes a registry key, a `PATH` entry or an uninstall record, so an
  install cannot leave a half-removed state behind. The cost: adding `lrsalvage`
  to `PATH` is your step, and there is nothing for a non-technical user to click.
- **No signing, no notarisation, no attestations.** The release's only integrity
  claim is `SHA256SUMS`. The cost: a Mac download needs the one manual step
  under [Install](#install) before it will run, and a Windows download carries no
  publisher signature to show you. Signing is a provenance decision with an
  account and a secret behind it — an Apple Developer ID, a certificate on a
  runner — not a flag on the build.
- **No auto-updater.** A new version is a new download, and the binary makes no
  update check, so it never reaches the network at all. The cost: nobody tells
  you a release happened.
- **No package-manager formula.** No Homebrew cask, Scoop manifest or distro
  package. Those routes carry their own maintainers and their own review, and a
  formula pointing at an archive that changes shape is worse than no formula. The
  cost: the archives, `cargo install`, `cargo binstall` and a source build are
  the routes this release carries, and a distro user's usual command is not yet
  one of them.

A graphical interface is planned after 0.3 rather than ruled out
([#68](https://github.com/marcd35/lrsalvage/issues/68)). It is separate in the way
that matters for this tree: reading, planning and writing live in
`crates/lrsalvage-core` behind a library interface, so a UI drives that library
rather than changing it, and the surfaces [ADR 0010](docs/adr/0010-what-0-2-promises.md)
freezes are the ones it inherits. There is no UI today.

## Licence

Dual-licensed under either of

- MIT
- Apache License, Version 2.0

at your option. See [`LICENSE-APACHE`](LICENSE-APACHE) for the Apache text.
