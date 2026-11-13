//! Minimal EXIF writing: put the catalog's facts back into the recovered JPEG.
//!
//! The preview bytes are never modified — an APP1 segment is inserted (or
//! replaced) in front of the existing image data. See ADR 0003.

use std::ops::Range;
use std::sync::LazyLock;

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
    /// `Pyramid.colorProfile` from the catalog: `sRGB`, `AdobeRGB`, …
    ///
    /// This is not part of the XMP blob, so [`from_catalog`] leaves it `None`;
    /// the caller that holds the plan item fills it in.
    pub color_profile: Option<String>,
}

impl MetadataInput {
    /// True when there is nothing worth writing, EXIF or ICC.
    pub fn is_empty(&self) -> bool {
        !self.has_exif_fields() && self.color_profile.is_none()
    }

    /// True when the EXIF APP1 segment has anything to say.
    fn has_exif_fields(&self) -> bool {
        self.capture_time.is_some()
            || self.make.is_some()
            || self.model.is_some()
            || self.lens.is_some()
            || self.iso.is_some()
            || self.exposure_time.is_some()
            || self.f_number.is_some()
            || self.focal_length.is_some()
            || self.rating.is_some()
            || self.gps.is_some()
            || !self.orientation.is_upright()
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
        color_profile: None,
    }
}

/// The same input, plus the colour profile name `previews.db` recorded.
///
/// The profile name is not part of the XMP blob, so the extractor has to pass
/// it in from the plan item it is writing.
pub fn from_catalog_with_profile(
    xmp: &XmpMetadata,
    orientation: Orientation,
    color_profile: Option<String>,
) -> MetadataInput {
    MetadataInput {
        color_profile,
        ..from_catalog(xmp, orientation)
    }
}

/// What [`inject_full`] produced, plus anything it deliberately did not write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Injection {
    pub jpeg: Vec<u8>,
    /// Remarks about metadata the caller asked for but did not get.
    pub notes: Vec<String>,
}

/// Insert or replace the metadata segments. Returns a complete JPEG.
///
/// This is the entry point for callers that do not need the remarks from
/// [`inject_full`]. It still embeds the ICC profile when one is named, because
/// a profile is metadata too; only the XMP packet is left to [`inject_full`],
/// which keeps this function's "one EXIF segment added" contract.
pub fn inject(jpeg: &[u8], meta: &MetadataInput) -> Result<Vec<u8>> {
    Ok(inject_segments(jpeg, meta, false)?.jpeg)
}

/// Insert EXIF, XMP (star rating) and ICC metadata, and collect notes.
///
/// The returned bytes are the input with segments added in front of the first
/// existing segment; every segment kept and all scan data stay verbatim.
pub fn inject_full(jpeg: &[u8], meta: &MetadataInput) -> Result<Injection> {
    inject_segments(jpeg, meta, true)
}

fn inject_segments(jpeg: &[u8], meta: &MetadataInput, include_xmp: bool) -> Result<Injection> {
    if jpeg.len() < 2 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return Err(crate::error::Error::JpegMarkersMissing);
    }
    if meta.is_empty() {
        return Ok(Injection {
            jpeg: jpeg.to_vec(),
            notes: Vec::new(),
        });
    }

    let (segments, tail_start) = walk_header(jpeg)?;
    let mut notes = Vec::new();

    // New segments go immediately after SOI, in this order: EXIF, XMP, ICC.
    let mut inserted: Vec<u8> = Vec::new();
    let mut replace_exif = false;
    let mut replace_icc = false;

    if meta.has_exif_fields() {
        inserted.extend_from_slice(&build_app1(meta));
        replace_exif = true;
    }

    if include_xmp {
        if let Some(packet) = build_xmp(meta) {
            let present = segments.iter().any(|segment| {
                segment.marker == 0xE1 && segment.payload(jpeg).starts_with(XMP_PREFIX)
            });
            if present {
                notes.push(
                    "an XMP packet is already present; the star rating was left in EXIF only"
                        .to_string(),
                );
            } else {
                inserted.extend_from_slice(&packet);
            }
        }
    }

    if let Some(name) = meta.color_profile.as_deref() {
        match icc_profile(name) {
            Some(profile) => {
                inserted.extend_from_slice(&icc_segments(profile)?);
                replace_icc = true;
            }
            None => notes.push(format!(
                "colour profile `{name}` has no built-in ICC profile; none embedded"
            )),
        }
    }

    let mut out = Vec::with_capacity(jpeg.len() + inserted.len());
    out.extend_from_slice(&jpeg[..2]);
    out.extend_from_slice(&inserted);
    for segment in &segments {
        let replaced = (replace_exif
            && segment.marker == 0xE1
            && segment.payload(jpeg).starts_with(EXIF_PREFIX))
            || (replace_icc
                && segment.marker == 0xE2
                && segment.payload(jpeg).starts_with(ICC_PREFIX));
        if !replaced {
            out.extend_from_slice(&jpeg[segment.range.clone()]);
        }
    }
    out.extend_from_slice(&jpeg[tail_start..]);
    Ok(Injection { jpeg: out, notes })
}

