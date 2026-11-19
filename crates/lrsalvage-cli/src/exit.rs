//! The exit-code vocabulary: the six numbers a run can end on, and the one table
//! a generated artifact reads.
//!
//! Each code's meaning is written **once**, in [`PUBLISHED`]: that is the section
//! the man page prints, its sentences are built on the constants below, and
//! `README.md`'s exit-code table is the long form of the same six rows. A
//! constant here carries its number and, where the number needs defending, the
//! reason it exists — not a second gloss of what it means.
//!
//! `main.rs` keeps the mapping from an error to a code (`exit_code_for`) and the
//! test that pins it, because that mapping decides what a new error means. The
//! test also asserts [`PUBLISHED`]'s numbers are exactly these six, so a seventh
//! code cannot reach the binary and miss the man page.

pub const EXIT_OK: u8 = 0;
pub const EXIT_USAGE: u8 = 1;
pub const EXIT_PARTIAL: u8 = 2;
pub const EXIT_NOTHING: u8 = 3;
pub const EXIT_UNSAFE: u8 = 4;

/// Its own code because it is its own problem: not a bad destination (`4`), not a
/// half-done extraction (`2`), and not something to retry unchanged.
pub const EXIT_EVIDENCE: u8 = 5;

/// The six published codes with a one-line meaning each, in order.
///
/// The numbers are the constants above rather than literals, so this table
/// cannot list a code the binary does not have. The man page's `EXIT STATUS`
/// section is rendered from it, and nothing else in this crate restates it.
pub const PUBLISHED: [(u8, &str); 6] = [
    (EXIT_OK, "Success, including --help and --version."),
    (
        EXIT_USAGE,
        "Usage or input error: bad arguments, a catalog that cannot be read, \
         or a preview cache path that is not a directory.",
    ),
    (
        EXIT_PARTIAL,
        "extract completed, and at least one selected image could not be written.",
    ),
    (EXIT_NOTHING, "Nothing was selected to do."),
    (
        EXIT_UNSAFE,
        "Unsafe, unwritable or too small a destination: the destination is \
         inside the evidence, cannot be written, or does not have the free \
         space this run would still write.",
    ),
    (
        EXIT_EVIDENCE,
        "The catalog changed while it was being read, so nothing was planned \
         from the copy.",
    ),
];
