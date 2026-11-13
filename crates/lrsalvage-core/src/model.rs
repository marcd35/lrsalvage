//! Plain data types shared by every module. No I/O, no formatting.

use std::path::{Component, Path, PathBuf};

/// Longest single path component this tool will emit. Well under every
/// filesystem's limit, and short enough that a truncated name still identifies
/// the photo.
const MAX_COMPONENT_CHARS: usize = 120;

/// Longest whole relative output path. A path longer than this is shortened
/// rather than handed to a filesystem that may refuse it.
const MAX_RELATIVE_PATH_CHARS: usize = 240;

fn char_count(text: &str) -> usize {
    text.chars().count()
}

/// Keep the first `limit` characters, counting characters rather than bytes so
/// a UTF-8 sequence is never cut in half.
fn truncate_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// Characters that cannot appear in a Windows file name, plus `/` and `\`
/// (which would turn one component into two).
fn is_illegal(ch: char) -> bool {
    matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '/' | '\\') || ch.is_control()
}

/// `CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`, with or without an
/// extension, in any case. Windows resolves these to devices, not files.
fn is_reserved_device_name(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end()
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let Some(rest) = stem.strip_prefix("COM").or_else(|| stem.strip_prefix("LPT")) else {
        return false;
    };
    matches!(rest.as_bytes(), [b'1'..=b'9'])
}

/// Turn one catalog-controlled string into one safe path component.
///
/// Returns `None` for anything that must not become a component at all: empty
/// or whitespace-only text, `.`/`..`, a path separator, an absolute path or a
/// drive prefix, and a Windows device name. Callers then skip the component or
/// fall back to a name of their own.
///
/// Anything containing an illegal character (see [`is_illegal`]) keeps its
/// meaning with `_` in place of the character: a name is never silently
/// dropped. The result is at most [`MAX_COMPONENT_CHARS`] characters.
pub fn sanitize_component(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    // One component only. The split has already happened by the time this is
    // called for a folder; a file name still carries its own separators.
    if trimmed.contains(['/', '\\']) {
        return None;
    }
    // `C:` or `C:name`: a drive prefix, even where the host would call it a
    // plain file name (POSIX). Checked before `:` is replaced with `_`.
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return None;
    }
    if Path::new(trimmed)
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }

    let mut cleaned = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        cleaned.push(if is_illegal(ch) { '_' } else { ch });
    }
    // Windows strips trailing dots and spaces from a name, so `photo.` and
    // `photo` would be one file. Do it here so the plan says what the
    // filesystem will do. Truncation never re-introduces one: it only removes
    // characters from the end.
    let cleaned = cleaned.trim_end_matches([' ', '.']);
    if cleaned.is_empty() || is_reserved_device_name(cleaned) {
        return None;
    }

    Some(truncate_chars(cleaned, MAX_COMPONENT_CHARS))
}

/// `<stem><suffix>.jpg`, with the stem shortened so the suffix and extension
/// always survive.
fn output_file_name(stem: &str, suffix: &str) -> String {
    const EXTENSION: &str = ".jpg";
    let tail = char_count(suffix) + EXTENSION.len();
    let budget = MAX_COMPONENT_CHARS.saturating_sub(tail);
    format!("{}{suffix}{EXTENSION}", truncate_chars(stem, budget))
}

