//! Salvage JPEG previews out of Adobe Lightroom Classic catalogs.
//!
//! The crate is deliberately split into small, deep modules so that the CLI,
//! the future GUI, and the tests all drive the same verified pipeline:
//!
//! * [`snapshot`] — takes a stable, read-only copy of a `.lrcat` (WAL included).
//! * [`catalog`] — queries the catalog: images, paths, XMP metadata.
//! * [`previews_db`] — reads `previews.db`, the authoritative image → payload map.
//! * [`lrprev`] / [`header`] / [`jpeg`] — the `AgHg` chunk container and its parts.
//! * [`plan`] — turns the above into a list of decisions, without writing anything.
//! * [`fs`] — the one module that touches a filesystem, so the others can build
//!   for a target that has none.
//!
//! Two rules are load-bearing and recorded as ADRs:
//!
//! * **Parse, don't carve** (`docs/adr/0001-parse-dont-carve.md`).
//! * **Map via `previews.db`, never re-derive hashes**
//!   (`docs/adr/0002-map-via-previews-db.md`).

pub mod catalog;
pub mod error;
pub mod exif;
pub mod extract;
pub mod fs;
pub mod header;
pub mod jpeg;
pub mod lrprev;
// Mask export is a desktop feature: the app line does not carry it (#117), so
// the module is not compiled for `wasm32` at all. Everything else in the crate
// is, and `scripts/check.sh wasm` keeps it that way.
#[cfg(not(target_arch = "wasm32"))]
pub mod masks;
pub mod model;
pub mod plan;
pub mod previews_db;
pub mod report;
pub mod snapshot;

pub use error::{Error, Result};
pub use model::{CacheEntry, CatalogImage, Orientation, Pyramid, PyramidLevel, XmpMetadata};
pub use plan::{Payload, Plan, PlanItem, Status};

/// Version recorded by the shared writer and reports.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
