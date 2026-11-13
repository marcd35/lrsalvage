//! `.lrcat-data` holds AI mask layers, not previews. This module decodes them
//! anyway, because a silhouette is still evidence that an edit existed.
//!
//! Layout: `<hex[0]>/<hex[..4]>/<32-hex-uppercase>.dat`, whose name equals the
//! `MaskDigest` recorded in `Adobe_imageDevelopSettings`. The payload is a TIFF
//! (`II*\0`) preceded by a 16-byte prefix, with Adobe Deflate strips and
//! Predictor 2. Strip offsets are relative to the TIFF header, so add 16.

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Bytes in front of the TIFF header; strip offsets are relative to its end.
const PREFIX_LEN: usize = 16;

/// A decoded mask bitmap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    /// Row-major 8-bit samples, length `width * height`.
    pub gray: Vec<u8>,
}

/// One IFD entry, with its 4-byte value field kept verbatim.
struct Entry {
    tag: u16,
    typ: u16,
    count: u32,
    field: [u8; 4],
}

fn malformed(detail: impl Into<String>) -> Error {
    Error::PyramidHeader(detail.into())
}

/// Bytes per value for every TIFF type this format can carry.
fn type_size(typ: u16) -> Option<usize> {
    Some(match typ {
        // BYTE, ASCII, SBYTE, UNDEFINED
        1 | 2 | 6 | 7 => 1,
        // SHORT, SSHORT
        3 | 8 => 2,
        // LONG, SLONG, FLOAT, IFD
        4 | 9 | 11 | 13 => 4,
        // RATIONAL, SRATIONAL, DOUBLE
        5 | 10 | 12 => 8,
        _ => return None,
    })
}

fn le_u16(bytes: &[u8], at: usize) -> Result<u16> {
    let pair = bytes
        .get(at..at + 2)
        .ok_or_else(|| malformed(format!("truncated u16 at {at}")))?;
    Ok(u16::from_le_bytes([pair[0], pair[1]]))
}

fn le_u32(bytes: &[u8], at: usize) -> Result<u32> {
    let quad = bytes
        .get(at..at + 4)
        .ok_or_else(|| malformed(format!("truncated u32 at {at}")))?;
    Ok(u32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]))
}

impl Entry {
    /// The entry's values as bytes, following the offset when they do not fit
    /// in the 4-byte value field.
    fn value_bytes<'a>(&'a self, tiff: &'a [u8]) -> Result<&'a [u8]> {
        let size = type_size(self.typ).ok_or_else(|| {
            malformed(format!(
                "tag 0x{:04X} has unknown type {}",
                self.tag, self.typ
            ))
        })?;
        let len = size.checked_mul(self.count as usize).ok_or_else(|| {
            malformed(format!(
                "tag 0x{:04X} declares an impossible count",
                self.tag
            ))
        })?;
        if len <= 4 {
            return Ok(&self.field[..len]);
        }
        let offset = u32::from_le_bytes(self.field) as usize;
        let end = offset
            .checked_add(len)
            .ok_or_else(|| malformed(format!("tag 0x{:04X} values overflow", self.tag)))?;
        tiff.get(offset..end)
            .ok_or_else(|| malformed(format!("tag 0x{:04X} values run past the TIFF", self.tag)))
    }

    /// The first value of a SHORT or LONG entry.
    fn first_u32(&self, tiff: &[u8]) -> Result<u32> {
        let bytes = self.value_bytes(tiff)?;
        match (self.typ, bytes.len()) {
            (3, n) if n >= 2 => Ok(u16::from_le_bytes([bytes[0], bytes[1]]) as u32),
            (4, n) if n >= 4 => le_u32(bytes, 0),
            (1, n) if n >= 1 => Ok(bytes[0] as u32),
            _ => Err(malformed(format!(
                "tag 0x{:04X} is not a scalar SHORT or LONG",
                self.tag
            ))),
        }
    }

    /// Every value of a SHORT or LONG entry.
    fn u32_values(&self, tiff: &[u8]) -> Result<Vec<u32>> {
        let bytes = self.value_bytes(tiff)?;
        let count = self.count as usize;
        match self.typ {
            3 => (0..count)
                .map(|i| le_u16(bytes, i * 2).map(u32::from))
                .collect(),
            4 => (0..count).map(|i| le_u32(bytes, i * 4)).collect(),
            _ => Err(malformed(format!(
                "tag 0x{:04X} is not a SHORT or LONG array",
                self.tag
            ))),
        }
    }
}

