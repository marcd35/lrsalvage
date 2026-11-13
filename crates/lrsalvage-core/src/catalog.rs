//! Read-only queries against the Lightroom catalog (`*.lrcat`).

use std::io::Read;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};

use crate::error::{Error, Result};
use crate::model::{CatalogImage, Orientation, XmpMetadata};

/// A catalog opened read-only, on top of a stable [`crate::snapshot`] copy.
pub struct Catalog {
    conn: Connection,
    has_metadata: bool,
}

impl Catalog {
    /// Open a catalog snapshot. Never mutates the caller's file.
    pub fn from_snapshot(snapshot: &crate::snapshot::CatalogSnapshot) -> Result<Self> {
        let conn = crate::snapshot::open_readonly(snapshot.path())?;

        if !table_exists(&conn, "Adobe_images")? {
            return Err(Error::NotACatalog {
                path: snapshot.path().to_path_buf(),
            });
        }

        // Old catalogs predate `Adobe_AdditionalMetadata`; metadata is a bonus,
        // never a reason to refuse a salvage.
        let has_metadata = table_exists(&conn, "Adobe_AdditionalMetadata")?;

        Ok(Self { conn, has_metadata })
    }

    /// Number of rows in `Adobe_images`.
    pub fn image_count(&self) -> Result<u64> {
        let count: i64 = self
            .conn
            .query_row("SELECT count(*) FROM Adobe_images", [], |row| row.get(0))?;
        Ok(count.max(0) as u64)
    }

    /// Every image, joined to `AgLibraryFile` / `AgLibraryFolder` /
    /// `AgLibraryRootFolder`, with the original path reconstructed.
    pub fn images(&self) -> Result<Vec<CatalogImage>> {
        // LEFT JOIN, not JOIN: an image whose file row was deleted still has a
        // preview worth salvaging, so it must not vanish from the list.
        let mut stmt = self.conn.prepare(
            "SELECT i.id_local, i.copyName, i.orientation, i.fileFormat, \
                    rf.absolutePath, fo.pathFromRoot, fi.baseName, fi.extension \
             FROM Adobe_images i \
             LEFT JOIN AgLibraryFile fi ON fi.id_local = i.rootFile \
             LEFT JOIN AgLibraryFolder fo ON fo.id_local = fi.folder \
             LEFT JOIN AgLibraryRootFolder rf ON rf.id_local = fo.rootFolder \
             ORDER BY i.id_local",
        )?;

        let rows = stmt.query_map([], |row| {
            let copy_name: Option<String> = row.get(1)?;
            let orientation: Option<String> = row.get(2)?;
            let file_format: Option<String> = row.get(3)?;
            let root_folder: Option<String> = row.get(4)?;
            let path_from_root: Option<String> = row.get(5)?;
            let base_name: Option<String> = row.get(6)?;
            let extension: Option<String> = row.get(7)?;

            let base_name = base_name.unwrap_or_default();
            let extension = extension.unwrap_or_default();

            Ok(CatalogImage {
                id_local: row.get(0)?,
                copy_name,
                orientation: orientation
                    .as_deref()
                    .map(Orientation::from_catalog_code)
                    .unwrap_or(Orientation::Unknown),
                file_format: file_format.unwrap_or_default(),
                original_path: original_path(
                    root_folder.as_deref(),
                    path_from_root.as_deref(),
                    &base_name,
                    &extension,
                ),
                root_folder: root_folder.unwrap_or_default(),
                path_from_root: path_from_root.unwrap_or_default(),
                base_name,
                extension,
                xmp: None,
            })
        })?;

        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Metadata from `Adobe_AdditionalMetadata.xmp` for one image.
    pub fn xmp(&self, image_id: i64) -> Result<Option<XmpMetadata>> {
        if !self.has_metadata {
            return Ok(None);
        }

        // `OptionalExtension` separates "no row for this image" from the row
        // itself, and the inner Option covers a NULL blob.
        let blob: Option<Option<Vec<u8>>> = self
            .conn
            .query_row(
                "SELECT xmp FROM Adobe_AdditionalMetadata WHERE image = ?1 LIMIT 1",
                [image_id],
                |row| row.get(0),
            )
            .optional()?;

        match blob.flatten() {
            None => Ok(None),
            Some(blob) => Ok(Some(parse_xmp(&inflate_xmp(&blob)?))),
        }
    }

    /// Convenience for callers that only have a path.
    pub fn open(path: &Path) -> Result<(crate::snapshot::CatalogSnapshot, Self)> {
        let snapshot = crate::snapshot::CatalogSnapshot::capture(path)?;
        let catalog = Self::from_snapshot(&snapshot)?;
        Ok((snapshot, catalog))
    }
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [name],
        |row| row.get(0),
    )?;
    Ok(exists)
}

