# The Windows prototype, kept where a checkout can open it

`lrsalvage-windows-prototype.html` is the prototype #117 was written against: one
self-contained page with a dark titlebar, a three-step navigation, five
walkthrough scenarios (save a batch, a small preview, folders and duplicate names,
stop and continue, no previews found) and a "Website guide" tab. ADR 0012 records
it, and #117's *Entry and review* decision was measured against it.

**It is a design reference, not the product.** It was written for the **Windows
executable** that ADR 0012 D10 replaced with a browser page, so its screens assume
things a page cannot do — the entry screen picks the `.lrcat` *file* and reads the
preview folder out of the file's own path, which no browser exposes. Its flows are
simulated: fictional paths, generated photos, and a recovery that reports success
without reading a cache.

Three things it is kept for:

- **The agreed experience.** Screen order, the shell, the wording of every step, and
  the shape of the "no previews found" answer are what #117 says survives the
  artifact change. A change to what the page says or shows should be checked
  against this file before it is called new.
- **The launch page's material.** Its five numbered steps are the numbered visual
  guide #117 wants, and the screenshots for it already exist in the prototype's
  release archive.
- **A worked comparison.** `specs/app.md` records that the page in `0.1.0` was
  written from the user stories and carries its own visual language, and that
  porting this shell is a copy pass rather than a rewrite.

Open it in a browser: no build step, no server, no dependencies.

```console
start docs/prototype/lrsalvage-windows-prototype.html      # Windows
open docs/prototype/lrsalvage-windows-prototype.html       # macOS
xdg-open docs/prototype/lrsalvage-windows-prototype.html   # Linux
```

The five `guide-0N.png` screenshots and the sample photo atlas it references are
**not** in this repository: they are generated images and sample data for the
launch page, and the prototype renders without them — the guide tab shows its text
and the photo grid falls back to its placeholder. They travel with the prototype's
release archive, which is attached to #117.

**Why this file is here at all.** It lived outside the repository, and a slice
built from #117's prose alone therefore shipped a page whose shell and wording
nobody could hold to it — the same shape as #126's lesson, "a harness nobody can
run from a checkout is a claim nobody can reproduce". A prototype nobody can open
from a checkout is a design nobody can follow.
