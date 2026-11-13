//! Minimal JPEG inspection: dimensions and completeness.

use crate::error::Result;

/// Read `(width, height)` from the first start-of-frame marker.
pub fn dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    // SOI is the only legal opening; anything else cannot hold a frame.
    if bytes.len() < 2 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return Err(crate::error::Error::JpegMalformed);
    }

    let mut pos = 2;
    while pos < bytes.len() {
        // Segment markers start with one or more 0xFF fill bytes.
        if bytes[pos] != 0xFF {
            return Err(crate::error::Error::JpegMalformed);
        }
        while pos < bytes.len() && bytes[pos] == 0xFF {
            pos += 1;
        }
        let Some(&marker) = bytes.get(pos) else {
            // Input ends on a fill byte run: truncated.
            return Err(crate::error::Error::JpegMalformed);
        };
        pos += 1;

        match marker {
            // Standalone markers carry no length field.
            0x01 | 0xD0..=0xD7 | 0xD8 | 0xD9 => continue,
            // Start of scan: entropy-coded data follows, no frame marker ahead.
            0xDA => return Err(crate::error::Error::JpegMalformed),
            // A stuffed 0x00 is only legal inside entropy-coded data.
            0x00 => return Err(crate::error::Error::JpegMalformed),
            _ => {
                let len_bytes = bytes
                    .get(pos..pos + 2)
                    .ok_or(crate::error::Error::JpegMalformed)?;
                let len = u16::from_be_bytes([len_bytes[0], len_bytes[1]]) as usize;
                // The length includes its own two bytes.
                if len < 2 {
                    return Err(crate::error::Error::JpegMalformed);
                }
                let end = pos
                    .checked_add(len)
                    .filter(|&end| end <= bytes.len())
                    .ok_or(crate::error::Error::JpegMalformed)?;

                if is_frame_marker(marker) {
                    // payload: [precision:1][height:2][width:2]...
                    if len < 7 {
                        return Err(crate::error::Error::JpegMalformed);
                    }
                    let height = u16::from_be_bytes([bytes[pos + 3], bytes[pos + 4]]);
                    let width = u16::from_be_bytes([bytes[pos + 5], bytes[pos + 6]]);
                    return Ok((width as u32, height as u32));
                }
                pos = end;
            }
        }
    }

    Err(crate::error::Error::JpegMalformed)
}

/// Start-of-frame markers whose payload carries `height` then `width`.
fn is_frame_marker(marker: u8) -> bool {
    matches!(
        marker,
        0xC0 | 0xC1 | 0xC2 | 0xC3 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCA | 0xCB | 0xCD | 0xCE | 0xCF
    )
}

/// `true` when the buffer starts with SOI and ends with EOI.
pub fn is_complete(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes.starts_with(&[0xFF, 0xD8]) && bytes.ends_with(&[0xFF, 0xD9])
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
}
