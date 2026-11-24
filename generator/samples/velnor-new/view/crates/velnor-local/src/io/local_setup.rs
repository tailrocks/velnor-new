// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The local variant of `tailrocks/velnor-new/actions/setup-velnor`.
//!
//! The production action never mentions a local binary. Under `velnor-local up --velnor-ci-binary`
//! the api-stub answers the runner's download of that action with a tarball of this variant, and
//! no workflow, step or file in a job can make GitHub's or a scale set's runner do the same.

const ACTION: &str = include_str!("local_setup/action.yml");
const TOP: &str = "velnor-local-setup";
const PATH: &str = "actions/setup-velnor/action.yml";
const BLOCK: usize = 0xffff;

/// The repository whose action downloads the stub answers with the variant.
pub const REPOSITORY: &str = "tailrocks/velnor-new";

#[must_use]
pub const fn variant_action() -> &'static str {
    ACTION
}

/// A gzip-compressed tar of the variant, with one top directory as the runner expects.
#[must_use]
pub fn tarball() -> Vec<u8> {
    let mut tar = Vec::new();
    let mut name = format!("{TOP}/");
    header(&mut tar, &name, b'5', 0);
    for part in PATH.split('/').take(2) {
        name = format!("{name}{part}/");
        header(&mut tar, &name, b'5', 0);
    }
    header(&mut tar, &format!("{TOP}/{PATH}"), b'0', ACTION.len());
    tar.extend_from_slice(ACTION.as_bytes());
    tar.resize(tar.len().next_multiple_of(512) + 1024, 0);
    gzip(&tar)
}

fn field(block: &mut [u8; 512], at: usize, text: &[u8]) {
    block[at..at + text.len()].copy_from_slice(text);
}

fn header(tar: &mut Vec<u8>, name: &str, kind: u8, size: usize) {
    let mut block = [0_u8; 512];
    field(&mut block, 0, name.as_bytes());
    let mode: &[u8] = if kind == b'5' {
        b"0000755\0"
    } else {
        b"0000644\0"
    };
    field(&mut block, 100, mode);
    field(&mut block, 108, b"0000000\0");
    field(&mut block, 116, b"0000000\0");
    field(&mut block, 124, format!("{size:011o}\0").as_bytes());
    field(&mut block, 136, b"00000000000\0");
    field(&mut block, 148, b"        ");
    block[156] = kind;
    field(&mut block, 257, b"ustar\0");
    field(&mut block, 263, b"00");
    let sum: u32 = block.iter().map(|byte| u32::from(*byte)).sum();
    field(&mut block, 148, format!("{sum:06o}\0 ").as_bytes());
    tar.extend_from_slice(&block);
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Gzip with stored deflate blocks: the file is a few KiB and has no use for compression.
fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
    let mut blocks = bytes.chunks(BLOCK).peekable();
    while let Some(chunk) = blocks.next() {
        out.push(u8::from(blocks.peek().is_none()));
        let length = u16::try_from(chunk.len()).unwrap_or(u16::MAX);
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&(!length).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&crc32(bytes).to_le_bytes());
    out.extend_from_slice(&u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_le_bytes());
    out
}
