//! Minimal EXIF writing: put the catalog's facts back into the recovered JPEG.
//!
//! The preview bytes are never modified — an APP1 segment is inserted (or
//! replaced) in front of the existing image data. See ADR 0003.

use crate::error::Result;
use crate::model::{Orientation, XmpMetadata};

/// The fields worth restoring on a recovered preview.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MetadataInput {
    pub orientation: Orientation,
    /// `YYYY-MM-DDTHH:MM:SS` as stored in the catalog.
    pub capture_time: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<u32>,
    pub exposure_time: Option<String>,
    pub f_number: Option<String>,
    pub focal_length: Option<String>,
    pub rating: Option<u8>,
    pub gps: Option<(f64, f64)>,
}

impl MetadataInput {
    /// True when there is nothing worth writing.
    pub fn is_empty(&self) -> bool {
        self.capture_time.is_none()
            && self.make.is_none()
            && self.model.is_none()
            && self.lens.is_none()
            && self.iso.is_none()
            && self.gps.is_none()
            && self.orientation.is_upright()
    }
}

/// Build the input from catalog metadata plus the orientation decision.
pub fn from_catalog(xmp: &XmpMetadata, orientation: Orientation) -> MetadataInput {
    MetadataInput {
        orientation,
        capture_time: xmp.capture_time.clone(),
        make: xmp.make.clone(),
        model: xmp.model.clone(),
        lens: xmp.lens.clone(),
        iso: xmp.iso,
        exposure_time: xmp.exposure_time.clone(),
        f_number: xmp.f_number.clone(),
        focal_length: xmp.focal_length.clone(),
        rating: xmp.rating,
        gps: xmp.gps,
    }
}

/// Insert or replace the EXIF APP1 segment. Returns a complete JPEG.
pub fn inject(jpeg: &[u8], meta: &MetadataInput) -> Result<Vec<u8>> {
    if jpeg.len() < 2 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return Err(crate::error::Error::JpegMarkersMissing);
    }
    if meta.is_empty() {
        return Ok(jpeg.to_vec());
    }

    let (segments, tail_start) = split_header(jpeg)?;
    let app1 = build_app1(meta);

    let mut out = Vec::with_capacity(jpeg.len() + app1.len());
    out.extend_from_slice(&jpeg[..2]);
    out.extend_from_slice(&app1);
    for range in segments {
        out.extend_from_slice(&jpeg[range]);
    }
    out.extend_from_slice(&jpeg[tail_start..]);
    Ok(out)
}

/// The EXIF identifier that opens an APP1 payload.
const EXIF_PREFIX: &[u8] = b"Exif\0\0";

/// Largest legal value of a segment's two-byte length field.
const MAX_SEGMENT_LENGTH: usize = u16::MAX as usize - 2;
/// Largest APP1 payload whose `+ 2` length field still fits.
const MAX_APP1_PAYLOAD: usize = MAX_SEGMENT_LENGTH - 2;

// TIFF field types, all little-endian.
const TYPE_BYTE: u16 = 1;
const TYPE_ASCII: u16 = 2;
const TYPE_SHORT: u16 = 3;
const TYPE_LONG: u16 = 4;
const TYPE_RATIONAL: u16 = 5;

/// Walk the header of a JPEG, returning the byte ranges worth keeping and the
/// offset where scan (or trailer) data begins.
///
/// Every segment other than an existing `Exif\0\0` APP1 is kept, so the caller
/// reproduces the input byte for byte apart from the one inserted segment.
fn split_header(jpeg: &[u8]) -> Result<(Vec<std::ops::Range<usize>>, usize)> {
    let mut kept = Vec::new();
    let mut pos = 2;

    loop {
        if pos >= jpeg.len() {
            return Ok((kept, pos));
        }
        if jpeg[pos] != 0xFF {
            return Err(crate::error::Error::JpegMalformed);
        }
        let start = pos;
        // Markers may be preceded by any number of 0xFF fill bytes.
        while pos < jpeg.len() && jpeg[pos] == 0xFF {
            pos += 1;
        }
        let Some(&marker) = jpeg.get(pos) else {
            // A trailing run of fill bytes is truncated.
            return Err(crate::error::Error::JpegMalformed);
        };
        pos += 1;

        match marker {
            // A stuffed zero is only legal inside entropy-coded data.
            0x00 => return Err(crate::error::Error::JpegMalformed),
            // Standalone markers carry no length.
            0x01 | 0xD8 | 0xD0..=0xD7 => kept.push(start..pos),
            // From SOS and EOI onward the original bytes are copied verbatim.
            0xD9 | 0xDA => return Ok((kept, start)),
            _ => {
                let length = jpeg
                    .get(pos..pos + 2)
                    .ok_or(crate::error::Error::JpegMalformed)?;
                let length = u16::from_be_bytes([length[0], length[1]]) as usize;
                if length < 2 {
                    return Err(crate::error::Error::JpegMalformed);
                }
                let end = pos
                    .checked_add(length)
                    .filter(|&end| end <= jpeg.len())
                    .ok_or(crate::error::Error::JpegMalformed)?;
                let is_exif = marker == 0xE1
                    && length >= 2 + EXIF_PREFIX.len()
                    && jpeg[pos + 2..end].starts_with(EXIF_PREFIX);
                if !is_exif {
                    kept.push(start..end);
                }
                pos = end;
            }
        }
    }
}