/// Join `absolutePath`, `pathFromRoot` and the file name into a host path.
///
/// Lightroom stores `C:/Users/Marc/Pictures/` with POSIX separators and a
/// trailing slash; Windows accepts both separators, so the strings are kept as
/// stored and only joined component-wise.
fn original_path(
    root_folder: Option<&str>,
    path_from_root: Option<&str>,
    base_name: &str,
    extension: &str,
) -> PathBuf {
    let mut path = PathBuf::new();

    if let Some(root) = root_folder {
        let root = root.trim_end_matches(['/', '\\']);
        if !root.is_empty() {
            path.push(root);
        }
    }

    if let Some(from_root) = path_from_root {
        for part in from_root.split(['/', '\\']).filter(|part| !part.is_empty()) {
            path.push(part);
        }
    }

    let file_name = match (base_name.is_empty(), extension.is_empty()) {
        (true, _) => String::new(),
        (false, true) => base_name.to_string(),
        (false, false) => format!("{base_name}.{extension}"),
    };
    if !file_name.is_empty() {
        path.push(file_name);
    }

    path
}

/// The stored blob is a 4-byte prefix followed by a zlib stream of XMP XML.
fn inflate_xmp(blob: &[u8]) -> Result<String> {
    if blob.len() <= 4 {
        return Err(Error::Xmp(format!(
            "blob is {} bytes, too short for the 4-byte prefix plus zlib data",
            blob.len()
        )));
    }

    let mut xml = Vec::new();
    flate2::read::ZlibDecoder::new(&blob[4..])
        .read_to_end(&mut xml)
        .map_err(|err| Error::Xmp(format!("zlib decompression failed: {err}")))?;

    String::from_utf8(xml).map_err(|err| Error::Xmp(format!("xmp is not UTF-8: {err}")))
}

/// Parse the few XMP fields worth restoring, by string search: an XML crate
/// would be a large dependency for a dozen attributes whose spelling is fixed
/// by Adobe's schema.
fn parse_xmp(xml: &str) -> XmpMetadata {
    let datetime_original = attribute(xml, "exif:DateTimeOriginal");
    let create_date = attribute(xml, "xmp:CreateDate");
    let capture_time = datetime_original
        .as_deref()
        .and_then(normalise_timestamp)
        .or_else(|| create_date.as_deref().and_then(normalise_timestamp));

    XmpMetadata {
        capture_time,
        make: attribute(xml, "tiff:Make"),
        model: attribute(xml, "tiff:Model"),
        lens: attribute(xml, "aux:Lens").or_else(|| attribute(xml, "exifEX:LensModel")),
        iso: iso_speed(xml),
        exposure_time: attribute(xml, "exif:ExposureTime"),
        f_number: attribute(xml, "exif:FNumber"),
        focal_length: attribute(xml, "exif:FocalLength"),
        rating: attribute(xml, "xmp:Rating").and_then(|value| value.parse::<u8>().ok()),
        gps: gps(xml),
    }
}

/// `exif:ISOSpeedRatings` is usually an element wrapping `<rdf:li>`.
fn iso_speed(xml: &str) -> Option<u32> {
    if let Some(value) = attribute(xml, "exif:ISOSpeedRatings") {
        return value.parse().ok();
    }

    let start = xml.find("<exif:ISOSpeedRatings")?;
    let rest = &xml[start..];
    let item = rest.find("<rdf:li")?;
    let rest = &rest[item..];
    let open_end = rest.find('>')? + 1;
    let text = &rest[open_end..];
    let close = text.find("</rdf:li>")?;
    text[..close].trim().parse().ok()
}

