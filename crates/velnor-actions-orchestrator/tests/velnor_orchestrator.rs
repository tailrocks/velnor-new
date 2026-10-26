// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod gc1_qualification_cases;
mod gc1_qualification_support;
mod gc1_runtime_cases;
mod gc1_t07;
mod gc1_t08;
mod gc1_t09;
mod gc1_t10;
mod gc1_t12;
mod gc1_t15;
mod gc1_t19;
mod gc1_t20;
mod gc1_t21;
mod gc1_t22;
mod gc1_t24;
mod gc1_t25;
mod gc1_t26;
mod gc1_t27;
mod gc1_t32;
mod gc1_t33;
mod gc1_t34;
mod gc1_t35;
mod gc1_t36;
mod gc1_t39;
mod gc1_t40;
mod gc1_t41;
mod gc1_t49;
mod gc1_t51;
mod gc1_t52;

pub use velnor_actions_orchestrator::OrchestratorError;

#[expect(
    dead_code,
    reason = "T40 exercises the native task closure through path includes; unused sibling items stay compiled"
)]
#[path = "../src/native_mise_tasks.rs"]
mod native_mise_tasks;
#[expect(
    dead_code,
    reason = "T40 exercises the native task closure through path includes; unused sibling items stay compiled"
)]
#[path = "../src/native_mise_version.rs"]
mod native_mise_version;
#[expect(
    dead_code,
    reason = "T40 exercises the native task closure through path includes; unused sibling items stay compiled"
)]
#[path = "../src/native_tool_input.rs"]
mod native_tool_input;
#[expect(
    dead_code,
    reason = "T40 exercises the native task closure through path includes; unused sibling items stay compiled"
)]
#[path = "../src/native_tool_lock.rs"]
mod native_tool_lock;

#[expect(
    dead_code,
    reason = "T40 path includes need the real digest spelling; only the test closure validation runs"
)]
mod generator {
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