/// Shrink the longest directories until their text fits `budget`, never below
/// one character, so every folder in the tree stays represented.
fn shrink_directories(directories: &mut [String], budget: usize) {
    loop {
        let total: usize = directories.iter().map(|name| char_count(name)).sum();
        if total <= budget {
            return;
        }
        let Some(index) = directories
            .iter()
            .enumerate()
            .max_by_key(|(_, name)| char_count(name))
            .map(|(index, _)| index)
        else {
            return;
        };
        let length = char_count(&directories[index]);
        if length <= 1 {
            return;
        }
        let take = (total - budget).min(length - 1);
        directories[index] = truncate_chars(&directories[index], length - take);
    }
}

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
    ///
    /// The stem is a path component and comes from catalog text, so it is
    /// sanitised: an image with no `AgLibraryFile` row (or a base name that is
    /// unusable as a file name) falls back to `image-<id_local>`.
    pub fn output_stem(&self) -> String {
        let base = sanitize_component(&self.base_name)
            .unwrap_or_else(|| format!("image-{}", self.id_local));
        match self.copy_name.as_deref() {
            Some(copy) if !copy.trim().is_empty() => match sanitize_component(copy) {
                Some(copy) => truncate_chars(&format!("{base} - {copy}"), MAX_COMPONENT_CHARS),
                None => base,
            },
            _ => base,
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
    ///
    /// Every component comes from catalog text and is sanitised: the result is
    /// one normal component per folder plus the file name, and is therefore
    /// always relative. Empty, `.`, `..`, absolute and drive-prefixed parts are
    /// dropped rather than joined.
    pub fn relative_output_path(&self) -> PathBuf {
        self.relative_output_path_with_suffix("")
    }

    /// The best level keeps `<stem>.jpg`; additional levels add `suffix`
    /// (e.g. `" - 101x68"`) before the extension.
    pub fn relative_output_path_with_suffix(&self, suffix: &str) -> PathBuf {
        let mut directories: Vec<String> = Vec::new();
        if let Some(root) = sanitize_component(&self.root_display_name()) {
            directories.push(root);
        }
        for part in self.path_from_root.split(['/', '\\']) {
            if let Some(part) = sanitize_component(part) {
                directories.push(part);
            }
        }

        let file = output_file_name(&self.output_stem(), suffix);

        // The file name is fixed; the folder tree makes room for it. Each
        // separator costs one character too.
        let mut budget =
            MAX_RELATIVE_PATH_CHARS.saturating_sub(char_count(&file) + directories.len());
        // If even one character per folder does not fit, drop the shallowest
        // folders: the ones nearest the photo are the ones worth keeping.
        while directories.len() > 1 && budget < directories.len() {
            directories.remove(0);
            budget = MAX_RELATIVE_PATH_CHARS.saturating_sub(char_count(&file) + directories.len());
        }
        shrink_directories(&mut directories, budget);

        let mut path = PathBuf::new();
        for directory in &directories {
            path.push(directory);
        }
        path.push(file);
        path
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

#[cfg(test)]
mod tests {
    use std::path::{Component, Path, PathBuf};

    use super::*;

    /// A catalog image rooted at `F:/photog/2015/`, so its output tree starts
    /// with `2015/`.
    fn image(id: i64, base_name: &str) -> CatalogImage {
        CatalogImage {
            id_local: id,
            copy_name: None,
            orientation: Orientation::Normal,
            file_format: "CR2".to_string(),
            original_path: PathBuf::from("F:/photog/2015/IMG.CR2"),
            root_folder: "F:/photog/2015/".to_string(),
            path_from_root: String::new(),
            base_name: base_name.to_string(),
            extension: "CR2".to_string(),
            xmp: None,
        }
    }

    fn slashed(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    #[test]
    fn a_component_that_could_leave_the_output_root_is_rejected() {
        for hostile in [
            "", " ", ".", "..", "...", "./x", "../x", "a/b", "a\\b", "/etc", "\\windows", "C:",
            "C:\\x", "C:x", "C:/x",
        ] {
            assert_eq!(sanitize_component(hostile), None, "{hostile:?}");
        }
    }

    #[test]
    fn windows_device_names_are_rejected_in_any_case() {
        for reserved in [
            "CON", "con", "Con", "PRN", "aux", "NUL", "COM1", "com9", "LPT1", "lpt9", "CON.jpg",
            "aux.txt",
        ] {
            assert_eq!(sanitize_component(reserved), None, "{reserved}");
        }

        // Only the nine device numbers are reserved, and only as whole names.
        for allowed in ["COM0", "COM10", "LPT0", "CONSOLE", "CONS"] {
            assert_eq!(
                sanitize_component(allowed).as_deref(),
                Some(allowed),
                "{allowed}"
            );
        }
    }

    #[test]
    fn illegal_characters_are_replaced_not_dropped() {
        assert_eq!(
            sanitize_component("IMG<01>:x\"y|z?*w.jpg").as_deref(),
            Some("IMG_01__x_y_z__w.jpg")
        );
        assert_eq!(
            sanitize_component("bell\u{7} tab\tname").as_deref(),
            Some("bell_ tab_name")
        );
        // Windows silently strips trailing dots and spaces; do it ourselves.
        assert_eq!(sanitize_component("photo ").as_deref(), Some("photo"));
        assert_eq!(sanitize_component("photo.").as_deref(), Some("photo"));
        assert_eq!(sanitize_component("photo. ").as_deref(), Some("photo"));
    }

    #[test]
    fn a_hostile_path_from_root_cannot_escape_the_output_tree() {
        let mut hostile = image(1, "IMG_0001");
        hostile.path_from_root = "../../../escaped/".to_string();

        let relative = hostile.relative_output_path();
        assert_eq!(slashed(&relative), "2015/escaped/IMG_0001.jpg");
        assert!(
            relative
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
            "no component may be parent, current or root: {relative:?}"
        );
    }

    #[test]
    fn absolute_and_drive_prefixed_parts_are_dropped() {
        let mut hostile = image(1, "IMG");
        hostile.path_from_root = "C:/Users/evil/".to_string();
        assert_eq!(slashed(&hostile.relative_output_path()), "2015/Users/evil/IMG.jpg");

        let mut unc = image(1, "IMG");
        unc.path_from_root = "\\\\server\\share\\".to_string();
        // A UNC path has no meaning relative to the output root; its parts are
        // ordinary folder names once split.
        assert_eq!(
            slashed(&unc.relative_output_path()),
            "2015/server/share/IMG.jpg"
        );
    }

    #[test]
    fn a_file_name_with_a_separator_falls_back_to_the_image_id() {
        let mut hostile = image(17, "..\\..\\evil");
        assert_eq!(hostile.output_stem(), "image-17");
        assert_eq!(slashed(&hostile.relative_output_path()), "2015/image-17.jpg");

        // A copy name is untrusted too: it must not smuggle in a separator.
        hostile.copy_name = Some("../../x".to_string());
        assert_eq!(hostile.output_stem(), "image-17");
    }

    #[test]
    fn an_image_without_a_file_row_is_named_after_its_id() {
        let orphan = image(42, "");
        assert_eq!(orphan.output_stem(), "image-42");
        assert_eq!(slashed(&orphan.relative_output_path()), "2015/image-42.jpg");

        let mut copy = orphan;
        copy.copy_name = Some("Copy 1".to_string());
        assert_eq!(copy.output_stem(), "image-42 - Copy 1");
        assert_eq!(
            slashed(&copy.relative_output_path()),
            "2015/image-42 - Copy 1.jpg"
        );
    }

    #[test]
    fn long_names_are_truncated_without_losing_the_extension() {
        let long = image(1, &"N".repeat(400));
        let relative = long.relative_output_path();
        let file = relative.file_name().expect("file name").to_string_lossy();
        assert_eq!(file.chars().count(), 120, "{file}");
        assert!(file.ends_with(".jpg"), "{file}");
        assert!(file.starts_with("NNNN"), "{file}");

        let mut deep_name = image(2, "IMG");
        deep_name.path_from_root = format!("{}/", "d".repeat(300));
        let relative = deep_name.relative_output_path();
        let folder = relative
            .parent()
            .and_then(|parent| parent.file_name())
            .expect("folder component")
            .to_string_lossy();
        assert_eq!(folder.chars().count(), 120, "{folder}");

        // A name that only differs from a Windows-stripped one by trailing
        // dots must not become ambiguous.
        let dotted = image(3, &format!("photo{}", ".".repeat(200)));
        assert_eq!(
            dotted.relative_output_path().file_name().unwrap(),
            "photo.jpg"
        );
    }

    #[test]
    fn a_deep_tree_is_shortened_so_the_whole_relative_path_stays_within_budget() {
        let mut deep = image(1, "IMG_0001");
        deep.path_from_root = (1..=40).map(|n| format!("folder{n:02}/")).collect();

        let relative = deep.relative_output_path();
        assert!(
            slashed(&relative).chars().count() <= 240,
            "{} chars: {}",
            slashed(&relative).chars().count(),
            slashed(&relative)
        );
        // The file keeps its own name; the folders make room for it.
        assert_eq!(
            relative.file_name().unwrap().to_string_lossy(),
            "IMG_0001.jpg"
        );
        assert_eq!(
            relative.components().count(),
            42,
            "the root folder and every subfolder still appear in the path"
        );
        for component in relative.components() {
            let text = component.as_os_str().to_string_lossy().into_owned();
            assert!(text.chars().count() <= 120, "{text}");
        }
    }

    #[test]
    fn a_level_suffix_is_added_before_the_extension() {
        let mut image = image(1, "IMG_0001");
        image.copy_name = Some("Copy 1".to_string());
        assert_eq!(
            slashed(&image.relative_output_path_with_suffix(" - 101x68")),
            "2015/IMG_0001 - Copy 1 - 101x68.jpg"
        );
        // The suffix must not survive the truncation that keeps the extension.
        let long = self::image(2, &"N".repeat(400));
        let suffix = long.relative_output_path_with_suffix(" - 101x68");
        let file = suffix.file_name().unwrap().to_string_lossy();
        assert_eq!(file.chars().count(), 120, "{file}");
        assert!(file.ends_with(" - 101x68.jpg"), "{file}");
    }

    #[test]
    fn the_relative_path_is_always_relative() {
        for (root, from_root, base) in [
            ("C:/", "", "IMG"),
            ("C:/", "../..", "IMG"),
            ("", "../../..", "IMG"),
            ("F:/a/b/", "x/../../../y", "IMG"),
            ("F:/a/b/", "C:/x/", "IMG"),
        ] {
            let mut image = image(1, base);
            image.root_folder = root.to_string();
            image.path_from_root = from_root.to_string();
            let relative = image.relative_output_path();
            assert!(!relative.is_absolute(), "{root} {from_root}: {relative:?}");
            assert!(
                relative
                    .components()
                    .all(|component| matches!(component, Component::Normal(_))),
                "{root} {from_root}: {relative:?}"
            );
        }
    }
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
