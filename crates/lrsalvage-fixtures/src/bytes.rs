//! Raw byte shapes, no scenario.
//!
//! The only place a leaf name, a chunk header or a JPEG frame is constructed.
//! Every layout here is the one `specs/fixtures.md` names, and each shape is
//! verified against the module that reads it (`jpeg`, `lrprev`) rather than
//! against this file's own opinion.

// ---------------------------------------------------------------------------
// JPEG frames
// ---------------------------------------------------------------------------

/// A structurally valid baseline JPEG at `width` x `height`: SOI, APP0, SOF0,
/// SOS, one entropy byte, EOI.
///
/// Tiny, but `jpeg::dimensions` finds the SOF and `jpeg::is_complete` finds the
/// final EOI. No pixel is ever decoded.
pub fn jpeg(width: u16, height: u16) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8];
    bytes.extend_from_slice(&frame(width, height));
    bytes
}

/// The same frame, with `segments` (complete APP segments) in front of it.
///
/// `segments` are already-framed bytes, so `jpeg_with(w, h, &[exif_app1(6)])`
/// reads as the spec's `[exif_app1(6), xmp_app1(x), icc_app2(p)]`.
pub fn jpeg_with(width: u16, height: u16, segments: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8];
    for segment in segments {
        bytes.extend_from_slice(segment);
    }
    bytes.extend_from_slice(&frame(width, height));
    bytes
}

/// A frame with no EOI: complete header, cut-off scan.
pub fn truncated_jpeg(width: u16, height: u16) -> Vec<u8> {
    let mut bytes = jpeg(width, height);
    bytes.truncate(bytes.len() - 2);
    bytes
}

/// The body without its SOI — the damage ADR 0001 says must not be scanned for.
pub fn no_soi(width: u16, height: u16) -> Vec<u8> {
    frame(width, height)
}

/// Bytes that never were an image.
pub fn not_a_jpeg() -> Vec<u8> {
    b"These bytes were never a JPEG preview.\n".to_vec()
}

/// A destination we could have written: complete, SOF `(width, height)`, and
/// IFD0 0x0131 (Software) = `lrsalvage <version>`.
pub fn our_output(width: u16, height: u16, version: &str) -> Vec<u8> {
    foreign_output(width, height, &format!("lrsalvage {version}"))
}

/// A destination somebody else wrote: complete, SOF `(width, height)`, and
/// IFD0 0x0131 (Software) = `software` — a marker that is not ours.
///
/// The counterpart of [`our_output`]: the resume compares the marker's
/// `lrsalvage ` prefix rather than any `Software` value, so a file this shape
/// produces must be refused, never kept.
pub fn foreign_output(width: u16, height: u16, software: &str) -> Vec<u8> {
    let exif = exif_with(&[ascii_entry(0x0131, software)]);
    jpeg_with(width, height, &[exif])
}

