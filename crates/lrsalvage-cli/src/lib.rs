//! The `lrsalvage` command surface, as a library.
//!
//! A binary crate's items are private to it, and the completion scripts and the
//! man page are *generated* from `clap`'s definition rather than kept by hand —
//! so the definition lives here, where a generator and a test hold the same
//! value the binary parses. `src/main.rs` is still the whole of the tool's
//! behaviour: it parses with [`cli::Cli`], drives `lrsalvage-core` and returns
//! the [`exit`] code the run decided.
//!
//! Nothing in this root touches a catalog, a cache or a payload byte.

pub mod cli;
pub mod exit;