/// Build the complete `FF E1` segment, shrinking the TIFF until it fits.
///
/// GPS goes first, then the lens string, then long free text is truncated.
/// The final fallback carries only orientation and the numeric facts, which is
/// far below the limit, so an oversized length field is never emitted.
fn build_app1(meta: &MetadataInput) -> Vec<u8> {
    let attempts = [
        (true, true, usize::MAX),
        (false, true, usize::MAX),
        (false, false, usize::MAX),
        (false, false, 512),
        (false, false, 0),
    ];
    let tiff = attempts
        .into_iter()
        .map(|(gps, lens, max_text)| build_tiff(meta, gps, lens, max_text))
        .find(|tiff| EXIF_PREFIX.len() + tiff.len() <= MAX_APP1_PAYLOAD)
        .unwrap_or_else(|| {
            build_tiff(
                &MetadataInput {
                    orientation: meta.orientation,
                    ..MetadataInput::default()
                },
                false,
                false,
                0,
            )
        });

    let payload_len = EXIF_PREFIX.len() + tiff.len();
    debug_assert!(payload_len <= MAX_APP1_PAYLOAD);
    let length = u16::try_from(payload_len + 2).expect("payload size is checked above");

    let mut out = Vec::with_capacity(payload_len + 4);
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(EXIF_PREFIX);
    out.extend_from_slice(&tiff);
    out
}

/// One IFD entry, with its value bytes already encoded for the TIFF type.
#[derive(Debug, Clone)]
struct Entry {
    tag: u16,
    field_type: u16,
    count: u32,
    value: Vec<u8>,
}

fn ascii_entry(tag: u16, text: &str, max_len: usize) -> Entry {
    let text = truncate(text, max_len).replace('\0', "");
    let mut value = text.into_bytes();
    value.push(0);
    Entry {
        tag,
        field_type: TYPE_ASCII,
        count: value.len() as u32,
        value,
    }
}

fn short_entry(tag: u16, value: u16) -> Entry {
    Entry {
        tag,
        field_type: TYPE_SHORT,
        count: 1,
        value: value.to_le_bytes().to_vec(),
    }
}

fn long_entry(tag: u16, value: u32) -> Entry {
    Entry {
        tag,
        field_type: TYPE_LONG,
        count: 1,
        value: value.to_le_bytes().to_vec(),
    }
}

fn byte_entry(tag: u16, bytes: &[u8]) -> Entry {
    Entry {
        tag,
        field_type: TYPE_BYTE,
        count: bytes.len() as u32,
        value: bytes.to_vec(),
    }
}

fn rational_entry(tag: u16, values: &[(u32, u32)]) -> Entry {
    let mut value = Vec::with_capacity(values.len() * 8);
    for (numerator, denominator) in values {
        value.extend_from_slice(&numerator.to_le_bytes());
        value.extend_from_slice(&denominator.to_le_bytes());
    }
    Entry {
        tag,
        field_type: TYPE_RATIONAL,
        count: values.len() as u32,
        value,
    }
}

/// Word-align a value block, as libtiff expects.
fn aligned(len: usize) -> usize {
    len + (len & 1)
}

/// Size of an IFD written at any (even) base offset.
fn ifd_size(entries: &[Entry]) -> usize {
    2 + entries.len() * 12
        + 4
        + entries
            .iter()
            .filter(|entry| entry.value.len() > 4)
            .map(|entry| aligned(entry.value.len()))
            .sum::<usize>()
}

