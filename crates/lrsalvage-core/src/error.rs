//! One error type for the whole crate.

use std::path::PathBuf;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// The reason a cache-only run cannot read its required index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexProblemKind {
    Missing,
    Unreadable,
    Unsupported,
}

impl IndexProblemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Unreadable => "unreadable",
            Self::Unsupported => "unsupported",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::Missing => "The preview folder is missing previews.db.",
            Self::Unreadable => "The preview index could not be read.",
            Self::Unsupported => "This preview index has an unsupported schema.",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("i/o error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("{path} is not a Lightroom catalog (no Adobe_images table)")]
    NotACatalog { path: PathBuf },

    #[error("{path} is not a preview cache directory")]
    NotAPreviewCache { path: PathBuf },

    #[error("preview index at {path}: {}", kind.message())]
    RequiredPreviewIndex {
        path: PathBuf,
        kind: IndexProblemKind,
        #[source]
        source: Option<Box<Error>>,
    },

    #[error("preview payload not found: {path}")]
    PayloadMissing { path: PathBuf },

    #[error("the plan names no certified preview")]
    PreviewUnavailable,

    #[error("{path} no longer carries the certified `{level}` chunk")]
    PreviewLevelMissing { path: PathBuf, level: String },

    #[error("payload is {}x{} but the plan certified {}x{}", actual.0, actual.1, expected.0, expected.1)]
    PreviewDimensionsChanged {
        expected: (u32, u32),
        actual: (u32, u32),
    },

    #[error("truncated .lrprev at byte {offset}: {detail}")]
    LrprevTruncated { offset: usize, detail: String },

    #[error("bad .lrprev chunk header: {0}")]
    LrprevHeader(String),

    #[error("pyramid header is not parseable: {0}")]
    PyramidHeader(String),

    #[error("no JPEG start-of-frame found")]
    JpegMalformed,

    #[error("jpeg start/end markers not found in payload")]
    JpegMarkersMissing,

    #[error("ICC profile of {0} bytes does not fit in 255 JPEG APP2 segments")]
    IccTooLarge(usize),

    #[error("refusing to write inside the catalog or preview cache: {0}")]
    UnsafeDestination(PathBuf),

    #[error(
        "the catalog changed while it was being copied: {path}\n  \
         sha256 of the catalog:  {source_sha256}\n  \
         sha256 of our snapshot: {snapshot_sha256}\n  \
         The copy is not the catalog, so nothing was planned from it. Retry, with Lightroom closed if it was open."
    )]
    SourceChanged {
        path: PathBuf,
        source_sha256: String,
        snapshot_sha256: String,
    },

    #[error("cannot use {path} as a destination: {source}")]
    Destination {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("xmp blob is not zlib-compressed as expected: {0}")]
    Xmp(String),

    #[error("png encode failed: {0}")]
    Png(String),
}

/// Attach a path to an [`std::io::Error`].
pub trait IoContext<T> {
    fn at(self, path: impl Into<PathBuf>) -> Result<T>;
}

impl<T> IoContext<T> for std::result::Result<T, std::io::Error> {
    fn at(self, path: impl Into<PathBuf>) -> Result<T> {
        self.map_err(|source| Error::Io {
            path: path.into(),
            source,
        })
    }
}
