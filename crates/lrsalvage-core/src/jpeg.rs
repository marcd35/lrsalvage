//! Minimal JPEG inspection: framing, dimensions and completeness.

use std::io::Read;
use std::ops::Range;
use std::path::Path;

use crate::error::Result;

/// One marker in a JPEG header, before scan data begins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// The marker byte that follows the `FF` (`0xD8` for `SOI`, `0xE1` for
    /// `APP1`).
    pub marker: u8,
    /// Byte range of the whole segment: its `FF`, the marker byte, the length
    /// field and the payload. A redundant `FF` fill run in front of the marker
    /// is not part of it, so the length field always starts at
    /// `range.start + 2`.
    pub range: Range<usize>,
}

impl Segment {
    /// The segment's payload: everything after its two-byte length field.
    ///
    /// Empty for a standalone marker, which declares no length.
    pub fn payload<'a>(&self, jpeg: &'a [u8]) -> &'a [u8] {
        jpeg.get(self.range.start + 4..self.range.end)
            .unwrap_or(&[])
    }
}

/// The verdict of one certification walk over a payload's own bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Certified {
    /// `(width, height)` from the first top-level start-of-frame marker.
    pub dimensions: (u32, u32),
    /// `true` when `FF D9` ends the buffer, which is what makes a truncated
    /// file distinguishable from a whole one.
    pub complete: bool,
    /// Every segment before `SOS`, `SOI` first. Framing only: naming what an
    /// `APP1` or `APP2` carries is `exif`'s work.
    pub segments: Vec<Segment>,
}

/// Certify a payload in one walk: `SOI` at 0, every segment's length declared,
/// the first top-level `SOF` for the dimensions, `FF D9` at EOF.
///
/// APPn and COM payloads are skipped wholesale by their own length, so an
/// embedded EXIF thumbnail is never on the chain and can never be mistaken for
/// the frame.
pub fn certify(bytes: &[u8]) -> Result<Certified> {
    // SOI is the only legal opening; anything else cannot hold a frame.
    if bytes.len() < 2 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return Err(crate::error::Error::JpegMalformed);
    }

    let mut segments = vec![Segment {
        marker: 0xD8,
        range: 0..2,
    }];
    let mut dimensions = None;
    let mut pos = 2;

    // A header that ends without scan data is incomplete, not malformed: every
    // prefix of a real payload is answered, not rejected.
    while let Some(&lead) = bytes.get(pos) {
        // Segment markers start with one or more 0xFF fill bytes.
        if lead != 0xFF {
            return Err(crate::error::Error::JpegMalformed);
        }
        let mut marker_at = pos;
        while bytes.get(marker_at + 1) == Some(&0xFF) {
            marker_at += 1;
        }
        let Some(&marker) = bytes.get(marker_at + 1) else {
            break;
        };
        // The length field, for every marker that declares one.
        let length_at = marker_at + 2;
        let mut next = length_at;

        match marker {
            // A stuffed 0x00 is only legal inside entropy-coded data.
            0x00 => return Err(crate::error::Error::JpegMalformed),
            // Start of scan: entropy-coded data follows, and the header is done.
            0xDA => break,
            // End of image before any scan: the header is over.
            0xD9 => break,
            // Standalone markers carry no length field.
            0x01 | 0xD0..=0xD7 | 0xD8 => segments.push(Segment {
                marker,
                range: marker_at..length_at,
            }),
            _ => {
                let len_bytes = bytes
                    .get(length_at..length_at + 2)
                    .ok_or(crate::error::Error::JpegMalformed)?;
                let len = u16::from_be_bytes([len_bytes[0], len_bytes[1]]) as usize;
                // The length includes its own two bytes.
                if len < 2 {
                    return Err(crate::error::Error::JpegMalformed);
                }
                let end = length_at
                    .checked_add(len)
                    .filter(|&end| end <= bytes.len())
                    .ok_or(crate::error::Error::JpegMalformed)?;

                if is_frame_marker(marker) && dimensions.is_none() {
                    // payload: [precision:1][height:2][width:2]...
                    if len < 7 {
                        return Err(crate::error::Error::JpegMalformed);
                    }
                    let height = u16::from_be_bytes([bytes[length_at + 3], bytes[length_at + 4]]);
                    let width = u16::from_be_bytes([bytes[length_at + 5], bytes[length_at + 6]]);
                    dimensions = Some((u32::from(width), u32::from(height)));
                }
                segments.push(Segment {
                    marker,
                    range: marker_at..end,
                });
                next = end;
            }
        }
        pos = next;
    }

    // No top-level frame before the scan data: nothing here declares a size.
    let Some(dimensions) = dimensions else {
        return Err(crate::error::Error::JpegMalformed);
    };

    Ok(Certified {
        dimensions,
        complete: bytes.ends_with(&[0xFF, 0xD9]),
        segments,
    })
}