/// Insert `profile` as `ICC_PROFILE` APP2 segments, replacing any earlier ones.
///
/// ICC.1 Annex B allows at most 65,519 bytes of profile per segment; larger
/// profiles are split, numbered from 1, with the total chunk count in the
/// second byte. Every other segment and all scan data are copied verbatim.
/// An empty profile simply removes any profile the payload carried.
pub fn inject_icc(jpeg: &[u8], profile: &[u8]) -> Result<Vec<u8>> {
    if jpeg.len() < 2 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return Err(crate::error::Error::JpegMarkersMissing);
    }

    let (segments, tail_start) = walk_header(jpeg)?;
    let inserted = icc_segments(profile)?;

    let mut out = Vec::with_capacity(jpeg.len() + inserted.len());
    out.extend_from_slice(&jpeg[..2]);
    out.extend_from_slice(&inserted);
    for segment in &segments {
        if segment.marker == 0xE2 && segment.payload(jpeg).starts_with(ICC_PREFIX) {
            continue;
        }
        out.extend_from_slice(&jpeg[segment.range.clone()]);
    }
    out.extend_from_slice(&jpeg[tail_start..]);
    Ok(out)
}

/// The EXIF identifier that opens an APP1 payload.
const EXIF_PREFIX: &[u8] = b"Exif\0\0";
/// The XMP identifier that opens an APP1 payload.
const XMP_PREFIX: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
/// The identifier that opens every ICC APP2 payload.
const ICC_PREFIX: &[u8] = b"ICC_PROFILE\0";

/// Largest legal value of a segment's two-byte length field.
const MAX_SEGMENT_LENGTH: usize = u16::MAX as usize - 2;
/// Largest APP1 payload whose `+ 2` length field still fits.
const MAX_APP1_PAYLOAD: usize = MAX_SEGMENT_LENGTH - 2;
/// Largest profile bytes one APP2 segment can carry (ICC.1 Annex B.4).
const ICC_CHUNK_MAX: usize = MAX_SEGMENT_LENGTH - ICC_PREFIX.len() - 2;

/// A complete marker segment: `FF marker`, big-endian length, payload.
fn segment_bytes(marker: u8, payload: &[u8]) -> Vec<u8> {
    let length = u16::try_from(payload.len() + 2).expect("callers keep payloads below 65533");
    let mut out = Vec::with_capacity(payload.len() + 4);
    out.extend_from_slice(&[0xFF, marker]);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// Chunk `profile` into complete `ICC_PROFILE` APP2 segments.
fn icc_segments(profile: &[u8]) -> Result<Vec<u8>> {
    if profile.is_empty() {
        return Ok(Vec::new());
    }
    let chunks: Vec<&[u8]> = profile.chunks(ICC_CHUNK_MAX).collect();
    let count =
        u8::try_from(chunks.len()).map_err(|_| crate::error::Error::IccTooLarge(profile.len()))?;

    let mut out = Vec::with_capacity(profile.len() + 16 * chunks.len());
    for (index, chunk) in chunks.into_iter().enumerate() {
        let mut payload = Vec::with_capacity(ICC_PREFIX.len() + 2 + chunk.len());
        payload.extend_from_slice(ICC_PREFIX);
        payload.push(u8::try_from(index + 1).expect("index is below the chunk count"));
        payload.push(count);
        payload.extend_from_slice(chunk);
        out.extend_from_slice(&segment_bytes(0xE2, &payload));
    }
    Ok(out)
}

/// A minimal XMP packet carrying the star rating, as a complete APP1 segment.
///
/// XMP is all-or-nothing here: merging into a packet the payload already
/// carries is not attempted, so [`inject_segments`] leaves an existing packet
/// alone and says so in the notes.
fn build_xmp(meta: &MetadataInput) -> Option<Vec<u8>> {
    let rating = meta.rating?;
    let packet = format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
         <rdf:Description rdf:about=\"\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" \
         xmp:Rating=\"{rating}\"/>\n\
         </rdf:RDF>\n\
         </x:xmpmeta>\n\
         <?xpacket end=\"w\"?>"
    );
    let mut payload = Vec::with_capacity(XMP_PREFIX.len() + packet.len());
    payload.extend_from_slice(XMP_PREFIX);
    payload.extend_from_slice(packet.as_bytes());
    Some(segment_bytes(0xE1, &payload))
}

