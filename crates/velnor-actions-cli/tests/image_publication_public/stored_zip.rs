// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

pub(super) fn archive(entries: &[(&str, &[u8])]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut bytes = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let name = name.as_bytes();
        let name_len = u16::try_from(name.len())?;
        let size = u32::try_from(data.len())?;
        let offset = u32::try_from(bytes.len())?;
        let mut crc = flate2::Crc::new();
        crc.update(data);
        let sum = crc.sum();
        local(&mut bytes, name_len, size, sum);
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(data);
        directory(&mut central, name_len, size, sum, offset);
        central.extend_from_slice(name);
    }
    let offset = u32::try_from(bytes.len())?;
    let size = u32::try_from(central.len())?;
    let count = u16::try_from(entries.len())?;
    bytes.extend_from_slice(&central);
    u32le(&mut bytes, 0x0605_4b50);
    u16le(&mut bytes, 0);
    u16le(&mut bytes, 0);
    u16le(&mut bytes, count);
    u16le(&mut bytes, count);
    u32le(&mut bytes, size);
    u32le(&mut bytes, offset);
    u16le(&mut bytes, 0);
    Ok(bytes)
}

fn local(out: &mut Vec<u8>, name: u16, size: u32, crc: u32) {
    u32le(out, 0x0403_4b50);
    u16le(out, 20);
    u16le(out, 0);
    u16le(out, 0);
    u16le(out, 0);
    u16le(out, 0x21);
    u32le(out, crc);
    u32le(out, size);
    u32le(out, size);
    u16le(out, name);
    u16le(out, 0);
}

fn directory(out: &mut Vec<u8>, name: u16, size: u32, crc: u32, offset: u32) {
    u32le(out, 0x0201_4b50);
    u16le(out, 0x0314);
    u16le(out, 20);
    u16le(out, 0);
    u16le(out, 0);
    u16le(out, 0);
    u16le(out, 0x21);
    u32le(out, crc);
    u32le(out, size);
    u32le(out, size);
    u16le(out, name);
    u16le(out, 0);
    u16le(out, 0);
    u16le(out, 0);
    u16le(out, 0);
    u32le(out, 0o100_644 << 16);
    u32le(out, offset);
}

fn u16le(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn u32le(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
