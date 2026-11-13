//! The `.lrprev` container: a sequence of `AgHg` chunks.
//!
//! Layout (big-endian, verified against real files):
//!
//! ```text
//! "AgHg"            4 bytes  magic
//! u16               header length, INCLUDING the 4-byte magic
//! u8                version
//! u8                kind
//! u64               payload length
//! u64               padding length (bytes to skip AFTER the payload)
//! [header_len-24]   name: "header", "level_1" .. "level_N" (NUL padded)
//! [payload_len]     data
//! [padding_len]     pad
//! ```
//!
//! `header` holds a small Lua-like description of the pyramid; each `level_N`
//! holds one complete JPEG. The declared lengths are authoritative — never
//! scan for `FFD8`/`FFD9`, because every level JPEG carries its own EXIF
//! thumbnail and a naive scan can return that instead (ADR 0001).

use crate::error::{Error, Result};

pub const MAGIC: &[u8; 4] = b"AgHg";

/// magic (4) + header length (2) + version (1) + kind (1) + payload length (8)
/// + padding length (8). `header_len` counts all of it.
const FIXED_HEADER_LEN: usize = 24;

fn truncated(offset: usize, detail: impl Into<String>) -> Error {
    Error::LrprevTruncated {
        offset,
        detail: detail.into(),
    }
}

fn be_u64(bytes: &[u8], at: usize) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&bytes[at..at + 8]);
    u64::from_be_bytes(buf)
}

/// Chunk names are ASCII, NUL-padded to fill the header. Keep everything up to
/// the first NUL.
fn chunk_name(raw: &[u8]) -> String {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

/// One parsed chunk, borrowing from the input buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk<'a> {
    pub name: String,
    pub data: &'a [u8],
}

/// A parsed `.lrprev` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Lrprev<'a> {
    pub chunks: Vec<Chunk<'a>>,
}

impl<'a> Lrprev<'a> {
    /// Walk every chunk. Fails on a bad magic, a truncated chunk, or a
    /// header length smaller than the fixed part.
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.is_empty() {
            return Err(Error::LrprevHeader(
                "empty input: a .lrprev file holds at least one chunk".into(),
            ));
        }

        let mut chunks = Vec::new();
        let mut pos = 0usize;
        while pos < bytes.len() {
            let start = pos;
            let remaining = bytes.len() - start;
            if remaining < FIXED_HEADER_LEN {
                return Err(truncated(
                    start,
                    format!("chunk header needs {FIXED_HEADER_LEN} bytes, {remaining} left"),
                ));
            }
            let fixed = &bytes[start..start + FIXED_HEADER_LEN];

            if &fixed[..4] != MAGIC {
                return Err(Error::LrprevHeader(format!(
                    "bad magic {:02x?} at byte {start}",
                    &fixed[..4]
                )));
            }

            let header_len = u16::from_be_bytes([fixed[4], fixed[5]]) as usize;
            if header_len < FIXED_HEADER_LEN {
                return Err(Error::LrprevHeader(format!(
                    "header length {header_len} at byte {start} is smaller than the \
                     {FIXED_HEADER_LEN}-byte fixed part"
                )));
            }
            if header_len > remaining {
                return Err(truncated(
                    start,
                    format!("chunk header declares {header_len} bytes but only {remaining} remain"),
                ));
            }

            let payload_len = be_u64(fixed, 8);
            let padding_len = be_u64(fixed, 16);
            let name = chunk_name(&bytes[start + FIXED_HEADER_LEN..start + header_len]);

            let data_start = start + header_len;
            let payload_len = usize::try_from(payload_len).map_err(|_| {
                truncated(
                    start,
                    format!("chunk `{name}` payload length does not fit memory"),
                )
            })?;
            let data_end = data_start
                .checked_add(payload_len)
                .filter(|&end| end <= bytes.len())
                .ok_or_else(|| {
                    truncated(
                        start,
                        format!(
                            "chunk `{name}` declares {payload_len} payload bytes but only {} remain",
                            bytes.len() - data_start
                        ),
                    )
                })?;

            let padding_len = usize::try_from(padding_len).map_err(|_| {
                truncated(
                    start,
                    format!("chunk `{name}` padding length does not fit memory"),
                )
            })?;
            let next = match data_end.checked_add(padding_len) {
                Some(end) if end <= bytes.len() => end,
                // Lightroom does not write the final chunk's padding.
                // Measured on 154 of 3,057 real payloads: every overrun equals
                // exactly the last chunk's declared padding, and the payload
                // itself is complete. That is only tolerable when the payload
                // ends exactly at EOF; anywhere else the bytes really are
                // missing, and a cut file is a failure (ADR 0001), not
                // something to read past.
                _ if data_end == bytes.len() => data_end,
                _ => {
                    return Err(truncated(
                        start,
                        format!(
                            "chunk `{name}` declares {padding_len} padding bytes but only {} \
                             remain after its payload",
                            bytes.len() - data_end
                        ),
                    ));
                }
            };

            chunks.push(Chunk {
                name,
                data: &bytes[data_start..data_end],
            });
            pos = next;
        }

