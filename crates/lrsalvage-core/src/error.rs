//! One error type for the whole crate.

use std::path::PathBuf;

pub type Result<T, E = Error> = std::result::Result<T, E>;

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

    #[error("preview payload not found: {path}")]
    PayloadMissing { path: PathBuf },

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