/// Serialize an IFD whose first byte sits at `base` in the TIFF block.
fn serialize_ifd(entries: &[Entry], base: u32) -> Vec<u8> {
    let count = u16::try_from(entries.len()).expect("an IFD has few entries");
    let mut out = Vec::with_capacity(ifd_size(entries));
    out.extend_from_slice(&count.to_le_bytes());

    let data_start = base + 2 + (entries.len() as u32) * 12 + 4;
    let mut data: Vec<u8> = Vec::new();

    for entry in entries {
        out.extend_from_slice(&entry.tag.to_le_bytes());
        out.extend_from_slice(&entry.field_type.to_le_bytes());
        out.extend_from_slice(&entry.count.to_le_bytes());
        if entry.value.len() <= 4 {
            let mut inline = [0u8; 4];
            inline[..entry.value.len()].copy_from_slice(&entry.value);
            out.extend_from_slice(&inline);
        } else {
            let offset = data_start + data.len() as u32;
            out.extend_from_slice(&offset.to_le_bytes());
            data.extend_from_slice(&entry.value);
            if data.len() % 2 == 1 {
                data.push(0);
            }
        }
    }

    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&data);
    out
}

/// Lay out the TIFF block referenced by the APP1 payload.
///
/// `include_gps` and `include_lens` let the caller shed the largest optional
/// parts when the segment would not fit; `max_text` caps free-text values.
fn build_tiff(
    meta: &MetadataInput,
    include_gps: bool,
    include_lens: bool,
    max_text: usize,
) -> Vec<u8> {
    let mut ifd0 = Vec::new();
    if let Some(make) = meta.make.as_deref().filter(|s| !s.is_empty()) {
        ifd0.push(ascii_entry(0x010F, make, max_text));
    }
    if let Some(model) = meta.model.as_deref().filter(|s| !s.is_empty()) {
        ifd0.push(ascii_entry(0x0110, model, max_text));
    }
    // Always present: an upright image still benefits from an explicit 1.
    ifd0.push(short_entry(0x0112, meta.orientation.exif_value()));
    let taken = meta.capture_time.as_deref().and_then(exif_datetime);
    if let Some(datetime) = taken.as_deref() {
        ifd0.push(ascii_entry(0x0132, datetime, usize::MAX));
    }

    let mut exif_ifd = Vec::new();
    if let Some(datetime) = taken.as_deref() {
        exif_ifd.push(ascii_entry(0x9003, datetime, usize::MAX));
        exif_ifd.push(ascii_entry(0x9004, datetime, usize::MAX));
    }
    if let Some(iso) = meta.iso.and_then(|iso| u16::try_from(iso).ok()) {
        exif_ifd.push(short_entry(0x8827, iso));
    }
    if let Some(exposure) = meta.exposure_time.as_deref().and_then(parse_rational) {
        exif_ifd.push(rational_entry(0x829A, &[exposure]));
    }
    if let Some(f_number) = meta.f_number.as_deref().and_then(parse_rational) {
        exif_ifd.push(rational_entry(0x829D, &[f_number]));
    }
    if let Some(focal) = meta.focal_length.as_deref().and_then(parse_rational) {
        exif_ifd.push(rational_entry(0x920A, &[focal]));
    }
    if include_lens {
        if let Some(lens) = meta.lens.as_deref().filter(|s| !s.is_empty()) {
            exif_ifd.push(ascii_entry(0xA434, lens, max_text));
        }
    }

    let gps_ifd = if include_gps {
        meta.gps.and_then(gps_entries)
    } else {
        None
    };

    // Pointers are LONGs, so they take the same room whether present or not
    // and the IFD sizes stay stable while the offsets are worked out.
    if !exif_ifd.is_empty() {
        ifd0.push(long_entry(0x8769, 0));
    }
    if gps_ifd.is_some() {
        ifd0.push(long_entry(0x8825, 0));
    }
    ifd0.sort_by_key(|entry| entry.tag);
    exif_ifd.sort_by_key(|entry| entry.tag);

    let ifd0_size = ifd_size(&ifd0);
    let exif_base = 8 + ifd0_size as u32;
    // An empty Exif IFD is not written at all, so it consumes no space.
    let exif_size = if exif_ifd.is_empty() {
        0
    } else {
        ifd_size(&exif_ifd) as u32
    };
    let gps_base = exif_base + exif_size;

    for entry in &mut ifd0 {
        match entry.tag {
            0x8769 => entry.value = exif_base.to_le_bytes().to_vec(),
            0x8825 => entry.value = gps_base.to_le_bytes().to_vec(),
            _ => {}
        }
    }

    let mut out = Vec::with_capacity(8 + ifd0_size + exif_size as usize);
    out.extend_from_slice(b"II");
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&serialize_ifd(&ifd0, 8));
    if !exif_ifd.is_empty() {
        out.extend_from_slice(&serialize_ifd(&exif_ifd, exif_base));
    }
    if let Some(gps_ifd) = gps_ifd {
        out.extend_from_slice(&serialize_ifd(&gps_ifd, gps_base));
    }
    out
}