// TIFF field types, all little-endian.
const TYPE_BYTE: u16 = 1;
const TYPE_ASCII: u16 = 2;
const TYPE_SHORT: u16 = 3;
const TYPE_LONG: u16 = 4;
const TYPE_RATIONAL: u16 = 5;

/// One marker segment in the JPEG header, before scan data begins.
#[derive(Debug, Clone)]
struct HeaderSegment {
    /// Byte range of the whole segment, including `FF` and the length field.
    range: Range<usize>,
    marker: u8,
    /// Offset of the payload, i.e. just after the two-byte length field.
    payload: usize,
}

impl HeaderSegment {
    /// The segment payload, empty for standalone markers.
    fn payload<'a>(&self, jpeg: &'a [u8]) -> &'a [u8] {
        &jpeg[self.payload..self.range.end]
    }
}

/// Walk the header of a JPEG, returning its segments and the offset where scan
/// (or trailer) data begins.
///
/// Nothing is filtered here: insertion and replacement both need to see every
/// segment, so the caller decides what to drop.
fn walk_header(jpeg: &[u8]) -> Result<(Vec<HeaderSegment>, usize)> {
    let mut segments = Vec::new();
    let mut pos = 2;

    loop {
        if pos >= jpeg.len() {
            return Ok((segments, pos));
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
            0x01 | 0xD8 | 0xD0..=0xD7 => segments.push(HeaderSegment {
                range: start..pos,
                marker,
                payload: pos,
            }),
            // From SOS and EOI onward the original bytes are copied verbatim.
            0xD9 | 0xDA => return Ok((segments, start)),
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
                segments.push(HeaderSegment {
                    range: start..end,
                    marker,
                    payload: pos + 2,
                });
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
    if let Some(rating) = meta.rating {
        // 0x4746 Rating, the EXIF-compatible spelling of `xmp:Rating`.
        ifd0.push(short_entry(0x4746, u16::from(rating)));
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

// ---------------------------------------------------------------------------
// ICC profiles
// ---------------------------------------------------------------------------

/// The reconstructed ICC profile for a `Pyramid.colorProfile` name.
///
/// The catalog records only a *name*, never profile bytes
/// (`docs/format-notes.md` §6.4), so these are built here: ICC v2 matrix/TRC
/// display profiles with D50-adapted primaries, a D50 media white point,
/// `desc`, `cprt` and one tone curve per channel. Each is under 4 KB; the
/// tests parse them back as ICC v2 and check every declared offset.
///
/// Names are matched case-insensitively and ignoring spaces and punctuation,
/// so `AdobeRGB`, `Adobe RGB (1998)` and `adobergb1998` all resolve.
/// An unrecognised name returns `None`: a wrong profile is worse than none.
pub fn icc_profile(name: &str) -> Option<&'static [u8]> {
    let key: String = name
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect();
    match key.as_str() {
        "srgb" => Some(SRGB_PROFILE.as_slice()),
        "adobergb" | "adobergb1998" => Some(ADOBE_RGB_PROFILE.as_slice()),
        _ => None,
    }
}

static SRGB_PROFILE: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let curve = curve_tag(&srgb_curve(256));
    build_icc_profile(
        "sRGB IEC61966-2.1",
        "Public domain. Reconstructed by lrsalvage from the catalog's profile name.",
        SRGB_PRIMARIES,
        [curve.clone(), curve.clone(), curve],
    )
});

static ADOBE_RGB_PROFILE: LazyLock<Vec<u8>> = LazyLock::new(|| {
    // Adobe RGB (1998) is defined with a gamma of 563/256.
    let curve = gamma_curve(563.0 / 256.0);
    build_icc_profile(
        "Adobe RGB (1998)",
        "Public domain. Reconstructed by lrsalvage from the catalog's profile name.",
        ADOBE_RGB_PRIMARIES,
        [curve.clone(), curve.clone(), curve],
    )
});

/// Size of the ICC header everything else is offset from.
const ICC_HEADER_LEN: usize = 128;

/// The profile connection space illuminant, D50, in s15Fixed16.
const ICC_WHITE: [f64; 3] = [0.9642, 1.0, 0.8249];

/// sRGB colourants, D50-adapted (ICC / IEC 61966-2-1 values).
const SRGB_PRIMARIES: [[f64; 3]; 3] = [
    [0.4360, 0.2225, 0.0139],
    [0.3851, 0.7169, 0.0971],
    [0.1431, 0.0606, 0.7141],
];

/// Adobe RGB (1998) colourants, D50-adapted.
const ADOBE_RGB_PRIMARIES: [[f64; 3]; 3] = [
    [0.6097, 0.3111, 0.0195],
    [0.2053, 0.6257, 0.0609],
    [0.1492, 0.0632, 0.7446],
];

/// Assemble a complete ICC v2 matrix/TRC profile.
///
/// `curves` is the red, green and blue tone curve tag, in that order. The tag
/// table is sorted by signature and every data element starts on a four-byte
/// boundary, as ICC.1:2001 requires.
fn build_icc_profile(
    description: &str,
    copyright: &str,
    primaries: [[f64; 3]; 3],
    curves: [Vec<u8>; 3],
) -> Vec<u8> {
    let [red, green, blue] = curves;
    let mut tags: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"desc", desc_tag(description)),
        (*b"cprt", text_tag(copyright)),
        (*b"wtpt", xyz_tag(ICC_WHITE)),
        (*b"rXYZ", xyz_tag(primaries[0])),
        (*b"gXYZ", xyz_tag(primaries[1])),
        (*b"bXYZ", xyz_tag(primaries[2])),
        (*b"rTRC", red),
        (*b"gTRC", green),
        (*b"bTRC", blue),
    ];
    tags.sort_by_key(|tag| tag.0);

    let table_len = 4 + 12 * tags.len();
    let mut table = Vec::with_capacity(table_len);
    table.extend_from_slice(&(tags.len() as u32).to_be_bytes());

    let mut data: Vec<u8> = Vec::new();
    for (signature, bytes) in &tags {
        while (ICC_HEADER_LEN + table_len + data.len()) % 4 != 0 {
            data.push(0);
        }
        let offset = ICC_HEADER_LEN + table_len + data.len();
        table.extend_from_slice(signature);
        table.extend_from_slice(&(offset as u32).to_be_bytes());
        table.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        data.extend_from_slice(bytes);
    }
    while (ICC_HEADER_LEN + table_len + data.len()) % 4 != 0 {
        data.push(0);
    }

    let total = ICC_HEADER_LEN + table_len + data.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&icc_header(total));
    out.extend_from_slice(&table);
    out.extend_from_slice(&data);
    debug_assert_eq!(out.len(), total);
    out
}

