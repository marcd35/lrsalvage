//! Plain data types shared by every module. No I/O, no formatting.

use std::path::PathBuf;

/// How many degrees the stored pixels must be turned clockwise to appear upright.
///
/// Lightroom stores previews *unrotated* and records the rotation in the
/// catalog (`Adobe_images.orientation` / `ImageCacheEntry.orientation`) with a
/// two-letter code. Verified mapping, cross-checked against
/// `hfiguiere/lrcat-extractor` and `Linwood-F/LRValidate`:
///
/// | catalog | degrees | EXIF Orientation |
/// |---------|---------|------------------|
/// | `AB`    | 0       | 1                |
/// | `BC`    | 90      | 6                |
/// | `CD`    | 180     | 3                |
/// | `DA`    | 270     | 8                |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    #[default]
    Normal,
    Rotate90,
    Rotate180,
    Rotate270,
    Unknown,
}

impl Orientation {
    /// Parse Lightroom's two-letter orientation code.
    pub fn from_catalog_code(code: &str) -> Self {
        match code.trim() {
            "AB" => Self::Normal,
            "BC" => Self::Rotate90,
            "CD" => Self::Rotate180,
            "DA" => Self::Rotate270,
            _ => Self::Unknown,
        }
    }

    /// Parse an EXIF Orientation value (`1, 3, 6, 8`).
    pub fn from_exif(value: u16) -> Self {
        match value {
            1 => Self::Normal,
            3 => Self::Rotate180,
            6 => Self::Rotate90,
            8 => Self::Rotate270,
            _ => Self::Unknown,
        }
    }

    /// The EXIF Orientation tag value to write for this rotation.
    pub fn exif_value(self) -> u16 {
        match self {
            Self::Normal | Self::Unknown => 1,
            Self::Rotate180 => 3,
            Self::Rotate90 => 6,
            Self::Rotate270 => 8,
        }
    }

    /// Clockwise rotation in degrees.
    pub fn degrees(self) -> u16 {
        match self {
            Self::Normal | Self::Unknown => 0,
            Self::Rotate90 => 90,
            Self::Rotate180 => 180,
            Self::Rotate270 => 270,
        }
    }

    pub fn is_upright(self) -> bool {
        matches!(self, Self::Normal | Self::Unknown)
    }
}

/// One row of `Adobe_images`, joined to its file and folder rows.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogImage {
    pub id_local: i64,
    /// User-visible name of a virtual copy; `None` for masters.
    pub copy_name: Option<String>,
    pub orientation: Orientation,
    pub file_format: String,
    /// Fully reconstructed path of the *original* (usually missing) file.
    pub original_path: PathBuf,
    pub root_folder: String,
    pub path_from_root: String,
    pub base_name: String,
    pub extension: String,
    /// Metadata from the catalog, filled in by the planner.
    pub xmp: Option<XmpMetadata>,
}

impl CatalogImage {
    /// Output file stem: virtual copies get `"<base> - <copyName>"`.
    pub fn output_stem(&self) -> String {
        match self.copy_name.as_deref() {
            Some(copy) if !copy.trim().is_empty() => {
                format!("{} - {}", self.base_name, copy.trim())
            }
            _ => self.base_name.clone(),
        }
    }

    /// Lightroom's own name for this image's root folder.
    ///
    /// `AgLibraryRootFolder.absolutePath` is *not* always a drive-level root:
    /// in real catalogs it frequently points at the leaf folder that was
    /// imported, with `pathFromRoot` left empty. Using its last path segment
    /// gives a stable, drive-letter-free group name in both cases — e.g.
    /// `C:/Users/marcc/Pictures/2023/12.31.23/` → `12.31.23`, and
    /// `F:/photog/2015/3. March/3.16 107 york street/` → `3.16 107 york street`.
    pub fn root_display_name(&self) -> String {
        self.root_folder
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default()
            .to_string()
    }

    /// Path of the recovered JPEG relative to the output root, mirroring the
    /// Lightroom folder tree as `<root folder>/<pathFromRoot>/<stem>.jpg`.
    pub fn relative_output_path(&self) -> PathBuf {
        let mut p = PathBuf::new();
        let root = self.root_display_name();
        if !root.is_empty() {
            p.push(root);
        }
        for part in self
            .path_from_root
            .split(['/', '\\'])
            .filter(|s| !s.is_empty())
        {
            p.push(part);
        }
        p.push(format!("{}.jpg", self.output_stem()));
        p
    }
}

/// One row of `previews.db.ImageCacheEntry`.
#[derive(Debug, Clone, PartialEq)]
pub struct CacheEntry {
    pub image_id: i64,
    pub uuid: String,
    pub digest: String,
    pub orientation: Option<String>,
}

/// One row of `previews.db.Pyramid`.
#[derive(Debug, Clone, PartialEq)]
pub struct Pyramid {
    pub uuid: String,
    pub digest: String,
    pub color_profile: String,
    pub quality: String,
    pub cropped_width: u32,
    pub cropped_height: u32,
}

/// One row of `previews.db.PyramidLevel`.
#[derive(Debug, Clone, PartialEq)]
pub struct PyramidLevel {
    pub uuid: String,
    pub level: u32,
    pub long_dimension: u32,
    pub width: u32,
    pub height: u32,
}

/// Metadata recovered from `Adobe_AdditionalMetadata.xmp` (zlib, 4-byte prefix).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct XmpMetadata {
    pub capture_time: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<u32>,
    pub exposure_time: Option<String>,
    pub f_number: Option<String>,
    pub focal_length: Option<String>,
    pub rating: Option<u8>,
    /// Decimal degrees, `(latitude, longitude)`.
    pub gps: Option<(f64, f64)>,
}
