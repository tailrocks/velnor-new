// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Clone, Copy)]
pub(crate) enum Program {
    Sleep,
    Exit,
}

const LOAD_ADDRESS: u64 = 0x40_0000;
const HEADER_BYTES: u64 = 120;

const X86_SLEEP: [u8; 9] = [0xb8, 0x22, 0, 0, 0, 0x0f, 0x05, 0xeb, 0xf7];
const X86_EXIT: [u8; 12] = [0xbf, 1, 0, 0, 0, 0xb8, 0x3c, 0, 0, 0, 0x0f, 0x05];
const ARM_SLEEP: [u32; 7] = [
    0xd280_0000,
    0xd280_0001,
    0xd280_0002,
    0xd280_0003,
    0xd280_0928,
    0xd400_0001,
    0x17ff_fffa,
];
const ARM_EXIT: [u32; 3] = [0xd280_0020, 0xd280_0ba8, 0xd400_0001];

fn code(program: Program) -> (u16, Vec<u8>) {
    if cfg!(target_arch = "aarch64") {
        let words: &[u32] = match program {
            Program::Sleep => &ARM_SLEEP,
            Program::Exit => &ARM_EXIT,
        };
        (
            183,
            words.iter().flat_map(|word| word.to_le_bytes()).collect(),
        )
    } else {
        let bytes: &[u8] = match program {
            Program::Sleep => &X86_SLEEP,
            Program::Exit => &X86_EXIT,
        };
        (62, bytes.to_vec())
    }
}

pub(crate) fn elf(program: Program) -> Vec<u8> {
    let (machine, code) = code(program);
    let total = HEADER_BYTES + code.len() as u64;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&machine.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&(LOAD_ADDRESS + HEADER_BYTES).to_le_bytes());
    bytes.extend_from_slice(&64_u64.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    for half in [64_u16, 56, 1, 0, 0, 0] {
        bytes.extend_from_slice(&half.to_le_bytes());
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&5_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes.extend_from_slice(&LOAD_ADDRESS.to_le_bytes());
    bytes.extend_from_slice(&LOAD_ADDRESS.to_le_bytes());
    bytes.extend_from_slice(&total.to_le_bytes());
    bytes.extend_from_slice(&total.to_le_bytes());
    bytes.extend_from_slice(&0x1000_u64.to_le_bytes());
    bytes.extend_from_slice(&code);
    bytes
}