/// Everything after the SOI segment: APP0, SOF0, SOS, one entropy byte, EOI.
fn frame(width: u16, height: u16) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10]);
    bytes.extend_from_slice(b"JFIF\0");
    bytes.extend_from_slice(&[0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
    bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&[0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
    bytes.extend_from_slice(&[
        0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00,
    ]);
    bytes.extend_from_slice(&[0x00, 0xFF, 0xD9]);
    bytes
}

// ---------------------------------------------------------------------------
// APP segments
// ---------------------------------------------------------------------------

/// An `Exif` APP1 whose IFD0 carries orientation tag 0x0112.
pub fn exif_app1(orientation: u16) -> Vec<u8> {
    exif_with(&[short_entry(0x0112, orientation)])
}

/// An XMP packet as its own APP1: the standard signature, then `xml`.
pub fn xmp_app1(xml: &str) -> Vec<u8> {
    let mut payload = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    payload.extend_from_slice(xml.as_bytes());
    segment(0xE1, &payload)
}

/// An `ICC_PROFILE` APP2, split across as many segments as a real profile of
/// that size needs. The segments are returned end to end, ready to hand to
/// [`jpeg_with`] as one entry.
pub fn icc_app2(profile: &[u8]) -> Vec<u8> {
    // A segment's length field includes the two bytes counting itself, so the
    // payload ceiling is 65535 - 2, less the 14-byte ICC_PROFILE header.
    const MAX: usize = 65_533 - 14;
    let count = profile.len().div_ceil(MAX).max(1);
    let count = u8::try_from(count).expect("an ICC profile needs at most 255 segments");

    let mut bytes = Vec::new();
    for (index, chunk) in profile.chunks(MAX).enumerate() {
        let mut payload = b"ICC_PROFILE\0".to_vec();
        payload.push(u8::try_from(index + 1).expect("segment number fits a u8"));
        payload.push(count);
        payload.extend_from_slice(chunk);
        bytes.extend_from_slice(&segment(0xE2, &payload));
    }
    bytes
}

/// One APP segment: marker, big-endian length (including itself), payload.
fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    let length = u16::try_from(payload.len() + 2).expect("an APP segment payload fits a u16");
    let mut bytes = Vec::with_capacity(payload.len() + 4);
    bytes.extend_from_slice(&[0xFF, marker]);
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

/// One IFD0 entry of a TIFF/EXIF IFD.
struct Entry {
    tag: u16,
    kind: u16,
    count: u32,
    value: Vec<u8>,
}

const TYPE_ASCII: u16 = 2;
const TYPE_SHORT: u16 = 3;

/// A 0x0112-style single `SHORT`, big-endian.
fn short_entry(tag: u16, value: u16) -> Entry {
    Entry {
        tag,
        kind: TYPE_SHORT,
        count: 1,
        value: value.to_be_bytes().to_vec(),
    }
}

/// A NUL-terminated `ASCII` value; `count` includes the terminator.
fn ascii_entry(tag: u16, text: &str) -> Entry {
    let mut value = text.as_bytes().to_vec();
    value.push(0);
    Entry {
        tag,
        kind: TYPE_ASCII,
        count: value.len() as u32,
        value,
    }
}

/// An `Exif` APP1 whose IFD0 holds `entries`.
fn exif_with(entries: &[Entry]) -> Vec<u8> {
    let mut payload = b"Exif\0\0".to_vec();
    payload.extend_from_slice(&tiff(entries));
    segment(0xE1, &payload)
}

/// A big-endian TIFF image file directory at offset 8, values in a data area
/// after it. Nothing here ever needs a second IFD.
fn tiff(entries: &[Entry]) -> Vec<u8> {
    let ifd_offset = 8u32;
    let count = u16::try_from(entries.len()).expect("an IFD fits a u16 entry count");
    // count + entries + next-IFD offset, all before the data area.
    let entries_len = 2 + 12 * entries.len() as u32 + 4;
    let mut data = Vec::new();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"MM");
    bytes.extend_from_slice(&42u16.to_be_bytes());
    bytes.extend_from_slice(&ifd_offset.to_be_bytes());
    bytes.extend_from_slice(&count.to_be_bytes());

    for entry in entries {
        bytes.extend_from_slice(&entry.tag.to_be_bytes());
        bytes.extend_from_slice(&entry.kind.to_be_bytes());
        bytes.extend_from_slice(&entry.count.to_be_bytes());
        if entry.value.len() <= 4 {
            let mut inline = entry.value.clone();
            inline.resize(4, 0);
            bytes.extend_from_slice(&inline);
        } else {
            // Offsets are from the TIFF header, and an even offset keeps the
            // data area word-aligned the way EXIF wants it.
            let offset = ifd_offset + entries_len + data.len() as u32;
            bytes.extend_from_slice(&offset.to_be_bytes());
            data.extend_from_slice(&entry.value);
            if data.len() % 2 == 1 {
                data.push(0);
            }
        }
    }
    bytes.extend_from_slice(&0u32.to_be_bytes()); // no next IFD
    bytes.extend_from_slice(&data);
    bytes
}

// ---------------------------------------------------------------------------
// The `AgHg` container (`.lrprev`)
// ---------------------------------------------------------------------------

/// One `AgHg` chunk: magic, u16 header length, version, kind, u64 payload
/// length, u64 padding length, name, payload, padding.
///
/// Padding bytes are deliberately not zero: they must be skipped, not read.
pub fn chunk(name: &str, payload: &[u8], padding: usize) -> Vec<u8> {
    let header_len = 24 + name.len();
    let header_len = u16::try_from(header_len).expect("a chunk header fits a u16");
    let mut bytes = Vec::with_capacity(usize::from(header_len) + payload.len() + padding);
    bytes.extend_from_slice(b"AgHg");
    bytes.extend_from_slice(&header_len.to_be_bytes());
    bytes.push(0); // version
    bytes.push(0); // kind
    bytes.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    bytes.extend_from_slice(&(padding as u64).to_be_bytes());
    bytes.extend_from_slice(name.as_bytes());
    bytes.extend_from_slice(payload);
    bytes.resize(bytes.len() + padding, 0xEE);
    bytes
}

/// The Lua-like `header` chunk payload: a list of level dimensions.
pub fn header(levels: &[(u16, u16)]) -> Vec<u8> {
    let mut text = String::from("pyramid = {\n\tlevels = {\n");
    for (width, height) in levels {
        text.push_str(&format!("\t\t{{ height = {height}, width = {width}, }},\n"));
    }
    text.push_str("\t},\n}\n");
    text.into_bytes()
}

/// The header chunk plus one `level_N` chunk per level.
pub fn container(levels: &[(u16, u16)]) -> Vec<u8> {
    let mut bytes = chunk("header", &header(levels), 0);
    for (index, (width, height)) in levels.iter().enumerate() {
        bytes.extend(chunk(
            &format!("level_{}", index + 1),
            &jpeg(*width, *height),
            0,
        ));
    }
    bytes
}

// ---------------------------------------------------------------------------
// Leaf addresses (the modern layout, docs/format-notes.md §5.2)
// ---------------------------------------------------------------------------

/// `<UUID>-<digest>_<long_edge>`: the file name a level lives under.
///
/// `long_edge` is the rounded `PyramidLevel.longDimension`, never the suffix of
/// the declared width or height (§5.7).
pub fn leaf(uuid: &str, digest: &str, long_edge: u32) -> String {
    format!("{uuid}-{digest}_{long_edge}")
}

/// `(uuid[0], uuid[0..4])` — the two shard directories, **not** the whole uuid.
pub fn leaf_dir(uuid: &str) -> (String, String) {
    let first = uuid.chars().take(1).collect();
    let first_four = uuid.chars().take(4).collect();
    (first, first_four)
}

// ---------------------------------------------------------------------------
// The catalog's XMP blob
// ---------------------------------------------------------------------------

/// The catalog's zlib blob behind the 4-byte prefix Lightroom uses.
///
/// Written as a zlib stream of *stored* deflate blocks, so the builder needs no
/// compressor: `specs/fixtures.md` gives it `rusqlite` and `tempfile` only, and
/// a stored block is legal zlib that any inflater (including `catalog`'s)
/// reads back byte for byte.
pub fn xmp_blob(xml: &str) -> Vec<u8> {
    let data = xml.as_bytes();
    let mut blob = vec![0u8; 4];

    // 0x78 0x01: deflate, 32 KiB window, no preset dictionary; the two bytes
    // together are a multiple of 31, which zlib requires of its header.
    blob.extend_from_slice(&[0x78, 0x01]);
    if data.is_empty() {
        blob.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    } else {
        for (index, block) in data.chunks(0xFFFF).enumerate() {
            let last = (index + 1) * 0xFFFF >= data.len();
            blob.push(u8::from(last)); // BFINAL, BTYPE = 00 (stored)
            let length = u16::try_from(block.len()).expect("a stored block fits a u16");
            blob.extend_from_slice(&length.to_le_bytes());
            blob.extend_from_slice(&(!length).to_le_bytes());
            blob.extend_from_slice(block);
        }
    }
    blob.extend_from_slice(&adler32(data).to_be_bytes());
    blob
}

/// The zlib trailer checksum.
fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use lrsalvage_core::{jpeg, lrprev};

    use super::*;

    #[test]
    fn a_jpeg_has_a_frame_and_an_eoi() {
        let bytes = jpeg(1616, 1077);
        assert!(bytes.starts_with(&[0xFF, 0xD8]));
        assert!(bytes.ends_with(&[0xFF, 0xD9]));
        assert_eq!(
            jpeg::dimensions(&bytes).expect("a readable frame"),
            (1616, 1077)
        );
        assert!(jpeg::is_complete(&bytes));
    }

    #[test]
    fn the_damaged_shapes_are_the_damage_they_name() {
        let truncated = truncated_jpeg(1616, 1077);
        assert!(!jpeg::is_complete(&truncated));
        assert_eq!(
            jpeg::dimensions(&truncated).expect("a readable frame"),
            (1616, 1077)
        );

        let headless = no_soi(1616, 1077);
        assert!(!headless.starts_with(&[0xFF, 0xD8]));
        assert!(jpeg::dimensions(&headless).is_err());

        assert!(jpeg::dimensions(&not_a_jpeg()).is_err());
        assert!(!jpeg::is_complete(&not_a_jpeg()));
    }

    #[test]
    fn a_container_parses_back_into_the_levels_it_declares() {
        let bytes = container(&[(101, 68), (1616, 1077)]);
        let parsed = lrprev::Lrprev::parse(&bytes).expect("a parseable container");
        let levels = parsed.levels();
        assert_eq!(levels.len(), 2);
        assert_eq!(
            jpeg::dimensions(levels[0].data).expect("readable"),
            (101, 68)
        );
        assert_eq!(
            jpeg::dimensions(levels[1].data).expect("readable"),
            (1616, 1077)
        );
        assert!(parsed.header().is_some());
    }

    #[test]
    fn padding_is_declared_not_written_into_the_payload() {
        let mut bytes = chunk("level_1", &jpeg(800, 600), 9);
        bytes.extend(chunk("level_2", &jpeg(1600, 1200), 0));
        let parsed = lrprev::Lrprev::parse(&bytes).expect("a parseable container");
        assert_eq!(parsed.levels().len(), 2);
        assert_eq!(
            jpeg::dimensions(parsed.levels()[1].data).expect("readable"),
            (1600, 1200)
        );
    }

    #[test]
    fn the_leaf_and_its_shard_folders_follow_the_measured_grammar() {
        let uuid = "1418F22B-41B0-4786-9DF0-D5081559CAD1";
        let digest = "bd012a8e0e0b5b73ddaf34aeceb8bf48";
        assert_eq!(
            leaf(uuid, digest, 1616),
            "1418F22B-41B0-4786-9DF0-D5081559CAD1-bd012a8e0e0b5b73ddaf34aeceb8bf48_1616"
        );
        let (first, first_four) = leaf_dir(uuid);
        assert_eq!(first, "1");
        assert_eq!(first_four, "1418");
    }

    #[test]
    fn our_output_carries_the_software_tag_and_the_frames_size() {
        let bytes = our_output(1616, 1077, "0.1.0");
        assert!(jpeg::is_complete(&bytes));
        assert_eq!(
            jpeg::dimensions(&bytes).expect("a readable frame"),
            (1616, 1077)
        );

        let (tag, value) = ifd0_entry(&bytes, 0x0131).expect("IFD0 carries 0x0131");
        assert_eq!(tag, 0x0131);
        assert_eq!(value, b"lrsalvage 0.1.0\0");
    }

    #[test]
    fn a_foreign_output_carries_a_marker_that_is_not_ours() {
        let bytes = foreign_output(1616, 1077, "Adobe Photoshop 25.0");
        assert!(jpeg::is_complete(&bytes));
        assert_eq!(
            jpeg::dimensions(&bytes).expect("a readable frame"),
            (1616, 1077)
        );

        // A `Software` tag, so presence alone cannot tell the two apart: only
        // the value's `lrsalvage ` prefix can.
        let (tag, value) = ifd0_entry(&bytes, 0x0131).expect("IFD0 carries 0x0131");
        assert_eq!(tag, 0x0131);
        assert_eq!(value, b"Adobe Photoshop 25.0\0");
    }

    #[test]
    fn an_exif_app1_carries_the_orientation_it_was_given() {
        let segment = exif_app1(6);
        assert!(
            app1_payload(&segment).is_some_and(|payload| payload.starts_with(b"Exif\0\0")),
            "an Exif APP1 opens with the Exif signature"
        );

        // Read the tag back the way a reader walks it: inside a frame.
        let bytes = jpeg_with(64, 48, &[segment]);
        let (tag, value) = ifd0_entry(&bytes, 0x0112).expect("IFD0 carries 0x0112");
        assert_eq!(tag, 0x0112);
        assert_eq!(u16::from_be_bytes([value[0], value[1]]), 6);
    }

    #[test]
    fn an_xmp_packet_is_its_own_app1_with_the_standard_signature() {
        let packet = xmp_app1("<x:xmpmeta/>");
        assert_eq!(packet[0], 0xFF);
        assert_eq!(packet[1], 0xE1);
        let payload = &packet[4..];
        assert!(payload.starts_with(b"http://ns.adobe.com/xap/1.0/\0"));
        assert!(payload.ends_with(b"<x:xmpmeta/>"));
    }

    #[test]
    fn a_large_icc_profile_is_chunked_into_numbered_app2_segments() {
        let profile = vec![0x42u8; 70_000];
        let bytes = icc_app2(&profile);
        // 70_000 bytes need two segments; the second one's header says so.
        let second = bytes
            .windows(2)
            .enumerate()
            .filter(|(_, window)| *window == [0xFF, 0xE2])
            .map(|(at, _)| at)
            .nth(1)
            .expect("two APP2 segments");
        assert_eq!(&bytes[second + 4..second + 16], b"ICC_PROFILE\0");
        assert_eq!(bytes[second + 16], 2, "segment number");
        assert_eq!(bytes[second + 17], 2, "segment count");
    }

    #[test]
    fn the_xmp_blob_is_a_zlib_stream_of_the_documented_shape() {
        let xml = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>";
        let blob = xmp_blob(xml);
        assert_eq!(&blob[..4], &[0, 0, 0, 0], "the 4-byte prefix");
        assert_eq!(&blob[4..6], &[0x78, 0x01], "a zlib header");
        assert_eq!(
            &blob[blob.len() - 4..],
            &adler32(xml.as_bytes()).to_be_bytes(),
            "the zlib trailer"
        );
        // Stored, then the bytes themselves: nothing is re-encoded.
        assert!(
            blob.windows(xml.len())
                .any(|window| window == xml.as_bytes()),
            "the text survives uncompressed in a stored block"
        );
    }

    /// The payload of a single APP segment, by its declared length.
    fn app1_payload(segment: &[u8]) -> Option<&[u8]> {
        Some(&segment[4..2 + u16::from_be_bytes([segment[2], segment[3]]) as usize])
    }

    /// Walk a complete JPEG's header and read IFD0 entry `tag` out of its Exif
    /// APP1. Enough TIFF for the two tags this builder writes.
    fn ifd0_entry(bytes: &[u8], tag: u16) -> Option<(u16, Vec<u8>)> {
        let mut pos = 2;
        while pos + 4 <= bytes.len() {
            let marker = bytes[pos + 1];
            if marker == 0xDA {
                return None;
            }
            let length = u16::from_be_bytes([bytes[pos + 2], bytes[pos + 3]]) as usize;
            if marker == 0xE1 && bytes[pos + 4..].starts_with(b"Exif\0\0") {
                let tiff = &bytes[pos + 10..pos + 2 + length];
                assert_eq!(&tiff[..2], b"MM");
                let ifd = u32::from_be_bytes([tiff[4], tiff[5], tiff[6], tiff[7]]) as usize;
                let count = u16::from_be_bytes([tiff[ifd], tiff[ifd + 1]]) as usize;
                for index in 0..count {
                    let at = ifd + 2 + index * 12;
                    let found = u16::from_be_bytes([tiff[at], tiff[at + 1]]);
                    let kind = u16::from_be_bytes([tiff[at + 2], tiff[at + 3]]);
                    let entries = u32::from_be_bytes([
                        tiff[at + 4],
                        tiff[at + 5],
                        tiff[at + 6],
                        tiff[at + 7],
                    ]) as usize;
                    if found != tag {
                        continue;
                    }
                    let field = &tiff[at + 8..at + 12];
                    let width = if kind == TYPE_SHORT { 2 } else { 1 };
                    let len = entries * width;
                    return if len <= 4 {
                        Some((found, field[..len].to_vec()))
                    } else {
                        let offset =
                            u32::from_be_bytes([field[0], field[1], field[2], field[3]]) as usize;
                        Some((found, tiff[offset..offset + len].to_vec()))
                    };
                }
                return None;
            }
            pos += 2 + length;
        }
        None
    }
}
