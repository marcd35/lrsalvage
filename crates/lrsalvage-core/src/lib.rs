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
pub mod header;
pub mod jpeg;
pub mod lrprev;
pub mod masks;
pub mod model;
pub mod plan;
pub mod previews_db;
pub mod report;
pub mod snapshot;

pub use error::{Error, Result};
pub use model::{CacheEntry, CatalogImage, Orientation, Pyramid, PyramidLevel, XmpMetadata};
pub use plan::{Plan, PlanItem, Status};
