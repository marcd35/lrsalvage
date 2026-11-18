//! `exif` reading bytes this module did not write.
//!
//! A payload's own metadata is untrusted input from a dead drive, and a
//! destination may be one an older build wrote; both arrive in byte orders and
//! segment layouts this writer never emits. `lrsalvage-fixtures` builds those
//! bytes; `specs/fixtures.md` says why such a test lives here, not in
//! `src/exif.rs`.

use lrsalvage_core::Orientation;
use lrsalvage_core::exif;
use lrsalvage_core::jpeg;
use lrsalvage_fixtures::bytes;

#[test]
fn a_payloads_own_orientation_is_read_back() {
    // The fixture builder writes the other byte order, so this also proves the
    // reader handles a TIFF block this module did not write.
    let own = bytes::jpeg_with(4, 4, &[bytes::exif_app1(6)]);
    let segments = jpeg::certify(&own).expect("a valid frame certifies");
    assert_eq!(
        exif::orientation(&own, &segments.segments),
        Some(Orientation::Rotate90)
    );

    // A mirrored value is not one this writer can restate, so it declares
    // nothing usable and the catalog's orientation applies.
    let mirrored = bytes::jpeg_with(4, 4, &[bytes::exif_app1(2)]);
    let segments = jpeg::certify(&mirrored).expect("a valid frame certifies");
    assert_eq!(exif::orientation(&mirrored, &segments.segments), None);
}

#[test]
fn a_destinations_marker_is_read_from_the_other_byte_order() {
    // A resume reads whatever a destination happens to be, and a destination
    // another build wrote carries the marker in the other byte order: the
    // identity must survive a TIFF block this writer would not produce.
    let big_endian = bytes::our_output(4, 4, "0.1.0");
    let segments = jpeg::certify(&big_endian).expect("a valid frame certifies");
    assert_eq!(
        exif::software(&big_endian, &segments.segments).as_deref(),
        Some("lrsalvage 0.1.0")
    );
}