/// The 128-byte ICC header, with the declared profile size filled in.
fn icc_header(total: usize) -> Vec<u8> {
    let mut header = vec![0u8; ICC_HEADER_LEN];
    header[0..4].copy_from_slice(&(total as u32).to_be_bytes());
    // Preferred CMM type: left at zero, as ICC.1 allows when none is stated.
    // Version 2.1.0: a v2 profile, not v4.
    header[8..12].copy_from_slice(&0x0210_0000u32.to_be_bytes());
    header[12..16].copy_from_slice(b"mntr");
    header[16..20].copy_from_slice(b"RGB ");
    header[20..24].copy_from_slice(b"XYZ ");
    // A fixed creation date keeps the bytes reproducible run to run.
    for (offset, value) in [
        (24usize, 2026u16),
        (26, 11),
        (28, 12),
        (30, 0),
        (32, 0),
        (34, 0),
    ] {
        header[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
    }
    header[36..40].copy_from_slice(b"acsp");
    // Rendering intent 0 (perceptual), PCS illuminant D50, no profile ID.
    header[68..72].copy_from_slice(&0x0000_F6D6u32.to_be_bytes());
    header[72..76].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    header[76..80].copy_from_slice(&0x0000_D32Du32.to_be_bytes());
    header
}

/// `s15Fixed16Number`: an ICC fixed-point XYZ component.
fn s15f16(value: f64) -> i32 {
    (value * 65_536.0).round() as i32
}

/// An `XYZType` tag holding one colorant or white point.
fn xyz_tag(xyz: [f64; 3]) -> Vec<u8> {
    let mut out = Vec::with_capacity(20);
    out.extend_from_slice(b"XYZ ");
    out.extend_from_slice(&[0u8; 4]);
    for value in xyz {
        out.extend_from_slice(&s15f16(value).to_be_bytes());
    }
    out
}

/// A `curveType` tag from explicit samples.
fn curve_tag(samples: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + samples.len() * 2);
    out.extend_from_slice(b"curv");
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&(samples.len() as u32).to_be_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_be_bytes());
    }
    out
}

