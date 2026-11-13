//! Property tests: the parsers read untrusted files from dead drives, so the
//! first duty is "never panic, never allocate unboundedly".
//!
//! These complement the hand-written `every_prefix_…` tests inside each module:
//! proptest mutates structure, not just length.

use lrsalvage_core::{jpeg, lrprev, model};
use proptest::prelude::*;

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig {
        cases: 256,
        ..proptest::prelude::ProptestConfig::default()
    })]

    /// Any byte sequence is answered with `Ok` or `Err`, never a panic.
    #[test]
    fn the_container_parser_survives_arbitrary_bytes(
        bytes in proptest::collection::vec(any::<u8>(), 0..2048)
    ) {
        let _ = lrprev::Lrprev::parse(&bytes);
    }

    /// Same for the JPEG header walker, which is reached from two paths.
    #[test]
    fn the_jpeg_reader_survives_arbitrary_bytes(
        bytes in proptest::collection::vec(any::<u8>(), 0..2048)
    ) {
        let _ = jpeg::dimensions(&bytes);
        let _ = jpeg::is_complete(&bytes);
    }

    /// The mask decoder is the one parser that allocates from declared sizes,
    /// so it must refuse absurd dimensions instead of aborting.
    #[cfg(feature = "masks")]
    #[test]
    fn the_mask_decoder_survives_arbitrary_bytes(
        bytes in proptest::collection::vec(any::<u8>(), 0..2048)
    ) {
        let _ = lrsalvage_core::masks::decode_dat(&bytes);
    }

    /// A sanitised path component can never reintroduce traversal or a
    /// separator, whatever the catalog contains.
    #[test]
    fn sanitised_components_stay_relative_and_single(name in ".*") {
        if let Some(clean) = model::sanitize_component(&name) {
            proptest::prop_assert!(!clean.is_empty());
            proptest::prop_assert!(!clean.contains('/'));
            proptest::prop_assert!(!clean.contains('\\'));
            proptest::prop_assert!(clean != ".");
            proptest::prop_assert!(clean != "..");
            proptest::prop_assert!(!clean.contains(':'));
        }
    }

    /// Truncating a valid container anywhere still yields a result, never a
    /// panic — the property that catches off-by-one slicing.
    #[test]
    fn every_truncation_point_is_answered(
        cut in 0usize..64usize,
        fill in proptest::collection::vec(any::<u8>(), 0..512)
    ) {
        let mut bytes = b"AgHg".to_vec();
        bytes.extend(fill);
        let cut = cut.min(bytes.len());
        let _ = lrprev::Lrprev::parse(&bytes[..cut]);
    }
}