fn gps(xml: &str) -> Option<(f64, f64)> {
    let latitude = coordinate(xml, "exif:GPSLatitude", "exif:GPSLatitudeRef")?;
    let longitude = coordinate(xml, "exif:GPSLongitude", "exif:GPSLongitudeRef")?;
    Some((latitude, longitude))
}

fn coordinate(xml: &str, value_name: &str, ref_name: &str) -> Option<f64> {
    let raw = attribute(xml, value_name)?;
    let reference = attribute(xml, ref_name);
    parse_coordinate(&raw, reference.as_deref())
}

/// Accept `deg,min,sec` as either floats, rationals (`41/1,19/1,408/100`) or
/// a value ending in its hemisphere (`41,19.68S`), with the hemisphere also
/// possibly in a separate `...Ref` attribute.
fn parse_coordinate(raw: &str, reference: Option<&str>) -> Option<f64> {
    let mut hemisphere = reference
        .map(|reference| reference.trim().to_ascii_uppercase())
        .unwrap_or_default();

    let mut parts = Vec::new();
    for part in raw.split(',') {
        let mut part = part.trim();
        if let Some(last) = part.chars().last() {
            let upper = last.to_ascii_uppercase();
            if matches!(upper, 'N' | 'S' | 'E' | 'W') {
                if hemisphere.is_empty() {
                    hemisphere = upper.to_string();
                }
                part = part[..part.len() - last.len_utf8()].trim();
            }
        }
        if part.is_empty() {
            continue;
        }
        parts.push(parse_number(part)?);
    }

    let degrees = match parts.as_slice() {
        [] => return None,
        [degrees] => *degrees,
        [degrees, minutes] => degrees + minutes / 60.0,
        [degrees, minutes, seconds] => degrees + minutes / 60.0 + seconds / 3600.0,
        _ => return None,
    };

    Some(if hemisphere == "S" || hemisphere == "W" {
        -degrees
    } else {
        degrees
    })
}

fn parse_number(text: &str) -> Option<f64> {
    match text.split_once('/') {
        Some((numerator, denominator)) => {
            let numerator: f64 = numerator.trim().parse().ok()?;
            let denominator: f64 = denominator.trim().parse().ok()?;
            if denominator == 0.0 {
                return None;
            }
            Some(numerator / denominator)
        }
        None => text.trim().parse().ok(),
    }
}

/// Find `name="value"` (single or double quotes), falling back to
/// `<name>value</name>` for the few fields Lightroom writes as elements.
fn attribute(xml: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let needle = format!("{name}={quote}");
        let Some(start) = xml.find(&needle) else {
            continue;
        };
        let rest = &xml[start + needle.len()..];
        let Some(end) = rest.find(quote) else {
            continue;
        };
        let value = unescape(rest[..end].trim());
        return (!value.is_empty()).then_some(value);
    }

    element_text(xml, name)
}

fn element_text(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut search = 0;

    while let Some(offset) = xml[search..].find(&open) {
        let start = search + offset;
        let after_name = start + open.len();
        // `<exif:GPSLatitude` must not match `<exif:GPSLatitudeRef`.
        let next = xml[after_name..].chars().next();
        if !matches!(
            next,
            Some('>') | Some('/') | Some(' ') | Some('\t') | Some('\n') | Some('\r')
        ) {
            search = after_name;
            continue;
        }

        let open_end = after_name + xml[after_name..].find('>')? + 1;
        let rest = &xml[open_end..];
        let end = rest.find(&close)?;
        let value = unescape(rest[..end].trim());
        // Structured values (e.g. an `rdf:Seq`) are handled by their own
        // accessor; they are not plain text.
        return (!value.is_empty() && !value.contains('<')).then_some(value);
    }

    None
}