/// A `curveType` tag that is a single gamma value in u8Fixed8 notation.
fn gamma_curve(gamma: f64) -> Vec<u8> {
    curve_tag(&[(gamma * 256.0).round() as u16])
}

/// The IEC 61966-2-1 transfer function, sampled for a v2 `curv` tag.
fn srgb_curve(samples: usize) -> Vec<u16> {
    (0..samples)
        .map(|index| {
            let x = index as f64 / (samples - 1) as f64;
            let y = if x <= 0.04045 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            };
            (y * 65_535.0).round().clamp(0.0, 65_535.0) as u16
        })
        .collect()
}

/// A v2 `textDescriptionType` tag (`desc`).
fn desc_tag(text: &str) -> Vec<u8> {
    let ascii = text.as_bytes();
    let mut out = Vec::new();
    out.extend_from_slice(b"desc");
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&((ascii.len() + 1) as u32).to_be_bytes());
    out.extend_from_slice(ascii);
    out.push(0);
    // No Unicode or ScriptCode description.
    out.extend_from_slice(&[0u8; 4]); // Unicode language code
    out.extend_from_slice(&[0u8; 4]); // Unicode count
    out.extend_from_slice(&[0u8; 2]); // ScriptCode code
    out.push(0); // ScriptCode count
    out.extend_from_slice(&[0u8; 67]); // ScriptCode string
    out
}