fn parse_ifd(tiff: &[u8], offset: usize) -> Result<Vec<Entry>> {
    let count = le_u16(tiff, offset)? as usize;
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let at = offset + 2 + i * 12;
        let tag = le_u16(tiff, at)?;
        let typ = le_u16(tiff, at + 2)?;
        let n = le_u32(tiff, at + 4)?;
        let field = tiff
            .get(at + 8..at + 12)
            .ok_or_else(|| malformed(format!("IFD entry {i} runs past the TIFF")))?;
        let mut raw = [0u8; 4];
        raw.copy_from_slice(field);
        entries.push(Entry {
            tag,
            typ,
            count: n,
            field: raw,
        });
    }
    Ok(entries)
}

fn find(entries: &[Entry], tag: u16) -> Option<&Entry> {
    entries.iter().find(|e| e.tag == tag)
}

/// Decode one `.dat` file into an 8-bit mask.
pub fn decode_dat(bytes: &[u8]) -> Result<Mask> {
    let tiff = bytes
        .get(PREFIX_LEN..)
        .ok_or_else(|| malformed("file is shorter than the 16-byte prefix"))?;
    // The two leading u32s are a version and a mask kind we cannot interpret;
    // they are deliberately not validated. The TIFF header is mandatory.
    if tiff.len() < 8 || &tiff[0..2] != b"II" || le_u16(tiff, 2)? != 42 {
        return Err(malformed("no little-endian TIFF magic at offset 16"));
    }
    let ifd_offset = le_u32(tiff, 4)? as usize;
    let entries = parse_ifd(tiff, ifd_offset)?;

    let width = find(&entries, 0x0100)
        .ok_or_else(|| malformed("no ImageWidth"))?
        .first_u32(tiff)?;
    let height = find(&entries, 0x0101)
        .ok_or_else(|| malformed("no ImageLength"))?
        .first_u32(tiff)?;
    if width == 0 || height == 0 {
        return Err(malformed("empty mask image"));
    }

    let bits = match find(&entries, 0x0102) {
        Some(e) => e.first_u32(tiff)?,
        None => 8,
    };
    if bits != 8 {
        return Err(malformed(format!("unsupported BitsPerSample {bits}")));
    }

    let compression = match find(&entries, 0x0103) {
        Some(e) => e.first_u32(tiff)?,
        None => 1,
    };
    if compression != 8 && compression != 1 {
        return Err(malformed(format!("unsupported Compression {compression}")));
    }

    let photometric = match find(&entries, 0x0106) {
        Some(e) => e.first_u32(tiff)?,
        None => 1,
    };
    if photometric != 1 {
        return Err(malformed(format!(
            "unsupported PhotometricInterpretation {photometric}"
        )));
    }

    let samples = match find(&entries, 0x0115) {
        Some(e) => e.first_u32(tiff)?,
        None => 1,
    };
    if samples != 1 {
        return Err(malformed(format!("unsupported SamplesPerPixel {samples}")));
    }

    let planar = match find(&entries, 0x011C) {
        Some(e) => e.first_u32(tiff)?,
        None => 1,
    };
    if planar != 1 {
        return Err(malformed(format!(
            "unsupported PlanarConfiguration {planar}"
        )));
    }

    let predictor = match find(&entries, 0x013D) {
        Some(e) => e.first_u32(tiff)?,
        None => 1,
    };
    if predictor != 1 && predictor != 2 {
        return Err(malformed(format!("unsupported Predictor {predictor}")));
    }

    let offsets = find(&entries, 0x0111)
        .ok_or_else(|| malformed("no StripOffsets"))?
        .u32_values(tiff)?;
    let lengths = find(&entries, 0x0117)
        .ok_or_else(|| malformed("no StripByteCounts"))?
        .u32_values(tiff)?;
    if offsets.is_empty() || offsets.len() != lengths.len() {
        return Err(malformed(format!(
            "{} StripOffsets against {} StripByteCounts",
            offsets.len(),
            lengths.len()
        )));
    }

    let rows_per_strip = match find(&entries, 0x0116) {
        Some(e) => e.first_u32(tiff)?,
        None => height,
    };
    if rows_per_strip == 0 {
        return Err(malformed("RowsPerStrip is 0"));
    }

    let width = width as usize;
    let height = height as usize;
    let row_bytes = width;
    let total = width
        .checked_mul(height)
        .ok_or_else(|| malformed("mask dimensions overflow"))?;
    // A 20-byte header must not be able to request a multi-gigabyte
    // allocation. Real AI masks are a few megapixels; 250 MP is far above
    // anything Lightroom writes and still bounded.
    const MAX_PIXELS: usize = 250_000_000;
    if total > MAX_PIXELS {
        return Err(malformed(format!(
            "mask declares {total} pixels, above the {MAX_PIXELS} limit"
        )));
    }
    let mut gray = vec![0u8; total];

    let mut row_cursor = 0usize;
    for (index, (&offset, &length)) in offsets.iter().zip(lengths.iter()).enumerate() {
        if row_cursor >= height {
            break;
        }
        let rows = (rows_per_strip as usize).min(height - row_cursor);
        let needed = rows * row_bytes;
        let start = PREFIX_LEN
            .checked_add(offset as usize)
            .ok_or_else(|| malformed(format!("strip {index} offset overflows")))?;
        let end = start
            .checked_add(length as usize)
            .ok_or_else(|| malformed(format!("strip {index} length overflows")))?;
        let stored = bytes.get(start..end).ok_or_else(|| {
            malformed(format!(
                "strip {index} spans {start}..{end}, past the {} byte file",
                bytes.len()
            ))
        })?;

        let mut data = if compression == 8 {
            let mut inflated = Vec::new();
            flate2::read::ZlibDecoder::new(stored)
                .read_to_end(&mut inflated)
                .map_err(|e| malformed(format!("strip {index} is not zlib: {e}")))?;
            inflated
        } else {
            // Compression 1 stores the strip bytes as they are.
            stored.to_vec()
        };
        if data.len() < needed {
            return Err(malformed(format!(
                "strip {index} held {} bytes, expected {needed}",
                data.len()
            )));
        }
        data.truncate(needed);

        if predictor == 2 {
            for row in data.chunks_mut(row_bytes) {
                for i in 1..row.len() {
                    row[i] = row[i].wrapping_add(row[i - 1]);
                }
            }
        }

        gray[row_cursor * row_bytes..(row_cursor + rows) * row_bytes].copy_from_slice(&data);
        row_cursor += rows;
    }

    if row_cursor != height {
        return Err(malformed(format!(
            "strips cover {row_cursor} of {height} rows"
        )));
    }

    Ok(Mask {
        width: width as u32,
        height: height as u32,
        gray,
    })
}

