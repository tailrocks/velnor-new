// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! ZIP archives the way GitHub serves an Actions artifact, built for the tests.

pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 * (crc & 1));
        }
    }
    !crc
}

/// A ZIP of stored entries: `(name, data)`, with the given flag bits and Unix mode.
pub(crate) fn zip_of(entries: &[(&str, &[u8])], flags: u16, mode: u32) -> Vec<u8> {
    let (mut body, mut directory) = (Vec::new(), Vec::new());
    for (name, data) in entries {
        let at = u32::try_from(body.len()).unwrap_or(0);
        let (crc, size, len) = (
            crc32(data),
            u32::try_from(data.len()).unwrap_or(0),
            u16::try_from(name.len()).unwrap_or(0),
        );
        body.extend_from_slice(&0x0403_4b50_u32.to_le_bytes());
        body.extend_from_slice(&[20, 0]);
        body.extend_from_slice(&flags.to_le_bytes());
        body.extend_from_slice(&[0; 6]);
        for value in [crc, size, size] {
            body.extend_from_slice(&value.to_le_bytes());
        }
        body.extend_from_slice(&len.to_le_bytes());
        body.extend_from_slice(&[0, 0]);
        body.extend_from_slice(name.as_bytes());
        body.extend_from_slice(data);
        directory.extend_from_slice(&0x0201_4b50_u32.to_le_bytes());
        directory.extend_from_slice(&[0x1e, 3, 20, 0]);
        directory.extend_from_slice(&flags.to_le_bytes());
        directory.extend_from_slice(&[0; 6]);
        for value in [crc, size, size] {
            directory.extend_from_slice(&value.to_le_bytes());
        }
        directory.extend_from_slice(&len.to_le_bytes());
        directory.extend_from_slice(&[0; 8]);
        directory.extend_from_slice(&(mode << 16).to_le_bytes());
        directory.extend_from_slice(&at.to_le_bytes());
        directory.extend_from_slice(name.as_bytes());
    }
    let count = u16::try_from(entries.len()).unwrap_or(0);
    let (start, size) = (
        u32::try_from(body.len()).unwrap_or(0),
        u32::try_from(directory.len()).unwrap_or(0),
    );
    body.extend_from_slice(&directory);
    body.extend_from_slice(&0x0605_4b50_u32.to_le_bytes());
    body.extend_from_slice(&[0; 4]);
    body.extend_from_slice(&count.to_le_bytes());
    body.extend_from_slice(&count.to_le_bytes());
    body.extend_from_slice(&size.to_le_bytes());
    body.extend_from_slice(&start.to_le_bytes());
    body.extend_from_slice(&[0, 0]);
    body
}