/// A v2 `textType` tag (`cprt`).
fn text_tag(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + text.len() + 1);
    out.extend_from_slice(b"text");
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(text.as_bytes());
    out.push(0);
    out
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
            color_profile: None,
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
    fn header_pairs(jpeg: &[u8]) -> Vec<(u8, std::ops::Range<usize>)> {
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
        header_pairs(jpeg)
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

    // -- ICC profiles ------------------------------------------------------

    fn be32(bytes: &[u8]) -> u32 {
        u32::from_be_bytes(bytes.try_into().expect("four bytes"))
    }

    /// `(signature, offset, size)` for every tag table entry.
    fn icc_tags(profile: &[u8]) -> Vec<([u8; 4], usize, usize)> {
        let count = be32(&profile[ICC_HEADER_LEN..ICC_HEADER_LEN + 4]) as usize;
        profile[ICC_HEADER_LEN + 4..ICC_HEADER_LEN + 4 + count * 12]
            .chunks_exact(12)
            .map(|entry| {
                (
                    entry[0..4].try_into().expect("four bytes"),
                    be32(&entry[4..8]) as usize,
                    be32(&entry[8..12]) as usize,
                )
            })
            .collect()
    }

    /// APP2 payloads of every segment that opens with the ICC identifier.
    fn icc_app2(jpeg: &[u8]) -> Vec<&[u8]> {
        let (segments, _) = walk_header(jpeg).expect("test jpeg is well formed");
        segments
            .into_iter()
            .filter(|segment| {
                segment.marker == 0xE2 && segment.payload(jpeg).starts_with(ICC_PREFIX)
            })
            .map(|segment| segment.payload(jpeg))
            .collect()
    }

    /// APP1 payloads of every segment that opens with the XMP identifier.
    fn xmp_app1(jpeg: &[u8]) -> Vec<&[u8]> {
        let (segments, _) = walk_header(jpeg).expect("test jpeg is well formed");
        segments
            .into_iter()
            .filter(|segment| {
                segment.marker == 0xE1 && segment.payload(jpeg).starts_with(XMP_PREFIX)
            })
            .map(|segment| segment.payload(jpeg))
            .collect()
    }

    /// IFD0 tag 0x4746 (Rating), which `kamadak-exif` has no constant for.
    fn rating_field(exif: &::exif::Exif) -> Option<u32> {
        exif.get_field(
            ::exif::Tag(::exif::Context::Tiff, 0x4746),
            ::exif::In::PRIMARY,
        )?
        .value
        .get_uint(0)
    }

    #[test]
    fn icc_profiles_parse_as_valid_v2_profiles() {
        for name in ["sRGB", "AdobeRGB"] {
            let profile = icc_profile(name).unwrap_or_else(|| panic!("{name} must have a profile"));
            assert!(
                (ICC_HEADER_LEN + 4..4096).contains(&profile.len()),
                "{name}: {} bytes is outside the size budget",
                profile.len()
            );

            assert_eq!(
                be32(&profile[0..4]) as usize,
                profile.len(),
                "{name}: declared size must equal the actual size"
            );
            assert_eq!(profile[8], 2, "{name}: must be an ICC v2 profile");
            assert_eq!(&profile[12..16], b"mntr", "{name}: profile class");
            assert_eq!(&profile[16..20], b"RGB ", "{name}: colour space");
            assert_eq!(&profile[20..24], b"XYZ ", "{name}: PCS");
            assert_eq!(&profile[36..40], b"acsp", "{name}: acsp signature");

            let tags = icc_tags(profile);
            assert!(tags.len() >= 9, "{name}: only {} tags", tags.len());
            let table_end = ICC_HEADER_LEN + 4 + tags.len() * 12;
            assert!(
                table_end <= profile.len(),
                "{name}: tag table runs past the end"
            );
            assert!(
                tags.windows(2).all(|pair| pair[0].0 < pair[1].0),
                "{name}: tags must be sorted and unique"
            );
            for (signature, offset, size) in &tags {
                assert!(
                    *offset >= table_end,
                    "{name}: {:?} overlaps the tag table",
                    String::from_utf8_lossy(signature)
                );
                assert_eq!(*offset % 4, 0, "{name}: {signature:?} is not aligned");
                assert!(
                    offset + size <= profile.len(),
                    "{name}: {:?} runs past the end",
                    String::from_utf8_lossy(signature)
                );
            }

            for (signature, kind) in [
                (b"desc", b"desc"),
                (b"cprt", b"text"),
                (b"wtpt", b"XYZ "),
                (b"rXYZ", b"XYZ "),
                (b"gXYZ", b"XYZ "),
                (b"bXYZ", b"XYZ "),
                (b"rTRC", b"curv"),
                (b"gTRC", b"curv"),
                (b"bTRC", b"curv"),
            ] {
                let (_, offset, _) = tags
                    .iter()
                    .find(|(seen, _, _)| seen == signature)
                    .unwrap_or_else(|| {
                        panic!("{name}: missing {} tag", String::from_utf8_lossy(signature))
                    });
                assert_eq!(
                    &profile[*offset..*offset + 4],
                    kind,
                    "{name}: {:?} has the wrong data type",
                    String::from_utf8_lossy(signature)
                );
            }
        }
    }

    #[test]
    fn profile_names_resolve_case_and_punctuation_insensitively() {
        assert_eq!(icc_profile("sRGB"), Some(SRGB_PROFILE.as_slice()));
        assert_eq!(icc_profile("SRGB"), icc_profile("sRGB"));
        assert_eq!(icc_profile("Adobe RGB (1998)"), icc_profile("AdobeRGB"));
        assert_eq!(icc_profile("adobergb1998"), icc_profile("AdobeRGB"));
        assert_ne!(icc_profile("sRGB"), icc_profile("AdobeRGB"));
        assert!(
            icc_profile("ACR").is_none(),
            "a name we cannot build is None"
        );
        assert!(icc_profile("").is_none());
    }

    #[test]
    fn inject_icc_round_trips_a_profile_and_leaves_the_image_alone() {
        let jpeg = tiny_jpeg();
        let profile = icc_profile("sRGB").expect("sRGB profile");
        let out = inject_icc(&jpeg, profile).expect("inject icc");

        let payloads = icc_app2(&out);
        assert_eq!(payloads.len(), 1, "a small profile needs one segment");
        assert!(payloads[0].starts_with(ICC_PREFIX));
        assert_eq!(payloads[0][ICC_PREFIX.len()], 1, "sequence starts at 1");
        assert_eq!(payloads[0][ICC_PREFIX.len() + 1], 1, "one chunk in total");
        assert_eq!(&payloads[0][ICC_PREFIX.len() + 2..], profile);

        // A naive scan for the identifier finds it, and only it.
        let naive = out
            .windows(ICC_PREFIX.len())
            .filter(|window| *window == ICC_PREFIX)
            .count();
        assert_eq!(naive, 1, "the identifier appears exactly once");

        // The segment follows SOI and every byte after it is the input.
        let inserted = 4 + payloads[0].len();
        assert_eq!(&out[2 + inserted..], &jpeg[2..]);
        assert_eq!(&out[..2], [0xFF, 0xD8]);
    }

    #[test]
    fn a_profile_larger_than_one_segment_is_chunked_in_sequence() {
        let profile: Vec<u8> = (0..ICC_CHUNK_MAX * 2 + 7)
            .map(|value| (value % 251) as u8)
            .collect();
        let out = inject_icc(&tiny_jpeg(), &profile).expect("inject icc");

        let payloads = icc_app2(&out);
        assert_eq!(payloads.len(), 3);
        let mut reassembled = Vec::new();
        for (index, payload) in payloads.iter().enumerate() {
            assert_eq!(payload[12], u8::try_from(index + 1).unwrap(), "sequence");
            assert_eq!(payload[13], 3, "chunk count");
            assert!(payload.len() <= MAX_SEGMENT_LENGTH);
            reassembled.extend_from_slice(&payload[14..]);
        }
        assert_eq!(
            reassembled, profile,
            "chunks concatenate back to the profile"
        );
    }

    #[test]
    fn injecting_a_profile_replaces_any_earlier_one() {
        let mut original = vec![0xFF, 0xD8];
        original.extend(segment(0xE2, b"ICC_PROFILE\0\x01\x01STALE-PROFILE-BYTES"));
        original.extend_from_slice(&tiny_jpeg()[2..]);

        let engine = icc_profile("AdobeRGB").expect("AdobeRGB profile");
        let once = inject_icc(&original, engine).expect("inject icc");
        assert_eq!(icc_app2(&once).len(), 1);
        assert!(
            !once
                .windows(b"STALE-PROFILE-BYTES".len())
                .any(|window| window == b"STALE-PROFILE-BYTES"),
            "the old profile must be gone"
        );

        let twice = inject_icc(&once, engine).expect("inject icc again");
        assert_eq!(twice, once, "injection is idempotent");
    }

    #[test]
    fn the_profile_name_is_carried_from_the_plan_not_the_xmp_blob() {
        let xmp = XmpMetadata {
            make: Some("Canon".to_string()),
            ..XmpMetadata::default()
        };

        let meta = from_catalog_with_profile(&xmp, Orientation::Normal, Some("AdobeRGB".into()));
        assert_eq!(meta.color_profile.as_deref(), Some("AdobeRGB"));
        assert_eq!(meta.make.as_deref(), Some("Canon"));

        let without = from_catalog(&xmp, Orientation::Normal);
        assert!(
            without.color_profile.is_none(),
            "the XMP blob never carries the profile name"
        );
    }

    #[test]
    fn a_rating_alone_is_enough_to_write_exif() {
        let meta = MetadataInput {
            rating: Some(5),
            ..MetadataInput::default()
        };
        assert!(
            !meta.is_empty(),
            "a rating-only image still produces output"
        );

        let out = inject(&tiny_jpeg(), &meta).expect("inject");
        let exif = read_exif(&out);
        assert_eq!(rating_field(&exif), Some(5));
        assert_eq!(uint_field(&exif, ::exif::Tag::Orientation), Some(1));
        assert!(
            xmp_app1(&out).is_empty(),
            "inject keeps its one-added-segment contract"
        );
    }

    #[test]
    fn full_injection_embeds_exif_xmp_and_the_colour_profile() {
        let meta = MetadataInput {
            rating: Some(5),
            color_profile: Some("sRGB".to_string()),
            ..camera_metadata()
        };
        let result = inject_full(&tiny_jpeg(), &meta).expect("inject");
        assert!(result.notes.is_empty(), "{:?}", result.notes);

        let exif = read_exif(&result.jpeg);
        assert_eq!(rating_field(&exif), Some(5));
        assert_eq!(
            ascii_field(&exif, ::exif::Tag::Make).as_deref(),
            Some("Canon")
        );

        let packets = xmp_app1(&result.jpeg);
        assert_eq!(packets.len(), 1, "one XMP packet");
        let packet = String::from_utf8_lossy(packets[0]);
        assert!(packet.contains("xmp:Rating=\"5\""), "{packet}");

        let profiles = icc_app2(&result.jpeg);
        assert_eq!(profiles.len(), 1, "one ICC segment");
        assert_eq!(
            &profiles[0][ICC_PREFIX.len() + 2..],
            icc_profile("sRGB").expect("sRGB")
        );
    }

    #[test]
    fn a_colour_profile_alone_embeds_icc_without_an_exif_segment() {
        let mut original = vec![0xFF, 0xD8];
        original.extend(segment(0xE2, b"ICC_PROFILE\0\x01\x01STALE-PROFILE-BYTES"));
        original.extend_from_slice(&tiny_jpeg()[2..]);

        let meta = MetadataInput {
            color_profile: Some("sRGB".to_string()),
            ..MetadataInput::default()
        };
        let out = inject(&original, &meta).expect("inject");

        assert_eq!(icc_app2(&out).len(), 1);
        assert!(
            exif_app1_segments(&out).is_empty(),
            "nothing for EXIF to say"
        );
    }

    #[test]
    fn an_unknown_colour_profile_is_reported_not_invented() {
        let meta = MetadataInput {
            color_profile: Some("ACR".to_string()),
            ..MetadataInput::default()
        };
        let result = inject_full(&tiny_jpeg(), &meta).expect("inject");

        assert_eq!(result.jpeg, tiny_jpeg(), "nothing was added");
        assert_eq!(result.notes.len(), 1);
        assert!(result.notes[0].contains("ACR"), "{:?}", result.notes);
        assert!(icc_app2(&result.jpeg).is_empty());
    }

    #[test]
    fn an_existing_xmp_packet_is_left_alone_and_the_omission_is_noted() {
        let mut original = vec![0xFF, 0xD8];
        let packet = b"http://ns.adobe.com/xap/1.0/\0<x:xmpmeta xmp:Rating=\"2\"/>";
        original.extend(segment(0xE1, packet));
        original.extend_from_slice(&tiny_jpeg()[2..]);

        let meta = MetadataInput {
            rating: Some(5),
            ..MetadataInput::default()
        };
        let result = inject_full(&original, &meta).expect("inject");

        assert_eq!(
            result
                .jpeg
                .windows(packet.len())
                .filter(|window| *window == packet)
                .count(),
            1,
            "the payload's own packet survives and is not duplicated"
        );
        assert_eq!(result.notes.len(), 1);
        assert!(result.notes[0].contains("XMP"), "{:?}", result.notes);
        assert_eq!(
            rating_field(&read_exif(&result.jpeg)),
            Some(5),
            "the rating still lands in EXIF"
        );
    }
}