/// Encode a mask as a grayscale PNG.
pub fn to_png(mask: &Mask) -> Result<Vec<u8>> {
    let png_error = |e: png::EncodingError| Error::Png(e.to_string());

    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, mask.width, mask.height);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(png_error)?;
        writer.write_image_data(&mask.gray).map_err(png_error)?;
        writer.finish().map_err(png_error)?;
    }
    Ok(out)
}

/// Every `.dat` under an `.lrcat-data` directory.
pub fn list(root: &Path) -> Result<Vec<PathBuf>> {
    let io_error = |e: walkdir::Error| Error::Io {
        path: e
            .path()
            .map_or_else(|| root.to_path_buf(), Path::to_path_buf),
        source: std::io::Error::other(e),
    };

    let mut found = Vec::new();
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry.map_err(io_error)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let is_dat = entry
            .path()
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("dat"));
        if is_dat {
            found.push(entry.into_path());
        }
    }
    found.sort();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const PREFIX: [u8; 16] = [
        0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ];

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).expect("compress");
        encoder.finish().expect("finish zlib stream")
    }

    fn push_long(tiff: &mut Vec<u8>, tag: u16, value: u32) {
        tiff.extend_from_slice(&tag.to_le_bytes());
        tiff.extend_from_slice(&4u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&value.to_le_bytes());
    }

    fn push_short(tiff: &mut Vec<u8>, tag: u16, value: u16) {
        tiff.extend_from_slice(&tag.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&value.to_le_bytes());
        tiff.extend_from_slice(&[0, 0]);
    }

    fn push_long_array(tiff: &mut Vec<u8>, tag: u16, offset: u32, count: usize) {
        tiff.extend_from_slice(&tag.to_le_bytes());
        tiff.extend_from_slice(&4u16.to_le_bytes());
        tiff.extend_from_slice(&(count as u32).to_le_bytes());
        tiff.extend_from_slice(&offset.to_le_bytes());
    }

    /// Lay out a `.dat` around already-encoded strip payloads.
    fn dat_from_strips(
        width: u32,
        height: u32,
        rows_per_strip: u32,
        predictor: u16,
        compression: u16,
        strips: &[Vec<u8>],
    ) -> Vec<u8> {
        let strip_count = strips.len();
        const ENTRY_COUNT: usize = 12;
        let ifd_size = 2 + ENTRY_COUNT * 12 + 4;
        let arrays_start = 8 + ifd_size;
        let arrays_len = if strip_count > 1 { 8 * strip_count } else { 0 };
        let strip_data_start = arrays_start + arrays_len;

        let mut offsets = Vec::with_capacity(strip_count);
        let mut cursor = strip_data_start;
        for strip in strips {
            offsets.push(cursor as u32);
            cursor += strip.len();
        }

        let mut tiff = Vec::with_capacity(cursor);
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.extend_from_slice(&(ENTRY_COUNT as u16).to_le_bytes());
        push_long(&mut tiff, 0x00FE, 0);
        push_long(&mut tiff, 0x0100, width);
        push_long(&mut tiff, 0x0101, height);
        push_short(&mut tiff, 0x0102, 8);
        push_short(&mut tiff, 0x0103, compression);
        push_short(&mut tiff, 0x0106, 1);
        if strip_count == 1 {
            push_long(&mut tiff, 0x0111, offsets[0]);
        } else {
            push_long_array(&mut tiff, 0x0111, arrays_start as u32, strip_count);
        }
        push_short(&mut tiff, 0x0115, 1);
        push_long(&mut tiff, 0x0116, rows_per_strip);
        if strip_count == 1 {
            push_long(&mut tiff, 0x0117, strips[0].len() as u32);
        } else {
            push_long_array(
                &mut tiff,
                0x0117,
                (arrays_start + 4 * strip_count) as u32,
                strip_count,
            );
        }
        push_short(&mut tiff, 0x011C, 1);
        push_short(&mut tiff, 0x013D, predictor);
        tiff.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(tiff.len(), arrays_start, "IFD layout");
        if strip_count > 1 {
            for offset in &offsets {
                tiff.extend_from_slice(&offset.to_le_bytes());
            }
            for strip in strips {
                tiff.extend_from_slice(&(strip.len() as u32).to_le_bytes());
            }
        }
        assert_eq!(tiff.len(), strip_data_start, "strip data start");
        for strip in strips {
            tiff.extend_from_slice(strip);
        }

        let mut file = PREFIX.to_vec();
        file.extend_from_slice(&tiff);
        file
    }

    /// Encode a full image as strips, applying Predictor 2 per row of each strip.
    fn dat_image(
        width: usize,
        height: usize,
        rows_per_strip: usize,
        predictor: u16,
        compression: u16,
        pixels: &[u8],
    ) -> Vec<u8> {
        assert_eq!(pixels.len(), width * height, "test image size");
        let mut strips = Vec::new();
        let mut row = 0;
        while row < height {
            let rows = rows_per_strip.min(height - row);
            let mut raw = pixels[row * width..(row + rows) * width].to_vec();
            if predictor == 2 {
                for r in 0..rows {
                    let base = r * width;
                    for i in (1..width).rev() {
                        raw[base + i] = raw[base + i].wrapping_sub(raw[base + i - 1]);
                    }
                }
            }
            strips.push(if compression == 8 { zlib(&raw) } else { raw });
            row += rows;
        }
        dat_from_strips(
            width as u32,
            height as u32,
            rows_per_strip as u32,
            predictor,
            compression,
            &strips,
        )
    }

    #[test]
    fn decodes_two_deflate_strips_with_predictor_two() {
        // Row 0 ends at 40 and row 1 starts at 1: a decoder that lets the
        // running sum cross the strip boundary would read 41, not 1.
        let pixels: [u8; 8] = [10, 20, 30, 40, 1, 2, 3, 4];
        let dat = dat_image(4, 2, 1, 2, 8, &pixels);

        let mask = decode_dat(&dat).expect("decode two-strip dat");
        assert_eq!(mask.width, 4);
        assert_eq!(mask.height, 2);
        assert_eq!(mask.gray, pixels);
    }

    #[test]
    fn decodes_predictor_one_without_differencing() {
        // Repeated and descending values would change under Predictor 2.
        let pixels: [u8; 8] = [7, 7, 200, 3, 250, 1, 1, 128];
        let dat = dat_image(4, 2, 2, 1, 8, &pixels);

        let mask = decode_dat(&dat).expect("decode Predictor 1 dat");
        assert_eq!(mask.gray, pixels);
    }

    #[test]
    fn decodes_uncompressed_strips() {
        let pixels: [u8; 8] = [0, 255, 128, 64, 5, 6, 7, 8];
        let dat = dat_image(4, 2, 1, 1, 1, &pixels);

        let mask = decode_dat(&dat).expect("decode Compression 1 dat");
        assert_eq!(mask.gray, pixels);
    }

    #[test]
    fn last_strip_may_be_shorter_than_rows_per_strip() {
        // 3 rows in strips of 2: the second strip holds one row only.
        let pixels: [u8; 12] = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120];
        let dat = dat_image(4, 3, 2, 2, 8, &pixels);

        let mask = decode_dat(&dat).expect("decode partial last strip");
        assert_eq!(mask.height, 3);
        assert_eq!(mask.gray, pixels);
    }

    /// File offset of the first StripOffsets value, read straight from IFD0.
    fn strip_offsets_field(dat: &[u8]) -> usize {
        let ifd = 16 + 8;
        let count = u16::from_le_bytes([dat[ifd], dat[ifd + 1]]) as usize;
        for i in 0..count {
            let at = ifd + 2 + i * 12;
            if u16::from_le_bytes([dat[at], dat[at + 1]]) == 0x0111 {
                let values =
                    u32::from_le_bytes([dat[at + 4], dat[at + 5], dat[at + 6], dat[at + 7]]);
                let value =
                    u32::from_le_bytes([dat[at + 8], dat[at + 9], dat[at + 10], dat[at + 11]])
                        as usize;
                if values == 1 {
                    return value;
                }
                // More than one strip: the value field points at the array.
                let base = 16 + value;
                return u32::from_le_bytes([dat[base], dat[base + 1], dat[base + 2], dat[base + 3]])
                    as usize;
            }
        }
        panic!("test fixture has no StripOffsets entry");
    }

    #[test]
    fn strip_offsets_are_relative_to_the_tiff_header() {
        let pixels: [u8; 8] = [0, 255, 1, 254, 128, 127, 42, 200];
        let dat = dat_image(4, 2, 1, 2, 8, &pixels);
        assert_eq!(
            decode_dat(&dat).expect("decode relative offsets").gray,
            pixels
        );

        let offset = strip_offsets_field(&dat);
        // The TIFF-relative position holds the zlib header...
        assert_eq!(dat[16 + offset], 0x78, "zlib header at TIFF base + offset");
        // ...while reading from file start lands 16 bytes early, in the
        // prefix/IFD tail, where no zlib stream begins.
        assert_ne!(
            dat[offset], 0x78,
            "file-absolute offset must land off-stream"
        );
    }

    #[test]
    fn ignores_the_uninterpreted_prefix_words() {
        // The leading u32s are a version and a kind, not something to trust.
        let pixels: [u8; 4] = [1, 2, 3, 4];
        let mut dat = dat_image(4, 1, 1, 2, 8, &pixels);
        dat[0] = 0x09;
        dat[4] = 0x07;
        assert_eq!(
            decode_dat(&dat).expect("decode with odd prefix").gray,
            pixels
        );
    }

    #[test]
    fn rejects_files_without_a_tiff_header() {
        let mut dat = dat_image(4, 1, 1, 2, 8, &[1, 2, 3, 4]);
        dat[16] = b'M';
        dat[17] = b'M';
        assert!(matches!(decode_dat(&dat), Err(Error::PyramidHeader(_))));
        assert!(matches!(decode_dat(b""), Err(Error::PyramidHeader(_))));
        assert!(matches!(
            decode_dat(&dat[..20]),
            Err(Error::PyramidHeader(_))
        ));
    }

    #[test]
    fn rejects_a_strip_that_runs_past_eof() {
        let dat = dat_image(4, 1, 1, 2, 8, &[1, 2, 3, 4]);
        let truncated = &dat[..dat.len() - 2];
        assert!(matches!(
            decode_dat(truncated),
            Err(Error::PyramidHeader(_))
        ));
    }

    #[test]
    fn rejects_a_strip_that_is_not_zlib() {
        // Declared Adobe Deflate, but the strip is four plain bytes.
        let strips = vec![vec![0u8, 1, 2, 3]];
        let dat = dat_from_strips(4, 1, 1, 1, 8, &strips);
        assert!(matches!(decode_dat(&dat), Err(Error::PyramidHeader(_))));
    }

    #[test]
    fn rejects_compressions_and_predictors_it_cannot_undo() {
        let pixels: [u8; 4] = [1, 2, 3, 4];
        // Compression 7 is JPEG-in-TIFF; 5 is LZW.
        for compression in [5u16, 7] {
            let dat = dat_image(4, 1, 1, 1, compression, &pixels);
            assert!(
                matches!(decode_dat(&dat), Err(Error::PyramidHeader(_))),
                "Compression {compression} must be rejected"
            );
        }
        for predictor in [0u16, 3] {
            let dat = dat_image(4, 1, 1, predictor, 8, &pixels);
            assert!(
                matches!(decode_dat(&dat), Err(Error::PyramidHeader(_))),
                "Predictor {predictor} must be rejected"
            );
        }
    }

    #[test]
    fn every_prefix_of_a_valid_dat_is_answered_without_panicking() {
        let dat = dat_image(4, 2, 1, 2, 8, &[10, 20, 30, 40, 1, 2, 3, 4]);
        for end in 0..=dat.len() {
            // Either Ok or Err is fine here; a panic fails the test.
            let _ = decode_dat(&dat[..end]);
        }
        assert!(decode_dat(&dat).is_ok());
    }

    #[test]
    fn png_round_trips_dimensions_and_pixels() {
        let mask = Mask {
            width: 3,
            height: 2,
            gray: vec![0, 10, 200, 255, 128, 64],
        };
        let bytes = to_png(&mask).expect("encode grayscale png");

        let mut reader = png::Decoder::new(std::io::Cursor::new(&bytes))
            .read_info()
            .expect("read png info");
        let mut buffer = vec![0; reader.output_buffer_size().expect("png buffer size")];
        let info = reader.next_frame(&mut buffer).expect("decode png frame");

        assert_eq!((info.width, info.height), (3, 2));
        assert_eq!(info.color_type, png::ColorType::Grayscale);
        assert_eq!(info.bit_depth, png::BitDepth::Eight);
        buffer.truncate(info.buffer_size());
        assert_eq!(buffer, mask.gray);
        assert_eq!(buffer.first(), Some(&0));
        assert_eq!(buffer.last(), Some(&64));
    }

    #[test]
    fn lists_nested_dat_files_sorted_and_ignores_other_extensions() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("cat.lrcat-data");
        let nested = root.join("a").join("abcd");
        let other = root.join("b").join("1234");
        std::fs::create_dir_all(&nested).expect("nested dir");
        std::fs::create_dir_all(&other).expect("other dir");

        let first = nested.join("0123456789ABCDEF0123456789ABCDEF.dat");
        let second = other.join("FEDCBA9876543210FEDCBA9876543210.dat");
        std::fs::write(&first, b"mask").expect("write first");
        std::fs::write(&second, b"mask").expect("write second");
        std::fs::write(nested.join("notes.txt"), b"not a mask").expect("write txt");
        std::fs::write(other.join("preview.jpe"), b"not a mask").expect("write jpe");
        std::fs::write(other.join("mask.dat.bak"), b"not a mask").expect("write bak");
        std::fs::create_dir_all(other.join("directory.dat")).expect("dir named like a mask");

        let found = list(&root).expect("walk mask directory");
        assert_eq!(found, vec![first, second]);
    }

    #[test]
    fn missing_root_is_an_io_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = dir.path().join("absent.lrcat-data");
        assert!(matches!(list(&missing), Err(Error::Io { .. })));
    }
}