        Ok(Lrprev { chunks })
    }

    /// The `header` chunk payload (Lua-like text).
    pub fn header(&self) -> Option<&'a [u8]> {
        self.chunks
            .iter()
            .find(|c| c.name == "header")
            .map(|c| c.data)
    }

    /// All `level_N` chunks, in file order.
    pub fn levels(&self) -> Vec<&Chunk<'a>> {
        self.chunks
            .iter()
            .filter(|c| c.name.starts_with("level_"))
            .collect()
    }

    /// The largest `level_N` chunk, chosen by decoded JPEG dimensions (not by
    /// payload byte length).
    pub fn best_level(&self) -> Option<&Chunk<'a>> {
        self.levels()
            .into_iter()
            .filter_map(|c| crate::jpeg::dimensions(c.data).ok().map(|d| (d, c)))
            .max_by_key(|((w, h), _)| (*w as u64) * (*h as u64))
            .map(|(_, c)| c)
    }
}

/// Parse the level index out of a chunk name (`"level_3"` → `3`).
pub fn level_index(name: &str) -> Option<u32> {
    name.strip_prefix("level_")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    const HEADER_TEXT: &str = "pyramid = {\n\
\tcroppedHeight = 1077,\n\
\tcroppedWidth = 1616,\n\
\tlevels = {\n\
\t\t{ height = 68, width = 101, },\n\
\t\t{ height = 1077, width = 1616, },\n\
\t},\n\
}\n";

    /// Build one chunk. `name` is the raw header name field, so callers can
    /// pass NUL padding themselves. Padding bytes are deliberately non-zero.
    fn chunk(name: &str, payload: &[u8], padding: usize) -> Vec<u8> {
        let header_len = 24 + name.len();
        let mut v = Vec::new();
        v.extend_from_slice(MAGIC);
        v.extend_from_slice(
            &u16::try_from(header_len)
                .expect("header fits")
                .to_be_bytes(),
        );
        v.push(0); // version
        v.push(0); // kind
        v.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        v.extend_from_slice(&(padding as u64).to_be_bytes());
        v.extend_from_slice(name.as_bytes());
        v.extend_from_slice(payload);
        for i in 0..padding {
            v.push(0xA0u8.wrapping_add((i % 32) as u8));
        }
        v
    }

    /// A structurally valid JPEG whose pre-scan segments total `filler` bytes.
    fn jpeg(width: u16, height: u16, filler: usize) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        if filler > 0 {
            // Split large fillers across several APP segments (u16 lengths).
            let mut left = filler;
            while left > 0 {
                let take = left.min(60_000);
                let mut app = vec![0xFF, 0xE2];
                let len = u16::try_from(take + 2).expect("segment fits");
                app.extend_from_slice(&len.to_be_bytes());
                app.resize(take + 4, 0x5A);
                v.extend_from_slice(&app);
                left -= take;
            }
        }
        v.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        v.extend_from_slice(&height.to_be_bytes());
        v.extend_from_slice(&width.to_be_bytes());
        v.extend_from_slice(&[0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        v.extend_from_slice(&[
            0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00,
        ]);
        v.extend_from_slice(&[0x00, 0xFF, 0xD9]);
        v
    }

    fn sample_file() -> Vec<u8> {
        let mut bytes = chunk("header", HEADER_TEXT.as_bytes(), 0);
        bytes.extend(chunk("level_1", &jpeg(101, 68, 0), 7));
        bytes.extend(chunk("level_2", &jpeg(1616, 1077, 0), 11));
        bytes
    }

    #[test]
    fn parses_header_and_levels_in_file_order() {
        let bytes = sample_file();
        let lrprev = Lrprev::parse(&bytes).unwrap();
        assert_eq!(lrprev.chunks.len(), 3);
        assert_eq!(
            lrprev
                .chunks
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["header", "level_1", "level_2"]
        );
        assert_eq!(lrprev.header().unwrap(), HEADER_TEXT.as_bytes());
        let levels = lrprev.levels();
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0].data, jpeg(101, 68, 0).as_slice());
        assert_eq!(levels[1].data, jpeg(1616, 1077, 0).as_slice());

        // The two parsers compose: header text is `&str`, chunk payload is bytes.
        let text = std::str::from_utf8(lrprev.header().unwrap()).unwrap();
        let header = crate::header::PyramidHeader::parse(text).unwrap();
        assert_eq!(header.levels.len(), levels.len());
        assert_eq!(header.levels, vec![(101, 68), (1616, 1077)]);
    }

    #[test]
    fn skips_exactly_the_declared_padding_between_chunks() {
        // Non-zero paddings of every size seen in real files, back to back:
        // if any padding byte were consumed as chunk data this would fail.
        let mut bytes = chunk("header", b"pyramid = { }", 0);
        for (index, padding) in [0usize, 1, 2, 7, 9, 11].into_iter().enumerate() {
            bytes.extend(chunk(
                &format!("level_{}", index + 1),
                &jpeg(100 + index as u16, 60, 0),
                padding,
            ));
        }
        let lrprev = Lrprev::parse(&bytes).unwrap();
        assert_eq!(lrprev.chunks.len(), 7);
        assert_eq!(
            lrprev
                .chunks
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "header", "level_1", "level_2", "level_3", "level_4", "level_5", "level_6"
            ]
        );
        for (index, c) in lrprev.levels().iter().enumerate() {
            assert_eq!(c.data, jpeg(100 + index as u16, 60, 0).as_slice());
        }
    }

    #[test]
    fn trims_chunk_names_at_the_first_nul() {
        let bytes = chunk("level_3\0\0\0", b"payload", 0);
        let lrprev = Lrprev::parse(&bytes).unwrap();
        assert_eq!(lrprev.chunks[0].name, "level_3");
        assert_eq!(lrprev.chunks[0].data, b"payload");
        assert_eq!(level_index(&lrprev.chunks[0].name), Some(3));
    }

    #[test]
    fn exact_eof_on_a_chunk_boundary_is_success() {
        // The fixture ends immediately after the last padding byte.
        let bytes = sample_file();
        let lrprev = Lrprev::parse(&bytes).expect("a file ending on a chunk boundary must parse");
        assert_eq!(lrprev.chunks.len(), 3);
        assert_eq!(
            lrprev.chunks.last().unwrap().data,
            jpeg(1616, 1077, 0).as_slice()
        );
    }

    #[test]
    fn empty_input_is_a_header_error() {
        assert!(matches!(Lrprev::parse(&[]), Err(Error::LrprevHeader(_))));
    }

    #[test]
    fn bad_magic_anywhere_is_a_header_error() {
        let bytes = sample_file();
        let mut bad_first = bytes.clone();
        bad_first[0] = b'X';
        assert!(matches!(
            Lrprev::parse(&bad_first),
            Err(Error::LrprevHeader(_))
        ));

        let second_start = chunk("header", HEADER_TEXT.as_bytes(), 0).len();
        let mut bad_second = bytes;
        bad_second[second_start] = b'X';
        assert!(matches!(
            Lrprev::parse(&bad_second),
            Err(Error::LrprevHeader(_))
        ));
    }

    #[test]
    fn header_length_below_the_fixed_part_is_a_header_error() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&23u16.to_be_bytes()); // 23 < 24
        bytes.push(0);
        bytes.push(0);
        bytes.extend_from_slice(&0u64.to_be_bytes());
        bytes.extend_from_slice(&0u64.to_be_bytes());
        assert!(matches!(Lrprev::parse(&bytes), Err(Error::LrprevHeader(_))));
    }

    #[test]
    fn payload_running_past_eof_is_truncated_at_the_chunk_start() {
        let mut bytes = chunk("header", HEADER_TEXT.as_bytes(), 0);
        let second_start = bytes.len();
        bytes.extend(chunk("level_1", &jpeg(1616, 1077, 0), 5));
        // Cut inside the level payload.
        bytes.truncate(second_start + 24 + "level_1".len() + 10);
        assert!(matches!(
            Lrprev::parse(&bytes),
            Err(Error::LrprevTruncated { offset, .. }) if offset == second_start
        ));
    }

    #[test]
    fn a_final_chunk_may_omit_its_trailing_padding() {
        let mut bytes = chunk("header", HEADER_TEXT.as_bytes(), 0);
        bytes.extend(chunk("level_1", &jpeg(101, 68, 0), 9));
        // Lightroom omits the final chunk's padding entirely (154 of 3,057
        // real files do this). The payload is intact, so this must parse.
        bytes.truncate(bytes.len() - 9);
        let parsed = Lrprev::parse(&bytes).expect("payload is complete, padding is optional");
        assert_eq!(parsed.levels().len(), 1);
        assert_eq!(
            crate::jpeg::dimensions(parsed.levels()[0].data).unwrap(),
            (101, 68)
        );
    }

    #[test]
    fn a_payload_running_past_eof_is_still_truncated() {
        let mut bytes = chunk("header", HEADER_TEXT.as_bytes(), 0);
        let second_start = bytes.len();
        bytes.extend(chunk("level_1", &jpeg(101, 68, 0), 0));
        // Cut inside the payload, not the padding.
        bytes.truncate(bytes.len() - 8);
        assert!(matches!(
            Lrprev::parse(&bytes),
            Err(Error::LrprevTruncated { offset, .. }) if offset == second_start
        ));
    }

    #[test]
    fn chunk_header_running_past_eof_is_truncated_at_the_chunk_start() {
        let mut bytes = chunk("header", HEADER_TEXT.as_bytes(), 0);
        let second_start = bytes.len();
        bytes.extend(chunk("level_1", &jpeg(101, 68, 0), 0));
        // Keep only 12 bytes of the second chunk's 24-byte fixed header.
        bytes.truncate(second_start + 12);
        assert!(matches!(
            Lrprev::parse(&bytes),
            Err(Error::LrprevTruncated { offset, .. }) if offset == second_start
        ));
    }

    #[test]
    fn a_middle_chunk_cut_inside_its_padding_is_truncated() {
        let mut bytes = chunk("header", HEADER_TEXT.as_bytes(), 0);
        let second_start = bytes.len();
        bytes.extend(chunk("level_1", &jpeg(101, 68, 0), 9));
        let third_start = bytes.len();
        bytes.extend(chunk("level_2", &jpeg(1616, 1077, 0), 0));
        // Five of level_1's nine padding bytes are gone. Its payload is
        // complete, but it is not the final chunk, so the file was cut.
        bytes.truncate(third_start - 4);
        assert!(matches!(
            Lrprev::parse(&bytes),
            Err(Error::LrprevTruncated { offset, .. }) if offset == second_start
        ));
    }

    #[test]
    fn the_final_chunk_may_not_be_cut_inside_its_padding() {
        let mut bytes = chunk("header", HEADER_TEXT.as_bytes(), 0);
        let level_start = bytes.len();
        bytes.extend(chunk("level_1", &jpeg(101, 68, 0), 9));
        // Every declared byte is not there, and neither is the whole payload
        // boundary: only a payload that ends at EOF may skip its padding.
        bytes.truncate(bytes.len() - 4);
        assert!(matches!(
            Lrprev::parse(&bytes),
            Err(Error::LrprevTruncated { offset, .. }) if offset == level_start
        ));
    }

    #[test]
    fn best_level_picks_by_pixel_area_not_payload_size() {
        // level_1 has far more bytes but fewer pixels than level_2.
        let small_but_bulky = jpeg(101, 68, 40_000);
        let large_but_lean = jpeg(1616, 1077, 0);
        assert!(
            small_but_bulky.len() > large_but_lean.len(),
            "fixture must make the smaller level the byte-heaviest"
        );

        let mut bytes = chunk("header", HEADER_TEXT.as_bytes(), 0);
        bytes.extend(chunk("level_1", &small_but_bulky, 3));
        bytes.extend(chunk("level_2", &large_but_lean, 0));

        let lrprev = Lrprev::parse(&bytes).unwrap();
        let levels = lrprev.levels();
        assert_eq!(levels[0].data.len(), small_but_bulky.len());
        assert_eq!(levels[1].data.len(), large_but_lean.len());
        assert_eq!(lrprev.best_level().unwrap().name, "level_2");
    }

    #[test]
    fn best_level_skips_levels_without_a_readable_frame() {
        let mut bytes = chunk("header", HEADER_TEXT.as_bytes(), 0);
        bytes.extend(chunk("level_1", &jpeg(101, 68, 0), 0));
        // A bigger payload that is not a JPEG at all.
        bytes.extend(chunk("level_2", &vec![0u8; 500], 0));
        let lrprev = Lrprev::parse(&bytes).unwrap();
        assert_eq!(lrprev.best_level().unwrap().name, "level_1");
    }

    #[test]
    fn level_index_reads_the_trailing_number() {
        assert_eq!(level_index("level_1"), Some(1));
        assert_eq!(level_index("level_12"), Some(12));
        assert_eq!(level_index("level_"), None);
        assert_eq!(level_index("level_x"), None);
        assert_eq!(level_index("header"), None);
    }

    #[test]
    fn every_prefix_of_a_valid_file_is_answered_without_panicking() {
        let bytes = sample_file();
        for end in 0..=bytes.len() {
            let _ = Lrprev::parse(&bytes[..end]);
        }
        // Garbage must not panic either.
        let _ = Lrprev::parse(&[0xFF; 64]);
        let _ = Lrprev::parse(b"AgHgAgHgAgHg");
    }
}
