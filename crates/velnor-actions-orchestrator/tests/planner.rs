// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[expect(
    dead_code,
    reason = "T40 exercises the native task closure through path includes; unused sibling items stay compiled"
)]
#[path = "../src/native_mise_tasks.rs"]
pub(crate) mod native_mise_tasks;
#[expect(
    dead_code,
    reason = "T40 exercises the native task closure through path includes; unused sibling items stay compiled"
)]
#[path = "../src/native_mise_version.rs"]
pub(crate) mod native_mise_version;
#[expect(
    dead_code,
    reason = "T40 exercises the native task closure through path includes; unused sibling items stay compiled"
)]
#[path = "../src/native_tool_input.rs"]
pub(crate) mod native_tool_input;
#[expect(
    dead_code,
    reason = "T40 exercises the native task closure through path includes; unused sibling items stay compiled"
)]
#[path = "../src/native_tool_lock.rs"]
pub(crate) mod native_tool_lock;

#[expect(
    dead_code,
    reason = "T40 path includes need the real digest spelling; only the test closure validation runs"
)]
pub(crate) mod generator {
    pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::Digest as _;
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let digest = sha2::Sha256::digest(bytes);
        let mut out = String::with_capacity(digest.len() * 2);
        for byte in digest {
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0xf) as usize] as char);
        }
        out
    }
}