/// The five GPS tags needed for a position, or `None` for a bad coordinate.
fn gps_entries((latitude, longitude): (f64, f64)) -> Option<Vec<Entry>> {
    let latitude_parts = coordinate(latitude)?;
    let longitude_parts = coordinate(longitude)?;
    Some(vec![
        byte_entry(0x0000, &[2, 3, 0, 0]),
        ascii_entry(0x0001, if latitude >= 0.0 { "N" } else { "S" }, usize::MAX),
        rational_entry(0x0002, &latitude_parts),
        ascii_entry(0x0003, if longitude >= 0.0 { "E" } else { "W" }, usize::MAX),
        rational_entry(0x0004, &longitude_parts),
    ])
}

/// Split decimal degrees into `(degrees, minutes, seconds/10000)`.
///
/// Ten-thousandths of an arcsecond keep a recovered position accurate to
/// roughly three metres, far beyond what a thumbnail could justify.
fn coordinate(degrees: f64) -> Option<[(u32, u32); 3]> {
    if !degrees.is_finite() || degrees.abs() > 180.0 {
        return None;
    }
    let total = (degrees.abs() * 3600.0 * 10_000.0).round() as u64;
    let whole_degrees = total / 36_000_000;
    let remainder = total % 36_000_000;
    let minutes = remainder / 600_000;
    let seconds = remainder % 600_000;
    Some([
        (whole_degrees as u32, 1),
        (minutes as u32, 1),
        (seconds as u32, 10_000),
    ])
}

/// Parse `"16/1"`, `"1/250"` or a plain decimal such as `"2.8"`.
fn parse_rational(text: &str) -> Option<(u32, u32)> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some((numerator, denominator)) = text.split_once('/') {
        let numerator: u32 = numerator.trim().parse().ok()?;
        let denominator: u32 = denominator.trim().parse().ok()?;
        return (denominator != 0).then_some((numerator, denominator));
    }

    let (whole, fraction) = match text.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (text, ""),
    };
    let whole: u32 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    if fraction.is_empty() {
        return Some((whole, 1));
    }
    if fraction.len() > 6 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let fraction_value: u32 = fraction.parse().ok()?;
    let denominator = 10u32.checked_pow(fraction.len() as u32)?;
    let numerator = whole
        .checked_mul(denominator)?
        .checked_add(fraction_value)?;
    Some((numerator, denominator))
}

/// `YYYY-MM-DDTHH:MM:SS…` → `YYYY:MM:DD HH:MM:SS`, dropping fraction and zone.
fn exif_datetime(text: &str) -> Option<String> {
    let text = text.trim();
    let core = text.get(..19)?;
    let separators_ok = core.bytes().enumerate().all(|(i, byte)| match i {
        4 | 7 => byte == b'-',
        10 => byte == b'T' || byte == b' ',
        13 | 16 => byte == b':',
        _ => byte.is_ascii_digit(),
    });
    if !separators_ok {
        return None;
    }
    Some(format!("{} {}", &core[..10], &core[11..19]))
}

