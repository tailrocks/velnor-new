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
    #[cfg(target_os = "linux")]
    let bytes = std::fs::read("/proc/self/exe")
        .ok()
        .or_else(sealed_execution_bytes);
    #[cfg(not(target_os = "linux"))]
    let bytes = std::env::current_exe()
        .ok()
        .and_then(|exe| std::fs::read(exe).ok());
    let digest = bytes.map(|bytes| sha256_hex(&bytes));
    #[expect(
        clippy::needless_option_as_deref,
        reason = "the original Option must remain available after this branch"
    )]
    if let (Some(phases), Some(started)) = (phases.as_deref_mut(), started) {
        phases.generator_sha_us += started.elapsed().as_micros();
    }
    digest
}

#[cfg(target_os = "linux")]
fn sealed_execution_bytes() -> Option<Vec<u8>> {
    use std::io::{Read, Seek};

    let path = rustix::param::linux_execfn().to_str().ok()?;
    let raw_fd = path.strip_prefix("/proc/self/fd/")?;
    let fd = raw_fd.parse::<u32>().ok()?;
    if fd < 3 || fd.to_string() != raw_fd {
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    const MAX_BYTES: u64 = 256 * 1024 * 1024;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_BYTES {
        return None;
    }
    let required = rustix::fs::SealFlags::SEAL
        | rustix::fs::SealFlags::SHRINK
        | rustix::fs::SealFlags::GROW
        | rustix::fs::SealFlags::WRITE;
    if !rustix::fs::fcntl_get_seals(&file).ok()?.contains(required) {
        return None;
    }
    file.rewind().ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes).ok()?;
    (u64::try_from(bytes.len()).ok()? == metadata.len()).then_some(bytes)
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
