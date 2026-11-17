# lrsalvage-cli

`lrsalvage`, the tool that recovers the JPEG previews Lightroom Classic rendered,
when the photos themselves are gone.

Give it a `.lrcat` catalog and the directory beside it named
`* Previews.lrdata`. It reads the catalog and the `previews.db` that names the
payload each image owns, then writes each preview out as an ordinary JPEG, in a
folder tree that mirrors yours, with the catalog's metadata and capture time
attached. An image whose original is still on disk is reported and left alone.
Lightroom is not required: this is a file reader, not a plugin.

`lrsalvage scan` is the safe first move — it opens everything read-only and tells
you what `lrsalvage extract` would write, without writing it.

## Install

```console
cargo install lrsalvage-cli
```

The package is `lrsalvage-cli`; the binary it installs is `lrsalvage`, so the
command you run afterwards is still `lrsalvage`. Prebuilt archives and the
`cargo binstall` route are on the
[project page](https://github.com/marcd35/lrsalvage#install).

## The compatibility promise

[ADR 0010](https://github.com/marcd35/lrsalvage/blob/main/docs/adr/0010-what-0-2-promises.md)
lists the surfaces a script may lean on, and sorts changes into the ones a minor
may carry and the ones that need a new minor to deprecate them first. Read it
before building a script on this tool's output.

## What a preview is worth

What comes out is a preview, not the original: a compressed copy at the
resolution Lightroom stored, and nothing here adds back what was never in it. The
[repository README](https://github.com/marcd35/lrsalvage#readme) says so plainly,
and says whether this tool reads your catalog at all.

## Licence

Dual-licensed under either of

- MIT
- Apache License, Version 2.0

at your option. Both texts ship with this crate.
