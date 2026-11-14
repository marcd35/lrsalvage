---
id: error
type: module-design
title: "error: one error type for the whole core"
status: draft
parent: architecture
---

# error

## Responsibility

The single error enum every fallible core function returns, plus `IoContext` for
attaching the path an I/O failure happened on.

## Boundary

No I/O, no presentation, and — deliberately — no exit codes. The CLI maps this
type to a process status; a module that returned an exit code would be deciding
about a caller it cannot see.

## Decisions

- **One type, not a hierarchy.** A caller matches on variants that name the actual
  failure — `NotACatalog`, `LrprevTruncated { offset }`, `UnsafeDestination`,
  `IccTooLarge` — instead of unwrapping a boxed cause chain.
- **Every I/O error carries its path.** `IoContext::at` exists so a report can say
  which file failed, which is the difference between a usable report and a bare
  "permission denied".
- **Truncation carries the byte offset.** "Truncated at byte 428,562" is
  diagnostic evidence; "invalid file" is not, and the report has no other way to
  explain a failed preview.
- **Unsafe and unusable destinations are distinct variants** because they are the
  only two that mean exit 4: `--out` inside the catalog or the cache, or a
  destination that cannot be created or written. Everything else that stops a run
  early is a usage or input error.