/// Read `(width, height)` from the first start-of-frame marker.
pub fn dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    certify(bytes).map(|certified| certified.dimensions)
}

/// `true` when the file opens with `FF D8` — ADR 0004's narrow test for a
/// payload-like file.
///
/// At most two bytes are read and nothing is scanned: a file that merely
/// *contains* a `FF D8` somewhere is not payload-like. That narrowness is the
/// decision, not an oversight — widening the rule to a scan would let any file
/// with an embedded thumbnail claim a shape it does not have
/// (`adr-0004-detect-layout-not-version`).
///
/// An unreadable file is not payload-like: this is a classification of what was
/// seen, never a claim about what could not be.
pub fn is_payload_start(path: &Path) -> bool {
    let mut start = [0u8; 2];
    match std::fs::File::open(path) {
        Ok(mut file) => file.read_exact(&mut start).is_ok() && start == [0xFF, 0xD8],
        Err(_) => false,
    }
}

/// Start-of-frame markers whose payload carries `height` then `width`.
fn is_frame_marker(marker: u8) -> bool {
    matches!(
        marker,
        0xC0 | 0xC1 | 0xC2 | 0xC3 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCA | 0xCB | 0xCD | 0xCE | 0xCF
    )
}

/// `true` when the buffer is a complete JPEG within its own bounds: a legal
/// header walk and `FF D9` at EOF.
pub fn is_complete(bytes: &[u8]) -> bool {
    certify(bytes).is_ok_and(|certified| certified.complete)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    const SOF_MARKERS: [u8; 13] = [
        0xC0, 0xC1, 0xC2, 0xC3, 0xC5, 0xC6, 0xC7, 0xC9, 0xCA, 0xCB, 0xCD, 0xCE, 0xCF,
    ];

    /// Build a small but structurally valid JPEG at `width` x `height`.
    fn jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        // APP0/JFIF: length includes its own two bytes.
        v.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10]);
        v.extend_from_slice(b"JFIF\0");
        v.extend_from_slice(&[0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
        v.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        v.extend_from_slice(&height.to_be_bytes());
        v.extend_from_slice(&width.to_be_bytes());
        v.extend_from_slice(&[0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        // SOS, one entropy byte, EOI.
        v.extend_from_slice(&[
            0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00,
        ]);
        v.extend_from_slice(&[0x00, 0xFF, 0xD9]);
        v
    }

    fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
        let len = u16::try_from(payload.len() + 2).expect("segment fits");
        let mut v = vec![0xFF, marker];
        v.extend_from_slice(&len.to_be_bytes());
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn reads_dimensions_from_baseline_sof0() {
        // Width and height deliberately differ so a swapped read cannot pass.
        assert_eq!(dimensions(&jpeg(1616, 1077)).unwrap(), (1616, 1077));
        assert_eq!(dimensions(&jpeg(101, 68)).unwrap(), (101, 68));
    }

    #[test]
    fn does_not_mistake_marker_lookalikes_in_segments_for_the_frame() {
        // APP1 payload contains a fake SOF0; the real one comes later.
        let mut fake = vec![0xFF, 0xC0, 0x00, 0x11, 0x08];
        fake.extend_from_slice(&11u16.to_be_bytes());
        fake.extend_from_slice(&22u16.to_be_bytes());
        let mut bytes = vec![0xFF, 0xD8];
        bytes.extend_from_slice(&segment(0xE1, &fake));
        bytes.extend_from_slice(&jpeg(640, 480)[2..]);
        assert_eq!(dimensions(&bytes).unwrap(), (640, 480));
    }

    #[test]
    fn accepts_every_frame_marker_that_carries_dimensions() {
        for marker in SOF_MARKERS {
            let mut bytes = vec![0xFF, 0xD8];
            let mut sof = vec![0x08u8];
            sof.extend_from_slice(&321u16.to_be_bytes());
            sof.extend_from_slice(&123u16.to_be_bytes());
            sof.extend_from_slice(&[0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
            bytes.extend_from_slice(&segment(marker, &sof));
            bytes.extend_from_slice(&[0xFF, 0xD9]);
            assert_eq!(
                dimensions(&bytes).unwrap(),
                (123, 321),
                "SOF marker 0x{marker:02X} should provide dimensions"
            );
        }
    }

    #[test]
    fn ignores_non_frame_markers_that_look_like_frames() {
        // 0xC4 is DHT and 0xCC is DAC: neither carries frame dimensions.
        for marker in [0xC4u8, 0xCC] {
            let mut bytes = vec![0xFF, 0xD8];
            let mut body = vec![0x08u8];
            body.extend_from_slice(&11u16.to_be_bytes());
            body.extend_from_slice(&22u16.to_be_bytes());
            body.extend_from_slice(&[0u8; 9]);
            bytes.extend_from_slice(&segment(marker, &body));
            bytes.extend_from_slice(&jpeg(800, 600)[2..]);
            assert_eq!(
                dimensions(&bytes).unwrap(),
                (800, 600),
                "marker 0x{marker:02X} is not a frame marker"
            );
        }
    }

    #[test]
    fn stops_at_start_of_scan_and_never_reads_entropy_coded_data() {
        let mut bytes = vec![0xFF, 0xD8];
        bytes.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x0C, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
        // Entropy-coded data containing a perfect-looking SOF0 must be ignored.
        bytes.extend_from_slice(&[0x12, 0x34]);
        bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        bytes.extend_from_slice(&999u16.to_be_bytes());
        bytes.extend_from_slice(&888u16.to_be_bytes());
        bytes.extend_from_slice(&[0u8; 9]);
        bytes.extend_from_slice(&[0xFF, 0xD9]);
        assert!(matches!(dimensions(&bytes), Err(Error::JpegMalformed)));
    }

    #[test]
    fn errors_when_no_frame_marker_is_present() {
        assert!(matches!(
            dimensions(&[0xFF, 0xD8, 0xFF, 0xD9]),
            Err(Error::JpegMalformed)
        ));
        assert!(dimensions(&[]).is_err());
        assert!(dimensions(b"not a jpeg at all").is_err());
    }

    #[test]
    fn truncated_segments_are_errors_not_panics() {
        let valid = jpeg(1616, 1077);
        // Cut inside the APP0 payload and inside the SOF payload.
        assert!(dimensions(&valid[..6]).is_err());
        let sof_start = valid
            .windows(2)
            .position(|w| w == [0xFF, 0xC0])
            .expect("test jpeg has SOF0");
        assert!(dimensions(&valid[..sof_start + 6]).is_err());
    }

    #[test]
    fn every_prefix_of_a_valid_jpeg_is_answered_without_panicking() {
        let valid = jpeg(1616, 1077);
        for end in 0..=valid.len() {
            let prefix = &valid[..end];
            // Must return, either Ok or Err; a panic fails the test.
            let _ = dimensions(prefix);
        }
        // And the complete buffer still parses.
        assert_eq!(dimensions(&valid).unwrap(), (1616, 1077));
    }

    #[test]
    fn certification_names_the_segments_before_the_scan() {
        let bytes = jpeg(1616, 1077);
        let certified = certify(&bytes).expect("a valid frame certifies");

        assert_eq!(certified.dimensions, (1616, 1077));
        assert!(certified.complete);
        assert_eq!(
            certified
                .segments
                .iter()
                .map(|segment| segment.marker)
                .collect::<Vec<_>>(),
            vec![0xD8, 0xE0, 0xC0],
            "SOI, APP0 and SOF0, and nothing from the scan onward"
        );
        assert_eq!(
            certified.segments[0].range,
            0..2,
            "SOI is the first two bytes"
        );
        assert!(
            certified.segments[1].payload(&bytes).starts_with(b"JFIF\0"),
            "an APP0 payload is named by its declared range"
        );
        assert!(
            certified.segments[0].payload(&bytes).is_empty(),
            "a standalone marker declares no payload"
        );
        // The SOF's range covers its declared length, dimensions included.
        let sof = &certified.segments[2];
        assert_eq!(sof.payload(&bytes).len(), 15);
        assert_eq!(sof.payload(&bytes)[1..3], 1077u16.to_be_bytes());
        assert_eq!(sof.payload(&bytes)[3..5], 1616u16.to_be_bytes());
    }

    #[test]
    fn certification_reports_a_missing_eoi_without_losing_the_frame() {
        let complete = jpeg(1616, 1077);
        let truncated = &complete[..complete.len() - 2];

        let certified = certify(truncated).expect("the header still certifies");
        assert!(!certified.complete, "FF D9 is what completes a payload");
        assert_eq!(certified.dimensions, (1616, 1077));
        assert!(!is_complete(truncated));
        assert!(is_complete(&complete));
    }

    #[test]
    fn an_embedded_exif_thumbnail_is_never_on_the_segment_chain() {
        // An APP1 whose payload is itself a whole JPEG with a lying SOF.
        let fake = jpeg(11, 22);
        let mut bytes = vec![0xFF, 0xD8];
        bytes.extend_from_slice(&segment(0xE1, &fake));
        bytes.extend_from_slice(&jpeg(640, 480)[2..]);

        let certified = certify(&bytes).expect("the real frame is found");
        assert_eq!(certified.dimensions, (640, 480));
        assert_eq!(
            certified
                .segments
                .iter()
                .map(|segment| segment.marker)
                .collect::<Vec<_>>(),
            vec![0xD8, 0xE1, 0xE0, 0xC0],
            "the APP1 is one segment, whatever it contains"
        );
    }

    #[test]
    fn fill_bytes_before_a_marker_are_tolerated_and_not_part_of_the_segment() {
        let mut bytes = jpeg(1616, 1077);
        let sof_start = bytes
            .windows(2)
            .position(|window| window == [0xFF, 0xC0])
            .expect("test jpeg has SOF0");
        bytes.insert(sof_start, 0xFF);

        let certified = certify(&bytes).expect("a fill run is legal framing");
        assert_eq!(certified.dimensions, (1616, 1077));
        let sof = certified
            .segments
            .iter()
            .find(|segment| segment.marker == 0xC0)
            .expect("the SOF is still one segment");
        assert_eq!(
            sof.range.start,
            sof_start + 1,
            "the range starts at the marker's own FF"
        );
        assert_eq!(sof.payload(&bytes).len(), 15);
    }

    #[test]
    fn the_payload_start_test_reads_two_bytes_and_never_scans() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, bytes: &[u8]| {
            let path = dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        };

        assert!(is_payload_start(&write("soi", &jpeg(1616, 1077))));
        // Payload-like without being a whole JPEG: the test is the opening.
        assert!(is_payload_start(&write("only-soi", &[0xFF, 0xD8])));
        // A container, a truncated header and an unrelated file are not.
        assert!(!is_payload_start(&write("text", b"not a jpeg at all")));
        assert!(!is_payload_start(&write("short", &[0xFF])));
        assert!(!is_payload_start(&write("empty", &[])));
        // A `FF D8` behind the opening is not the opening: nothing is scanned.
        let mut buried = vec![0u8; 64];
        buried.extend_from_slice(&jpeg(1616, 1077));
        assert!(!is_payload_start(&write("buried", &buried)));
        // A file that is not there is not payload-like either.
        assert!(!is_payload_start(&dir.path().join("absent")));
    }

    #[test]
    fn a_header_without_image_data_is_incomplete_not_malformed() {
        // A frame and EOI with no scan: nothing to write, but the claim that a
        // payload is truncated is `complete`, not a parse error.
        let certified = certify(&[0xFF, 0xD8, 0xFF, 0xD9]).unwrap_err();
        assert!(matches!(certified, Error::JpegMalformed));
        let mut header_only = vec![0xFF, 0xD8];
        header_only.extend_from_slice(&segment(
            0xC0,
            &[
                0x08, 0x04, 0x35, 0x06, 0x50, 0x03, 1, 0x11, 0, 2, 0x11, 1, 3, 0x11, 1,
            ],
        ));
        header_only.extend_from_slice(&[0xFF, 0xD9]);
        let certified = certify(&header_only).expect("the frame is readable");
        assert_eq!(certified.dimensions, (1616, 1077));
        assert!(certified.complete, "it does end with FF D9");

        assert!(certify(b"not a jpeg at all").is_err());
        assert!(!is_complete(&[0xFF, 0xD8, 0xFF, 0xD9]));
    }
}
