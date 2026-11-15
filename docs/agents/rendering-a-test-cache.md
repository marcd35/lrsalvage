# Rendering a test cache

When a claim is about what Lightroom *writes* rather than what it *keeps*, a
converted cache cannot answer it. This is how to make a cache Lightroom rendered
from scratch, and what to record while doing it.

`docs/format-notes.md` §5.10 is the output of the last run: three catalogs, ten
images each, every level file walked. The numbers there are what a reader should
compare against, and the method below is how they were produced.

## Why a render and not a conversion

An in-place upgrade splits a legacy container's level bytes into files and
preserves them: a converted cache holds the 13.2 levels, its payloads carry no
metadata, every pyramid shares one digest, and there is one pyramid per image. A
render produces none of that — a different level series, four metadata segments
per level, several digests, and two pyramids per image. Measuring the first and
claiming the second is the mistake §5.1–§5.9 spent a year scoped against.

## What Lightroom decides on its own

A new catalog is not neutral. On the 16.0 build measured here (`ProductVersion
16.0 (202611041403-7ac14c66)`, `Edit > Catalog Settings… > Previews`):

| Setting | A new catalog holds |
|---|---|
| Standard Preview Size | `Auto (3840px)` — the popup offers `Auto`, `1024`, `1440`, `1680`, `2048`, `2880` and nothing else |
| Preview Quality | `Medium` |
| Automatically Discard 1:1 Previews | `After 30 Days` |
| Limit Preview Cache Size | off |

Two consequences worth planning around. **A size you invent does not exist** — a
ticket asking for 2560 px cannot be run, so pick from the list and say which. And
**`Auto` is not "the default 1440"**: it sized the series by the display, and
`1024 px` produced a visibly different one, which is what makes the setting the
variable worth isolating.

## The recipe

Each catalog is small on purpose: one import folder of ten files renders in under
a minute, and every extra file multiplies the walk.

1. **Create the catalog** in an empty scratch folder (`File > New Catalog…`). It
   becomes the most recent catalog, so later launches reopen it and nothing else.
2. **Set the variable** in `Catalog Settings > Previews` — one catalog per value
   of it, because a re-render in place leaves the previous levels behind, which
   is exactly the pruned/stale state that cannot be told apart later.
3. **Import with `Build Previews: Standard`**, all files selected.
4. **For a 1:1 measurement, add it after the import**: apply the catalog facts you
   want tested (a star rating is enough) and then `Library > Previews > Build 1:1
   Previews`. A 1:1 build *replaces* the standard pyramid rather than adding to
   it, so the same catalog cannot be asked both questions.

Compose the import set deliberately: a render can only be shown to embed
orientation, EXIF and profile facts if the inputs carry them. Camera files whose
pixels are already upright carry no orientation tag, so orientation inputs have to
be generated — `Pillow` can write `EXIF 0x0112` without `exiftool`.

## Driving the GUI

Lightroom has no command line for any of this: `lightroom.exe` ships no
documented switch, a plugin's `catalog:addPhoto()` cannot choose the preview size
or build type, and the import dialog's choices are not scriptable. So the run is
UI automation, and five facts make the difference between a working driver and a
lottery:

- **Menu command ids are re-assigned whenever a catalog opens.** `File > New
  Catalog…` was `40510`, then `40513`, then `40515` across three sessions. Read
  the label from the window's own menu (`GetMenu`, `GetMenuStringW`,
  `GetMenuItemID`) and post `WM_COMMAND` with the id found in that moment. The
  `GetMenuStringW` declaration needs `CharSet.Unicode`, or every label reads as
  `&`.
- **The UIA tree has no `MenuBar`.** Lightroom's frame is `AgWinMainFrame` and the
  canvas under it is nameless `Afx` panes; only real dialogs (`#32770`) expose
  controls. So menus are reached natively and dialogs by UIA.
- **A dialog is a child of the frame, not a top-level window of the process.** A
  search over `RootElement` children finds nothing; descend from the frame.
- **Assert the foreground window before every click and keystroke.** A fresh
  `powershell.exe` puts its own console in front and `SetForegroundWindow` is then
  refused; input lands wherever focus is, which on a working machine is somebody
  else's application. Retry with restore or `AttachThreadInput`, and abort if
  Lightroom is not in front. Accept any window of Lightroom's *process* — an open
  combo list legitimately holds the foreground.
- **Declare the process DPI-aware, and watch the frame's size.** On a 200%-scaled
  display the frame opened wider and taller than the screen, which puts `Create`
  and `OK` off-screen: use the UIA rectangle (`SetProcessDPIAware` first, or
  rectangles are in scaled units) and press `Enter` for a dialog's default button.

Take a screenshot after every step and look at it. That is the whole safety model:
a step is not done because a call returned, it is done because the picture shows
it. Aborting on an unexpected window is the correct outcome, not a failure.

## Measuring it

Quit Lightroom first: a running app leaves `previews.db` beside a `-wal`, and the
numbers should be the settled ones.

```console
sh scripts/probe-cache.sh "<name> Previews.lrdata"          # every level file
lrsalvage scan --catalog "<name>.lrcat" --previews "<...>"  # the shipped reader
```

`probe-cache.sh` reports the series, the chain each file carries, where a row and
a file disagree, and how many pyramids and cache entries the store holds — the
last of which is how the two-pyramids-per-image state was found. The reader's
`scan --report` is the quotable artefact for everything it already answers.

Record, for the notes: the preview setting and quality, the level series as files
*and* rows, which segments each file carries, `Pyramid` rows against
`ImageCacheEntry` rows, `Pyramid.quality` values, the distinct digests, and the
tool's exit code and stats. Record the negative as carefully as the positive: "a
render looks like the conversion" retires a doubt just as well as "it does not".

## What not to do

- **Do not launch Lightroom to find out what it does with a cache you care
  about.** It opens the most recent catalog on launch — back that cache up first,
  with a hash manifest, or open a fresh catalog and let it become the recent one.
- **Do not import from a cloud-only folder.** Deferred downloads stall the import
  and skew anything timed.
- **Do not commit any of it.** Catalog bytes, rendered previews and the paths that
  name them never enter the repository; the notes carry numbers only.
