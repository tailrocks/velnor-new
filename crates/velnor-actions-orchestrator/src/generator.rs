// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use crate::internal::phase_timing::PlanPhaseTimings;

pub(crate) fn current_exe_sha256() -> Option<String> {
    current_exe_sha256_with_phase_timings(None)
}

pub(crate) fn current_exe_sha256_with_phase_timings(
    mut phases: Option<&mut PlanPhaseTimings>,
) -> Option<String> {
    if let Some(phases) = phases.as_deref_mut() {
        phases.generator_sha_calls += 1;
    }
    let started = phases.as_ref().map(|_| Instant::now());
    let digest = std::env::current_exe()
        .ok()
        .and_then(|exe| std::fs::read(exe).ok())
        .map(|bytes| sha256_hex(&bytes));
    #[expect(
        clippy::needless_option_as_deref,
        reason = "the original Option must remain available after this branch"
    )]
    if let (Some(phases), Some(started)) = (phases.as_deref_mut(), started) {
        phases.generator_sha_us += started.elapsed().as_micros();
    }
    digest
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    hex_lower(sha2::Sha256::digest(bytes).as_slice())
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}
