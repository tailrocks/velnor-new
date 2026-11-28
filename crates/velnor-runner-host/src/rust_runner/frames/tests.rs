// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::encode;

#[test]
fn the_jit_frame_comes_first_then_the_trusted_facts_each_with_a_big_endian_length() {
    let bytes = encode("jit", b"{}").expect("two small frames");
    assert_eq!(&bytes[..], b"\0\0\0\x03jit\0\0\0\x02{}");
}

#[test]
fn a_frame_of_64_kib_goes_through_and_one_byte_more_or_nothing_is_refused() {
    let largest = "x".repeat(64 * 1024);
    assert!(encode(&largest, largest.as_bytes()).is_some());
    let over = "x".repeat(64 * 1024 + 1);
    assert!(encode(&over, b"{}").is_none(), "a JIT over 64 KiB");
    assert!(
        encode("jit", over.as_bytes()).is_none(),
        "facts over 64 KiB"
    );
    assert!(encode("", b"{}").is_none(), "an empty JIT");
    assert!(encode("jit", b"").is_none(), "empty facts");
}
