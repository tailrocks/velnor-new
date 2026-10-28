// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{Read, Seek, SeekFrom};

const GNU_INTERPRETER: &[u8] = b"/lib64/ld-linux-x86-64.so.2\0";

pub(super) fn target(
    reader: &mut (impl Read + Seek),
    file_size: u64,
) -> Result<&'static str, String> {
    if file_size < 20 {
        return Err("unsupported executable: truncated header".into());
    }
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|error| io_error(&error))?;
    let mut header = [0; 64];
    let header_len = file_size.min(64) as usize;
    reader
        .read_exact(&mut header[..header_len])
        .map_err(|error| io_error(&error))?;
    if header[..4] == [0x7f, b'E', b'L', b'F'] {
        return elf_target(reader, file_size, &header, header_len);
    }
    if header[..4] == [0xcf, 0xfa, 0xed, 0xfe] {
        if header_len < 32 || little_u32(&header[12..16]) != 2 {
            return Err("unsupported executable: invalid or truncated Mach-O header".into());
        }
        return match little_u32(&header[4..8]) {
            0x0100_000c => Ok("aarch64-apple-darwin"),
            0x0100_0007 => Ok("x86_64-apple-darwin"),
            _ => Err("unsupported executable: unexpected Mach-O architecture".into()),
        };
    }
    Err("unsupported executable: unknown format".into())
}

fn elf_target(
    reader: &mut (impl Read + Seek),
    file_size: u64,
    header: &[u8; 64],
    header_len: usize,
) -> Result<&'static str, String> {
    if header_len != 64
        || header[4] != 2
        || header[5] != 1
        || header[6] != 1
        || !matches!(little_u16(&header[16..18]), 2 | 3)
        || little_u16(&header[18..20]) != 62
        || little_u32(&header[20..24]) != 1
        || little_u16(&header[52..54]) != 64
        || little_u16(&header[54..56]) != 56
    {
        return Err("unsupported executable: invalid ELF64 x86-64 header".into());
    }
    if header[36..40] != [0; 4] {
        return Err(
            "unsupported executable: program table offset exceeds the supported range".into(),
        );
    }
    let table_offset = u64::from(little_u32(&header[32..36]));
    let count = u64::from(little_u16(&header[56..58]));
    let table_bytes = count
        .checked_mul(56)
        .ok_or("invalid ELF program table bounds")?;
    if count == 0 || table_offset > file_size || table_bytes > file_size - table_offset {
        return Err("unsupported executable: invalid ELF program table bounds".into());
    }
    let mut matched = 0_u8;
    for index in 0..count {
        let offset = table_offset + index * 56;
        reader
            .seek(SeekFrom::Start(offset))
            .map_err(|error| io_error(&error))?;
        let mut program = [0; 56];
        reader
            .read_exact(&mut program)
            .map_err(|error| io_error(&error))?;
        if little_u32(&program[..4]) == 3 {
            check_interpreter(reader, file_size, &program)?;
            matched = matched.saturating_add(1);
        }
    }
    if matched == 1 {
        Ok("x86_64-unknown-linux-gnu")
    } else {
        Err("unsupported executable: missing or duplicate GNU ELF interpreter".into())
    }
}

fn check_interpreter(
    reader: &mut (impl Read + Seek),
    file_size: u64,
    program: &[u8; 56],
) -> Result<(), String> {
    let offset = u64::from(little_u32(&program[8..12]));
    let size = u64::from(little_u32(&program[32..36]));
    if program[12..16] != [0; 4]
        || program[36..40] != [0; 4]
        || size != GNU_INTERPRETER.len() as u64
        || offset > file_size
        || size > file_size - offset
    {
        return Err("unsupported executable: invalid ELF interpreter bounds".into());
    }
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(|error| io_error(&error))?;
    let mut interpreter = [0; 28];
    reader
        .read_exact(&mut interpreter)
        .map_err(|error| io_error(&error))?;
    if interpreter != GNU_INTERPRETER {
        return Err("unsupported executable: unexpected ELF interpreter".into());
    }
    Ok(())
}

fn little_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn little_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn io_error(error: &std::io::Error) -> String {
    error.to_string()
}
