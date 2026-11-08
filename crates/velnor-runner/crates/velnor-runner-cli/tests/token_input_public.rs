// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "../src/dispatch/token_input.rs"]
mod token_input;

#[test]
fn token_line_accepts_optional_single_line_ending() {
    for (input, expected) in [
        (b"secret-token".as_slice(), b"secret-token".as_slice()),
        (b"secret-token\n".as_slice(), b"secret-token".as_slice()),
        (b"secret-token\r\n".as_slice(), b"secret-token".as_slice()),
    ] {
        assert_eq!(token_input::normalize_token_line(input), Some(expected));
    }
}

#[test]
fn token_line_rejects_empty_whitespace_control_and_non_ascii_bytes() {
    for input in [
        b"".as_slice(),
        b"\n".as_slice(),
        b"\r\n".as_slice(),
        b"secret token".as_slice(),
        b"secret\ntoken".as_slice(),
        b"secret\r".as_slice(),
        b"secret\n\n".as_slice(),
        b"secret\r\n\r\n".as_slice(),
        b"secret\x7f".as_slice(),
        b"secret\x80".as_slice(),
        b"\xff".as_slice(),
    ] {
        assert_eq!(token_input::normalize_token_line(input), None, "{input:?}");
    }
}
