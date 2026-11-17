# lrsalvage-core

Recover the JPEG previews Lightroom Classic rendered, when the photos themselves
are gone.

A `.lrcat` catalog lists the images you imported, and the directory beside it named
`* Previews.lrdata` holds the rendered copies Lightroom built for them. This crate
reads those two as ordinary files on disk — the catalog, and the `previews.db`
that names the payload each image owns — and copies each preview's own JPEG bytes
out verbatim, adding only metadata segments around them. Lightroom is not
required: this is a file reader, not a plugin, and nothing Adobe ships is loaded
or called.

It is the library behind the `lrsalvage` command line tool,
[`lrsalvage-cli`](https://crates.io/crates/lrsalvage-cli).

## The compatibility promise

[ADR 0010](https://github.com/marcd35/lrsalvage/blob/main/docs/adr/0010-what-0-2-promises.md)
lists the surfaces a script may lean on, and sorts changes into the ones a minor
may carry and the ones that need a new minor to deprecate them first. Read it
before parsing anything this crate produces.

## Install

```console
cargo add lrsalvage-core
```

The command line tool is its own package: `cargo install lrsalvage-cli`.

## Documentation

- API reference: <https://docs.rs/lrsalvage-core>
- [The repository README](https://github.com/marcd35/lrsalvage#readme) — what a
  recovered preview is worth, and what this cannot do.

## Licence

Dual-licensed under either of

- MIT
- Apache License, Version 2.0

at your option. Both texts ship with this crate.
