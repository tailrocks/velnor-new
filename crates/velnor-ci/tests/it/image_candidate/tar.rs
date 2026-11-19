// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use velnor_ci::image_candidate::tar::{TarError, TarReader};

use super::fixture::{header, member};

fn reseal(block: &mut [u8]) {
    block[148..156].copy_from_slice(b"        ");
    let sum: u32 = block.iter().map(|byte| u32::from(*byte)).sum();
    block[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
}

fn first_entry(bytes: Vec<u8>) -> Result<Option<Vec<u8>>, TarError> {
    let mut archive = bytes;
    archive.extend_from_slice(&[0u8; 1024]);
    let mut reader = TarReader::new(Cursor::new(archive));
    Ok(reader.next_entry()?.map(|entry| entry.name))
}

#[test]
fn a_member_size_that_overflows_the_block_padding_is_refused() {
    let mut block = header("a", 0, b'0');
    block[124] = 0x80;
    block[125..136].fill(0xff);
    reseal(&mut block);
    assert!(matches!(first_entry(block), Err(TarError::MemberSize)));
}

#[test]
fn the_last_gnu_long_name_applies() {
    let mut archive = member("././@LongLink", b"first\0", b'L');
    archive.extend(member("././@LongLink", b"second\0", b'L'));
    archive.extend(member("short", b"x", b'0'));
    let name = first_entry(archive).ok().flatten();
    assert_eq!(name.as_deref(), Some(b"second".as_slice()));
}

#[test]
fn a_pax_record_with_an_empty_key_is_refused() {
    let mut archive = member("pax", b"6 =ab\n", b'x');
    archive.extend(member("short", b"x", b'0'));
    assert!(matches!(
        first_entry(archive),
        Err(TarError::Format("pax record"))
    ));
}

#[test]
fn a_numeric_header_field_that_is_not_octal_is_refused() {
    for (from, text) in [
        (100, b"\t000644\0"),
        (108, b"00 0 0\0\0"),
        (116, b"0000x00\0"),
    ] {
        let mut block = header("a", 0, b'0');
        block[from..from + 8].copy_from_slice(text);
        reseal(&mut block);
        assert!(
            matches!(
                first_entry(block),
                Err(TarError::Format("numeric header field"))
            ),
            "{from}"
        );
    }
}
