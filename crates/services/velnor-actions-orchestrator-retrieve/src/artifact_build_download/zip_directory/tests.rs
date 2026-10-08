use super::super::central_entry_count;
use std::io::{Cursor, Write};

#[test]
fn zip64_entry_count_comes_from_zip64_end_record() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("one.bin", zip::write::SimpleFileOptions::default())
        .expect("start ZIP entry");
    writer.write_all(b"one").expect("write ZIP entry");
    let mut bytes = writer.finish().expect("finish ZIP").into_inner();
    let eocd = bytes.len() - 22;
    let central_bytes = u32::from_le_bytes(
        bytes[eocd + 12..eocd + 16]
            .try_into()
            .expect("central size"),
    );
    let central_offset = u32::from_le_bytes(
        bytes[eocd + 16..eocd + 20]
            .try_into()
            .expect("central offset"),
    );

    let zip64_offset = u64::try_from(eocd).expect("ZIP64 offset");
    let mut zip64_end = [0_u8; 56];
    zip64_end[..4].copy_from_slice(b"PK\x06\x06");
    zip64_end[4..12].copy_from_slice(&44_u64.to_le_bytes());
    zip64_end[12..14].copy_from_slice(&45_u16.to_le_bytes());
    zip64_end[14..16].copy_from_slice(&45_u16.to_le_bytes());
    zip64_end[24..32].copy_from_slice(&1_u64.to_le_bytes());
    zip64_end[32..40].copy_from_slice(&1_u64.to_le_bytes());
    zip64_end[40..48].copy_from_slice(&u64::from(central_bytes).to_le_bytes());
    zip64_end[48..56].copy_from_slice(&u64::from(central_offset).to_le_bytes());

    let mut locator = [0_u8; 20];
    locator[..4].copy_from_slice(b"PK\x06\x07");
    locator[8..16].copy_from_slice(&zip64_offset.to_le_bytes());
    locator[16..20].copy_from_slice(&1_u32.to_le_bytes());
    let mut zip64_records = Vec::from(zip64_end);
    zip64_records.extend_from_slice(&locator);
    bytes.splice(eocd..eocd, zip64_records);
    let updated_eocd = eocd + 76;
    bytes[updated_eocd + 8..updated_eocd + 10].copy_from_slice(&u16::MAX.to_le_bytes());
    bytes[updated_eocd + 10..updated_eocd + 12].copy_from_slice(&u16::MAX.to_le_bytes());

    let count = central_entry_count(
        &mut Cursor::new(bytes.as_slice()),
        u64::try_from(bytes.len()).expect("ZIP bytes"),
    )
    .expect("read ZIP64 directory count");
    assert_eq!(count, 1);
    assert_eq!(
        zip::ZipArchive::new(Cursor::new(bytes))
            .expect("parse ZIP64 archive")
            .len(),
        1
    );
}