/// Truncate on a character boundary so no value ever splits a UTF-8 sequence.
fn truncate(text: &str, max_len: usize) -> &str {
    if text.len() <= max_len {
        return text;
    }
    let mut end = max_len.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use std::io::Cursor;

    const APP0: &[u8] = b"JFIF\0\x01\x01\x00\x00\x01\x00\x01\x00\x00";

    /// A marker segment whose length includes its own two bytes.
    fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![0xFF, marker];
        v.extend_from_slice(
            &u16::try_from(payload.len() + 2)
                .expect("fits")
                .to_be_bytes(),
        );
        v.extend_from_slice(payload);
        v
    }

    /// A structurally valid 4x4 baseline JPEG: SOI, APP0, DQT, SOF0, DHT, SOS,
    /// entropy bytes, EOI. No pixels are ever decoded, so the tables are small.
    fn tiny_jpeg() -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        v.extend(segment(0xE0, APP0));
        let mut dqt = vec![0u8];
        dqt.extend_from_slice(&[1u8; 64]);
        v.extend(segment(0xDB, &dqt));
        let mut sof = vec![0x08u8];
        sof.extend_from_slice(&4u16.to_be_bytes());
        sof.extend_from_slice(&4u16.to_be_bytes());
        sof.extend_from_slice(&[0x01, 0x01, 0x11, 0x00]);
        v.extend(segment(0xC0, &sof));
        let mut dht = vec![0x00u8];
        dht.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        dht.push(0x00);
        v.extend(segment(0xC4, &dht));
        let sos = [0x01u8, 0x01, 0x00, 0x00, 0x3F, 0x00];
        v.extend(segment(0xDA, &sos));
        v.extend_from_slice(&[0x12, 0x34, 0xFF, 0x00, 0x56, 0x78]);
        v.extend_from_slice(&[0xFF, 0xD9]);
        v
    }

    #[test]
    fn rejects_a_buffer_that_does_not_start_with_soi() {
        for input in [&b""[..], b"not a jpeg", &[0xFF, 0xD9][..]] {
            assert!(
                matches!(
                    inject(input, &MetadataInput::default()),
                    Err(Error::JpegMarkersMissing)
                ),
                "input {input:?} must be rejected"
            );
        }
    }

    #[test]
    fn leaves_the_bytes_alone_when_there_is_nothing_to_write() {
        let jpeg = tiny_jpeg();
        let out = inject(&jpeg, &MetadataInput::default()).expect("upright and empty is a no-op");
        assert_eq!(out, jpeg);
    }

    fn read_exif(jpeg: &[u8]) -> ::exif::Exif {
        ::exif::Reader::new()
            .read_from_container(&mut Cursor::new(jpeg))
            .expect("written EXIF must parse back")
    }

    fn ascii_field(jpeg_exif: &::exif::Exif, tag: ::exif::Tag) -> Option<String> {
        let field = jpeg_exif.get_field(tag, ::exif::In::PRIMARY)?;
        match &field.value {
            ::exif::Value::Ascii(values) => values
                .first()
                .map(|v| String::from_utf8_lossy(v).into_owned()),
            _ => None,
        }
    }

    fn uint_field(jpeg_exif: &::exif::Exif, tag: ::exif::Tag) -> Option<u32> {
        jpeg_exif
            .get_field(tag, ::exif::In::PRIMARY)?
            .value
            .get_uint(0)
    }

    fn rational_field(jpeg_exif: &::exif::Exif, tag: ::exif::Tag) -> Vec<::exif::Rational> {
        match jpeg_exif
            .get_field(tag, ::exif::In::PRIMARY)
            .map(|f| &f.value)
        {
            Some(::exif::Value::Rational(values)) => values.clone(),
            _ => Vec::new(),
        }
    }

    fn camera_metadata() -> MetadataInput {
        MetadataInput {
            orientation: Orientation::Rotate90,
            capture_time: Some("2023-12-31T23:59:58".to_string()),
            make: Some("Canon".to_string()),
            model: Some("Canon EOS R5".to_string()),
            lens: Some("RF50mm F1.2 L USM".to_string()),
            iso: Some(400),
            exposure_time: Some("1/250".to_string()),
            f_number: Some("2.8".to_string()),
            focal_length: Some("50/1".to_string()),
            rating: Some(4),
            gps: None,
        }
    }

    #[test]
    fn round_trips_the_core_camera_facts() {
        let out = inject(&tiny_jpeg(), &camera_metadata()).expect("inject");
        let exif = read_exif(&out);

        assert_eq!(uint_field(&exif, ::exif::Tag::Orientation), Some(6));
        assert_eq!(
            ascii_field(&exif, ::exif::Tag::Make).as_deref(),
            Some("Canon")
        );
        assert_eq!(
            ascii_field(&exif, ::exif::Tag::Model).as_deref(),
            Some("Canon EOS R5")
        );
        assert_eq!(
            ascii_field(&exif, ::exif::Tag::DateTimeOriginal).as_deref(),
            Some("2023-12-31 23:59:58")
        );
        assert_eq!(
            ascii_field(&exif, ::exif::Tag::DateTime).as_deref(),
            Some("2023-12-31 23:59:58")
        );
        assert_eq!(
            uint_field(&exif, ::exif::Tag::PhotographicSensitivity),
            Some(400)
        );

        let exposure = rational_field(&exif, ::exif::Tag::ExposureTime);
        assert_eq!(exposure.len(), 1);
        assert!((exposure[0].to_f64() - 1.0 / 250.0).abs() < 1e-9);

        let f_number = rational_field(&exif, ::exif::Tag::FNumber);
        assert_eq!(f_number.len(), 1);
        assert!((f_number[0].to_f64() - 2.8).abs() < 1e-9);

        let focal = rational_field(&exif, ::exif::Tag::FocalLength);
        assert_eq!(focal.len(), 1);
        assert!((focal[0].to_f64() - 50.0).abs() < 1e-9);
    }

    /// Header segments, excluding scan data: `(marker, range_of_whole_segment)`.
    fn header_segments(jpeg: &[u8]) -> Vec<(u8, std::ops::Range<usize>)> {
        let mut out = Vec::new();
        let mut pos = 2;
        while pos + 4 <= jpeg.len() {
            assert_eq!(jpeg[pos], 0xFF, "test jpeg is well formed");
            let marker = jpeg[pos + 1];
            if marker == 0xDA {
                break;
            }
            let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
            out.push((marker, pos..pos + 2 + len));
            pos += 2 + len;
        }
        out
    }

    fn exif_app1_segments(jpeg: &[u8]) -> Vec<std::ops::Range<usize>> {
        header_segments(jpeg)
            .into_iter()
            .filter(|(marker, range)| {
                *marker == 0xE1 && jpeg[range.start + 4..range.end].starts_with(b"Exif\0\0")
            })
            .map(|(_, range)| range)
            .collect()
    }

    #[test]
    fn every_other_segment_and_the_scan_bytes_survive() {
        let mut original = vec![0xFF, 0xD8];
        original.extend(segment(0xE0, APP0));
        let xmp = b"http://ns.adobe.com/xap/1.0/\0<x:xmpmeta/>";
        original.extend(segment(0xE1, xmp));
        let icc = b"ICC_PROFILE\0\x01\x01profile-bytes";
        original.extend(segment(0xE2, icc));
        original.extend_from_slice(&tiny_jpeg()[2..]);

        let out = inject(&original, &camera_metadata()).expect("inject");

        // Exactly one segment was added, immediately after SOI; deleting it
        // must give the input back byte for byte.
        let inserted = exif_app1_segments(&out);
        assert_eq!(inserted.len(), 1);
        assert_eq!(inserted[0].start, 2);
        let mut stripped = out.clone();
        stripped.drain(inserted[0].clone());
        assert_eq!(stripped, original);

        // The scan bytes are untouched, and the non-EXIF APP1/APP2 are still there.
        let sos = original
            .windows(2)
            .position(|w| w == [0xFF, 0xDA])
            .expect("SOS present");
        let out_sos = out
            .windows(2)
            .position(|w| w == [0xFF, 0xDA])
            .expect("SOS present");
        assert_eq!(&out[out_sos..], &original[sos..]);
        assert!(out.windows(xmp.len()).any(|w| w == xmp));
        assert!(out.windows(icc.len()).any(|w| w == icc));
    }

    #[test]
    fn injecting_twice_leaves_exactly_one_exif_segment() {
        let once = inject(&tiny_jpeg(), &camera_metadata()).expect("first inject");
        let twice = inject(&once, &camera_metadata()).expect("second inject");
        assert_eq!(exif_app1_segments(&twice).len(), 1);
        assert_eq!(twice, once, "the same facts must produce the same bytes");
    }

    #[test]
    fn gps_round_trips_south_latitude() {
        let latitude = -(41.0 + 19.0 / 60.0 + 18.0 / 3600.0);
        let meta = MetadataInput {
            gps: Some((latitude, 174.7633)),
            ..MetadataInput::default()
        };

        let out = inject(&tiny_jpeg(), &meta).expect("inject");
        let exif = read_exif(&out);

        assert_eq!(
            ascii_field(&exif, ::exif::Tag::GPSLatitudeRef).as_deref(),
            Some("S")
        );
        assert_eq!(
            ascii_field(&exif, ::exif::Tag::GPSLongitudeRef).as_deref(),
            Some("E")
        );

        let parts = rational_field(&exif, ::exif::Tag::GPSLatitude);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].to_f64(), 41.0);
        assert_eq!(parts[1].to_f64(), 19.0);
        assert!((parts[2].to_f64() - 18.0).abs() < 1e-4);

        let decimal = parts[0].to_f64() + parts[1].to_f64() / 60.0 + parts[2].to_f64() / 3600.0;
        assert!(
            (decimal + latitude).abs() < 1e-4,
            "expected about {}, got {decimal}",
            latitude.abs()
        );
        assert!((decimal - 41.32).abs() < 0.01);
    }

    #[test]
    fn truncated_segments_are_errors_not_panics() {
        // The length field itself is cut in half.
        assert!(matches!(
            inject(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00], &camera_metadata()),
            Err(Error::JpegMalformed)
        ));
        // The declared length runs past the end of the buffer.
        assert!(matches!(
            inject(
                &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F'],
                &camera_metadata()
            ),
            Err(Error::JpegMalformed)
        ));
        // No prefix of a real JPEG may panic, whatever it cuts through.
        let valid = tiny_jpeg();
        for end in 0..=valid.len() {
            let _ = inject(&valid[..end], &camera_metadata());
        }
    }

    #[test]
    fn an_oversized_lens_string_still_fits_the_length_field() {
        let meta = MetadataInput {
            lens: Some("L".repeat(70_000)),
            ..camera_metadata()
        };
        let out = inject(&tiny_jpeg(), &meta).expect("inject");

        let ranges = exif_app1_segments(&out);
        assert_eq!(ranges.len(), 1);
        let length = u16::from_be_bytes([out[ranges[0].start + 2], out[ranges[0].start + 3]]);
        assert!(
            length as usize <= 65533,
            "segment length {length} exceeds 65533"
        );

        let exif = read_exif(&out);
        assert!(
            exif.get_field(::exif::Tag::LensModel, ::exif::In::PRIMARY)
                .is_none(),
            "the overly long lens must be dropped to make room"
        );
        assert_eq!(
            ascii_field(&exif, ::exif::Tag::Make).as_deref(),
            Some("Canon")
        );
        assert_eq!(uint_field(&exif, ::exif::Tag::Orientation), Some(6));
    }

    #[test]
    fn upright_orientation_is_written_when_other_facts_exist() {
        let meta = MetadataInput {
            make: Some("Nikon".to_string()),
            ..MetadataInput::default()
        };
        let out = inject(&tiny_jpeg(), &meta).expect("inject");
        assert_eq!(
            uint_field(&read_exif(&out), ::exif::Tag::Orientation),
            Some(1)
        );
    }

    #[test]
    fn capture_time_loses_its_fraction_and_zone() {
        for input in [
            "2023-12-31T23:59:58.123+13:00",
            "2023-12-31T23:59:58-05:00",
            "2023-12-31T23:59:58.5Z",
        ] {
            let meta = MetadataInput {
                capture_time: Some(input.to_string()),
                ..MetadataInput::default()
            };
            let exif = read_exif(&inject(&tiny_jpeg(), &meta).expect("inject"));
            assert_eq!(
                ascii_field(&exif, ::exif::Tag::DateTimeOriginal).as_deref(),
                Some("2023-12-31 23:59:58"),
                "input {input}"
            );
        }
    }

    #[test]
    fn an_unparseable_rational_is_omitted_without_failing_the_write() {
        let meta = MetadataInput {
            iso: Some(100),
            exposure_time: Some("not a rational".to_string()),
            f_number: Some("2.8".to_string()),
            ..MetadataInput::default()
        };
        let out = inject(&tiny_jpeg(), &meta).expect("inject");
        let exif = read_exif(&out);

        assert!(
            exif.get_field(::exif::Tag::ExposureTime, ::exif::In::PRIMARY)
                .is_none()
        );
        assert_eq!(rational_field(&exif, ::exif::Tag::FNumber).len(), 1);
        assert_eq!(
            uint_field(&exif, ::exif::Tag::PhotographicSensitivity),
            Some(100)
        );
    }
}
