// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{admit_header, compare_digest, finish, read_blob};
use std::io::Cursor;

#[test]
fn git_batch_frames_preserve_binary_and_reject_malformed_content()
-> Result<(), Box<dyn std::error::Error>> {
    let oid = "a".repeat(40);
    let frame = |header: String, body: &[u8]| {
        let mut bytes = header.into_bytes();
        bytes.extend_from_slice(body);
        Cursor::new(bytes)
    };
    let mut valid = frame(format!("{oid} blob 4\n"), b"\0\n\xffx\n");
    assert_eq!(read_blob(&mut valid)?.1, b"\0\n\xffx");
    assert!(finish(&mut valid).is_ok());
    for (header, body) in [
        (format!("{}\n", "x".repeat(128)), b"".as_slice()),
        (format!("{oid} tree 1\n"), b"x\n".as_slice()),
        (format!("{oid} missing\n"), b"".as_slice()),
        (format!("{oid} blob 2\n"), b"x".as_slice()),
        (format!("{oid} blob 1\n"), b"xx".as_slice()),
    ] {
        assert!(read_blob(&mut frame(header, body)).is_err());
    }
    let mut oversized = vec![b'x'; 8 * 1024 * 1024 + 1];
    oversized.push(b'\n');
    assert!(read_blob(&mut frame(format!("{oid} blob 8388609\n"), &oversized)).is_err());
    let mut trailing = frame(format!("{oid} blob 1\n"), b"x\ny");
    assert_eq!(read_blob(&mut trailing)?.1, b"x");
    assert!(finish(&mut trailing).is_err());
    Ok(())
}

#[test]
fn compares_committed_binary_bytes_before_recording_digest()
-> Result<(), Box<dyn std::error::Error>> {
    let committed = b"a\0\n\xff";
    assert_eq!(
        compare_digest(committed, committed)?,
        "3638cc321f1fac9e8c8be937620f2c1e8250ce24e6b3f0f7a4302ea5a8faddc8"
    );
    assert!(compare_digest(committed, b"a\0\n\xfe").is_err());
    Ok(())
}

#[test]
fn binds_content_frames_to_ordered_preflight_headers() {
    let first = ("a".repeat(40), 3);
    let second = ("b".repeat(40), 4);
    assert!(admit_header(&first, &first).is_ok());
    assert!(admit_header(&second, &second).is_ok());
    assert!(admit_header(&first, &second).is_err());
    assert!(admit_header(&first, &("a".repeat(40), 4)).is_err());
    assert!(admit_header(&first, &("b".repeat(40), 3)).is_err());
}
