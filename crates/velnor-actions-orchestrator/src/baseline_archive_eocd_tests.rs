//! Ambiguous EOCD signatures are rejected before the ZIP crate parses them.

use super::*;

const EOCD_BYTES: usize = 22;

#[test]
fn ambiguous_eocd_candidates_are_rejected_by_preflight() {
    let mut archive = test_archive("baseline.json", b"{}");
    let eocd = eocd_offset(&archive);
    set_u16(
        &mut archive,
        eocd + 20,
        u16::try_from(EOCD_BYTES).expect("EOCD length fits"),
    );
    let mut embedded_eocd = [0; EOCD_BYTES];
    embedded_eocd[..4].copy_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
    archive.extend_from_slice(&embedded_eocd);

    assert!(matches!(
        zip_admission::preflight_zip(&archive),
        Err(reason) if reason == "baseline_archive_invalid"
    ));
    assert!(extract_baseline(&metadata(99, &archive), &archive).is_err());
}

#[test]
fn zip64_eocd_candidate_inside_comment_is_rejected_before_parser() {
    let mut archive = test_archive("baseline.json", b"{}");
    let eocd = eocd_offset(&archive);
    let mut locator = [0; 20];
    locator[..4].copy_from_slice(&[0x50, 0x4b, 0x06, 0x07]);
    let mut candidate = archive[eocd..eocd + EOCD_BYTES].to_vec();
    set_u16(&mut candidate, 8, u16::MAX);
    set_u16(&mut candidate, 10, u16::MAX);
    set_u32(&mut candidate, 12, u32::MAX);
    set_u32(&mut candidate, 16, u32::MAX);
    set_u16(&mut candidate, 20, 0);
    let mut comment = locator.to_vec();
    comment.extend_from_slice(&candidate);
    comment.extend_from_slice(b"tail");
    set_u16(
        &mut archive,
        eocd + 20,
        u16::try_from(comment.len()).expect("bounded ZIP comment"),
    );
    archive.extend_from_slice(&comment);

    let embedded_eocd = eocd + EOCD_BYTES + locator.len();
    assert_eq!(read_u16(&archive, embedded_eocd + 8), u16::MAX);
    assert_eq!(read_u32(&archive, embedded_eocd + 12), u32::MAX);
    assert!(embedded_eocd + EOCD_BYTES < archive.len());
    assert!(zip_admission::preflight_zip(&archive).is_err());
    assert!(extract_baseline(&metadata(99, &archive), &archive).is_err());
}

#[test]
fn earlier_eocd_signature_inside_entry_data_is_rejected() {
    let archive = stored_archive(b"PK\x05\x06");
    assert!(zip_admission::preflight_zip(&archive).is_err());
    assert!(extract_baseline(&metadata(99, &archive), &archive).is_err());
}