/// XMP is XML, so attribute values carry XML entities.
fn unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Reduce an XMP date-time to `YYYY-MM-DDTHH:MM:SS`.
///
/// Adobe writes fractional seconds and timezone offsets that carry no meaning
/// for a recovered JPEG; EXIF-style colon-separated dates are tolerated too.
fn normalise_timestamp(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let split = raw.find(['T', ' '])?;
    let (date, rest) = raw.split_at(split);
    let time = rest[1..].trim();

    let date = date.trim();
    if !is_date(date) {
        return None;
    }
    let date = date.replace(':', "-");

    let bytes = time.as_bytes();
    if bytes.len() < 8 {
        return None;
    }
    let time = &time[..8];
    let looks_like_time = time
        .bytes()
        .enumerate()
        .all(|(index, byte)| index == 2 || index == 5 || byte.is_ascii_digit())
        && time.as_bytes()[2] == b':'
        && time.as_bytes()[5] == b':';
    if !looks_like_time {
        return None;
    }

    Some(format!("{date}T{time}"))
}

fn is_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == bytes[7]
        && (bytes[4] == b'-' || bytes[4] == b':')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::{Path, PathBuf};

    use rusqlite::Connection;

    use crate::error::Error;
    use crate::model::{CatalogImage, Orientation, XmpMetadata};
    use crate::previews_db::PreviewIndex;
    use crate::snapshot::CatalogSnapshot;

    use super::Catalog;

    const SCHEMA: &str = r#"
        CREATE TABLE Adobe_images (id_local INTEGER PRIMARY KEY, copyName TEXT,
          orientation TEXT, fileFormat TEXT, rootFile INTEGER);
        CREATE TABLE AgLibraryFile (id_local INTEGER PRIMARY KEY, folder INTEGER,
          baseName TEXT, extension TEXT);
        CREATE TABLE AgLibraryFolder (id_local INTEGER PRIMARY KEY, pathFromRoot TEXT,
          rootFolder INTEGER);
        CREATE TABLE AgLibraryRootFolder (id_local INTEGER PRIMARY KEY, absolutePath TEXT);
        CREATE TABLE Adobe_AdditionalMetadata (image INTEGER, xmp BLOB);
    "#;

    /// Build a catalog file, then open it the way the tool does: through a
    /// snapshot copy.
    fn snapshot_with(seed: impl FnOnce(&Connection)) -> CatalogSnapshot {
        snapshot_from(SCHEMA, seed)
    }

    /// Same, but with no tables at all: for catalogs that lack a schema.
    fn bare_snapshot_with(seed: impl FnOnce(&Connection)) -> CatalogSnapshot {
        snapshot_from("", seed)
    }

    fn snapshot_from(schema: &str, seed: impl FnOnce(&Connection)) -> CatalogSnapshot {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.lrcat");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(schema).unwrap();
        seed(&conn);
        drop(conn);
        CatalogSnapshot::capture(&path).unwrap()
    }

    fn standard_images(conn: &Connection) {
        conn.execute_batch(
            r#"
            INSERT INTO AgLibraryRootFolder (id_local, absolutePath)
              VALUES (1, 'C:/Users/Marc/Pictures/');
            INSERT INTO AgLibraryFolder (id_local, pathFromRoot, rootFolder)
              VALUES (10, '2023/12.31.23/', 1), (11, '', 1);
            INSERT INTO AgLibraryFile (id_local, folder, baseName, extension)
              VALUES (100, 10, 'DSC00876', 'ARW'), (101, 11, 'DSC00877', 'JPG');
            INSERT INTO Adobe_images (id_local, copyName, orientation, fileFormat, rootFile)
              VALUES (1, NULL, 'AB', 'ARW', 100),
                     (2, 'Copy 1', 'BC', 'JPG', 101),
                     (3, NULL, NULL, NULL, NULL);
            "#,
        )
        .unwrap();
    }

    fn catalog_with(seed: impl FnOnce(&Connection)) -> Catalog {
        let snapshot = snapshot_with(seed);
        Catalog::from_snapshot(&snapshot).unwrap()
    }

    /// zlib-compress `xml` behind the 4-byte prefix Lightroom uses.
    fn xmp_blob(xml: &str) -> Vec<u8> {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(xml.as_bytes()).unwrap();
        let compressed = encoder.finish().unwrap();
        let mut blob = vec![0u8; 4];
        blob.extend_from_slice(&compressed);
        blob
    }

    fn add_xmp(conn: &Connection, image: i64, xml: &str) {
        conn.execute(
            "INSERT INTO Adobe_AdditionalMetadata (image, xmp) VALUES (?1, ?2)",
            rusqlite::params![image, xmp_blob(xml)],
        )
        .unwrap();
    }

    fn image(images: &[CatalogImage], id: i64) -> &CatalogImage {
        images.iter().find(|i| i.id_local == id).unwrap()
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-6,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn every_catalog_image_appears_even_without_a_file_row() {
        let catalog = catalog_with(standard_images);

        let images = catalog.images().unwrap();
        assert_eq!(catalog.image_count().unwrap(), 3);
        assert_eq!(images.len(), 3, "LEFT JOIN must not drop orphan images");

        let orphan = image(&images, 3);
        assert_eq!(orphan.base_name, "");
        assert_eq!(orphan.extension, "");
        assert_eq!(orphan.original_path, PathBuf::new());
        assert_eq!(orphan.orientation, Orientation::Unknown);
    }

    #[test]
    fn reconstructs_paths_from_root_folder_path_from_root_and_file() {
        let catalog = catalog_with(standard_images);
        let images = catalog.images().unwrap();

        let deep = image(&images, 1);
        assert_eq!(
            deep.original_path,
            PathBuf::from("C:/Users/Marc/Pictures/2023/12.31.23/DSC00876.ARW")
        );
        assert_eq!(deep.base_name, "DSC00876");
        assert_eq!(deep.extension, "ARW");
        assert_eq!(deep.path_from_root, "2023/12.31.23/");
        assert_eq!(deep.root_folder, "C:/Users/Marc/Pictures/");
        assert_eq!(deep.file_format, "ARW");

        // Empty pathFromRoot must not leave a doubled separator behind.
        assert_eq!(
            image(&images, 2).original_path,
            PathBuf::from("C:/Users/Marc/Pictures/DSC00877.JPG")
        );
    }

    #[test]
    fn orientation_codes_are_mapped_and_null_becomes_unknown() {
        let catalog = catalog_with(standard_images);
        let images = catalog.images().unwrap();

        assert_eq!(image(&images, 1).orientation, Orientation::Normal);
        assert_eq!(image(&images, 2).orientation, Orientation::Rotate90);
        assert_eq!(image(&images, 3).orientation, Orientation::Unknown);
    }

    #[test]
    fn copy_name_is_kept_for_virtual_copies() {
        let catalog = catalog_with(standard_images);
        let images = catalog.images().unwrap();

        assert_eq!(image(&images, 1).copy_name, None);
        assert_eq!(image(&images, 2).copy_name.as_deref(), Some("Copy 1"));
        // Root folder is `C:/Users/Marc/Pictures/`, so the mirror groups by
        // its last segment, then the path below it.
        assert_eq!(
            image(&images, 2).relative_output_path(),
            PathBuf::from("Pictures/DSC00877 - Copy 1.jpg")
        );
    }

    #[test]
    fn database_without_adobe_images_is_not_a_catalog() {
        let snapshot = bare_snapshot_with(|conn| {
            conn.execute_batch("CREATE TABLE SomethingElse (x INTEGER);")
                .unwrap();
        });

        match Catalog::from_snapshot(&snapshot) {
            Err(Error::NotACatalog { path }) => assert_eq!(path, snapshot.path()),
            other => panic!("expected Error::NotACatalog, got {:?}", other.err()),
        }
    }

    const FULL_XMP: &str = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:exif="http://ns.adobe.com/exif/1.0/"
    xmlns:tiff="http://ns.adobe.com/tiff/1.0/"
    xmlns:aux="http://ns.adobe.com/exif/1.0/aux/"
    xmp:CreateDate="2010-01-02T03:04:05"
    exif:DateTimeOriginal="2009-08-06T17:13:24.5+02:00"
    tiff:Make="SONY"
    tiff:Model="DSLR-A350"
    aux:Lens="DT 18-55mm F3.5-5.6 SAM"
    exif:FNumber="28/10"
    exif:ExposureTime="1/250"
    exif:FocalLength="350/10"
    xmp:Rating="4"
    exif:GPSLatitude="41/1,19/1,408/100"
    exif:GPSLatitudeRef="N"
    exif:GPSLongitude="2/1,10/1,3000/100"
    exif:GPSLongitudeRef="E">
   <exif:ISOSpeedRatings>
    <rdf:Seq><rdf:li>400</rdf:li></rdf:Seq>
   </exif:ISOSpeedRatings>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#;

    #[test]
    fn xmp_is_decompressed_and_every_field_is_extracted() {
        let catalog = catalog_with(|conn| {
            standard_images(conn);
            add_xmp(conn, 1, FULL_XMP);
        });

        let xmp = catalog.xmp(1).unwrap().expect("xmp row");
        assert_eq!(xmp.make.as_deref(), Some("SONY"));
        assert_eq!(xmp.model.as_deref(), Some("DSLR-A350"));
        assert_eq!(xmp.lens.as_deref(), Some("DT 18-55mm F3.5-5.6 SAM"));
        assert_eq!(xmp.iso, Some(400));
        assert_eq!(xmp.exposure_time.as_deref(), Some("1/250"));
        assert_eq!(xmp.focal_length.as_deref(), Some("350/10"));
        assert_eq!(xmp.f_number.as_deref(), Some("28/10"));
        assert_eq!(xmp.rating, Some(4));
        assert_eq!(xmp.capture_time.as_deref(), Some("2009-08-06T17:13:24"));

        let (lat, lon) = xmp.gps.expect("gps");
        assert_close(lat, 41.0 + 19.0 / 60.0 + 4.08 / 3600.0);
        assert_close(lon, 2.0 + 10.0 / 60.0 + 30.0 / 3600.0);
    }

    #[test]
    fn gps_accepts_the_hemisphere_suffix_inside_the_value() {
        let xml = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF><rdf:Description
            xmlns:exif="http://ns.adobe.com/exif/1.0/"
            exif:GPSLatitude="41,19.68S"
            exif:GPSLongitude="2,10.5W"/></rdf:RDF></x:xmpmeta>"#;
        let catalog = catalog_with(|conn| {
            standard_images(conn);
            add_xmp(conn, 1, xml);
        });

        let (lat, lon) = catalog.xmp(1).unwrap().unwrap().gps.expect("gps");
        assert_close(lat, -(41.0 + 19.68 / 60.0));
        assert_close(lon, -(2.0 + 10.5 / 60.0));
    }

    #[test]
    fn capture_time_falls_back_to_create_date_and_is_normalised() {
        let xml = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF><rdf:Description
            xmlns:xmp="http://ns.adobe.com/xap/1.0/"
            xmp:CreateDate="2010-01-02T03:04:05.75-08:00"/></rdf:RDF></x:xmpmeta>"#;
        let catalog = catalog_with(|conn| {
            standard_images(conn);
            add_xmp(conn, 1, xml);
        });

        assert_eq!(
            catalog.xmp(1).unwrap().unwrap().capture_time.as_deref(),
            Some("2010-01-02T03:04:05")
        );
    }

    #[test]
    fn unparseable_metadata_fields_become_none_without_failing() {
        let xml = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF><rdf:Description
            xmlns:xmp="http://ns.adobe.com/xap/1.0/"
            xmp:CreateDate="not a date"
            xmp:Rating="unknown"
            exif:GPSLatitude="somewhere"/></rdf:RDF></x:xmpmeta>"#;
        let catalog = catalog_with(|conn| {
            standard_images(conn);
            add_xmp(conn, 1, xml);
        });

        let xmp = catalog.xmp(1).unwrap().unwrap();
        assert_eq!(xmp, XmpMetadata::default());
    }

    #[test]
    fn missing_or_null_xmp_is_none() {
        let catalog = catalog_with(|conn| {
            standard_images(conn);
            conn.execute(
                "INSERT INTO Adobe_AdditionalMetadata (image, xmp) VALUES (1, NULL)",
                [],
            )
            .unwrap();
        });

        assert_eq!(catalog.xmp(1).unwrap(), None);
        assert_eq!(catalog.xmp(2).unwrap(), None, "image 2 has no row");
        assert_eq!(catalog.xmp(99).unwrap(), None, "unknown image");
    }

    #[test]
    fn catalog_without_metadata_table_reports_no_xmp() {
        let snapshot = bare_snapshot_with(|conn| {
            conn.execute_batch(
                r#"
                CREATE TABLE Adobe_images (id_local INTEGER PRIMARY KEY, copyName TEXT,
                  orientation TEXT, fileFormat TEXT, rootFile INTEGER);
                INSERT INTO Adobe_images (id_local, copyName, orientation, fileFormat, rootFile)
                  VALUES (1, NULL, 'AB', 'JPG', NULL);
                "#,
            )
            .unwrap();
        });
        let catalog = Catalog::from_snapshot(&snapshot).unwrap();

        assert_eq!(catalog.xmp(1).unwrap(), None);
    }

    #[test]
    fn undecompressable_xmp_blob_is_an_xmp_error() {
        let catalog = catalog_with(|conn| {
            standard_images(conn);
            conn.execute(
                "INSERT INTO Adobe_AdditionalMetadata (image, xmp) VALUES (1, ?1)",
                rusqlite::params![vec![0u8, 0, 0, 0, 1, 2, 3, 4, 5]],
            )
            .unwrap();
        });

        match catalog.xmp(1) {
            Err(Error::Xmp(_)) => {}
            other => panic!("expected Error::Xmp, got {other:?}"),
        }
    }

    #[test]
    #[ignore = "needs a real Lightroom catalog; set LRSALVAGE_TEST_CATALOG and LRSALVAGE_TEST_PREVIEWS"]
    fn real_catalog_matches_the_preview_cache() {
        let catalog_path = std::env::var("LRSALVAGE_TEST_CATALOG")
            .expect("set LRSALVAGE_TEST_CATALOG to a .lrcat");
        let cache_root = PathBuf::from(
            std::env::var("LRSALVAGE_TEST_PREVIEWS").expect("set LRSALVAGE_TEST_PREVIEWS"),
        );

        let (snapshot, catalog) = Catalog::open(Path::new(&catalog_path)).unwrap();
        let images = catalog.images().unwrap();
        assert_eq!(
            images.len() as u64,
            catalog.image_count().unwrap(),
            "images() must cover every Adobe_images row"
        );
        assert_eq!(images.len(), 78, "expected 78 images");

        let index = PreviewIndex::open(&cache_root.join("previews.db")).unwrap();
        assert_eq!(index.entries.len(), 78, "expected 78 cache entries");

        let mut with_xmp = 0;
        let mut sony = 0;
        for image in &images {
            let entry = index
                .entries
                .get(&image.id_local)
                .unwrap_or_else(|| panic!("no cache entry for image {}", image.id_local));
            let payload = index.payload_path(&cache_root, entry);
            assert!(payload.is_file(), "missing payload {}", payload.display());

            let pyramid = index
                .pyramids
                .get(&entry.uuid)
                .unwrap_or_else(|| panic!("no Pyramid row for uuid {}", entry.uuid));
            assert_eq!(pyramid.cropped_width, 1616, "cropped width");
            assert_eq!(pyramid.cropped_height, 1077, "cropped height");
            let best = index.best_level(&entry.uuid).expect("pyramid level");
            assert_eq!(best.width, 1616);
            assert_eq!(best.height, 1077);

            if let Some(xmp) = catalog.xmp(image.id_local).unwrap() {
                with_xmp += 1;
                if xmp.make.as_deref() == Some("SONY") {
                    sony += 1;
                }
            }
        }
        assert_eq!(with_xmp, 78, "every image should carry xmp");
        assert_eq!(sony, 78, "every image should be a SONY capture");
        drop(snapshot);
    }
}
