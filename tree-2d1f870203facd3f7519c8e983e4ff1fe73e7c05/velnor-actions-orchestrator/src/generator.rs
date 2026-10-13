//! Generator identity resolution and verification (P03).
//!
//! Declared via `#[path]` from `cover_identity.rs` (no `lib.rs` edit).
//! A generator SHA is verifiable only when it names a real binary: the
//! running executable's hash or a release-lock pin. All-zero, empty, and
//! unresolved-marker SHAs prove nothing and never validate evidence.

use velnor_actions_contract::Plan;
use velnor_actions_mise::catalog::lock::{load_text, parse_generator_lock};

use crate::internal_plan::snapshot::UNRESOLVED_GENERATOR_SHA;

/// Bootstrap lock resolving release generator identity for source builds.
const GENERATOR_LOCK_REL: &str = ".velnor/generator.lock";

/// Lookup-skipped reason for an unverifiable source-build generator.
pub(crate) const SOURCE_BUILD_REASON: &str = "generator_unverifiable_source_build";

/// True for generator SHAs that prove nothing: empty, all-zero, or the
/// explicit unresolved marker. No release binary stands behind any of
/// them, so baseline evidence bound to them is unverifiable.
pub(crate) fn is_source_build(sha: &str) -> bool {
    sha.is_empty()
        || sha == UNRESOLVED_GENERATOR_SHA
        || (sha.len() == 64 && sha.bytes().all(|b| b == b'0'))
}

/// SHA-256 hex of the running executable, comparable to release pins.
///
/// Root cause of the b3-vs-SHA256 incomparability: the native `b3-`
/// digest can never equal a 64-hex release pin. Recording the real
/// SHA-256 makes executable-against-release comparison structural;
/// provenance matches this value against the manifest pin exactly.
pub(crate) fn current_exe_sha256() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let bytes = std::fs::read(exe).ok()?;
    Some(sha256_hex(&bytes))
}

/// Pure-Rust SHA-256 over bytes, lowercase hex (no new dependency).
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for word in &sha256_block(bytes) {
        for shift in (0..8).rev() {
            let nibble = usize::try_from((word >> (shift * 4)) & 0xf).unwrap_or(0);
            out.push(HEX.get(nibble).copied().unwrap_or(b'0') as char);
        }
    }
    out
}

/// SHA-256 round constants (first 32 bits of cube roots of primes).
const SHA256_K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// SHA-256 initial state (first 32 bits of square roots of primes).
const SHA256_H0: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// Message schedule for one 64-byte chunk.
fn sha256_schedule(chunk: &[u8]) -> [u32; 64] {
    let mut schedule = [0u32; 64];
    for (index, word) in schedule.iter_mut().enumerate().take(16) {
        let s = index * 4;
        *word = u32::from_be_bytes([chunk[s], chunk[s + 1], chunk[s + 2], chunk[s + 3]]);
    }
    for index in 16..64 {
        let a = schedule[index - 15];
        let b = schedule[index - 2];
        let s0 = a.rotate_right(7) ^ a.rotate_right(18) ^ (a >> 3);
        let s1 = b.rotate_right(17) ^ b.rotate_right(19) ^ (b >> 10);
        schedule[index] = schedule[index - 16]
            .wrapping_add(s0)
            .wrapping_add(schedule[index - 7])
            .wrapping_add(s1);
    }
    schedule
}

/// Sixty-four compression rounds over one schedule.
fn sha256_rounds(state: [u32; 8], schedule: &[u32; 64]) -> [u32; 8] {
    let mut w = state;
    for (index, k) in SHA256_K.iter().enumerate() {
        let s1 = w[4].rotate_right(6) ^ w[4].rotate_right(11) ^ w[4].rotate_right(25);
        let ch = (w[4] & w[5]) ^ ((!w[4]) & w[6]);
        let t1 = w[7]
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(*k)
            .wrapping_add(schedule[index]);
        let s0 = w[0].rotate_right(2) ^ w[0].rotate_right(13) ^ w[0].rotate_right(22);
        let maj = (w[0] & w[1]) ^ (w[0] & w[2]) ^ (w[1] & w[2]);
        let t2 = s0.wrapping_add(maj);
        w[7] = w[6];
        w[6] = w[5];
        w[5] = w[4];
        w[4] = w[3].wrapping_add(t1);
        w[3] = w[2];
        w[2] = w[1];
        w[1] = w[0];
        w[0] = t1.wrapping_add(t2);
    }
    w
}

/// SHA-256 compression over padded blocks; returns the eight words.
fn sha256_block(bytes: &[u8]) -> [u32; 8] {
    let mut state = SHA256_H0;
    let mut padded = bytes.to_vec();
    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in padded.chunks(64) {
        let schedule = sha256_schedule(chunk);
        let w = sha256_rounds(state, &schedule);
        for (slot, word) in state.iter_mut().zip(w) {
            *slot = slot.wrapping_add(word);
        }
    }
    state
}

/// Resolve the plan generator SHA against the release lock.
///
/// Only markers (empty, all-zero, unresolved) fill from the lock when
/// it pins this exact version and target. Native `b3-` hashes and
/// explicit 64-hex SHAs always win untouched: a `b3-` hash names the
/// running binary and can never equal a release pin, so overriding it
/// would bless a source build as a release. Anything unresolvable
/// keeps its marker and skips live lookup.
pub(crate) fn resolve_generator_identity(plan: &mut Plan, root: &std::path::Path) {
    if !is_source_build(&plan.generator.sha256) {
        return;
    }
    let path = root.join(GENERATOR_LOCK_REL);
    if !path.is_file() {
        return;
    }
    let Ok(text) = load_text(&path) else {
        return;
    };
    let Ok(lock) = parse_generator_lock(&text) else {
        return;
    };
    let Some(target) = release_target(&plan.generator.target) else {
        return;
    };
    if lock.generator.version != plan.generator.version {
        return;
    }
    if let Some(record) = lock.binary_for_target(target) {
        plan.generator.sha256.clone_from(&record.sha256);
    }
}

/// Release triple for a generator target: triples pass through, else map.
///
/// Default builds record `{arch}-{os}`; release locks pin full triples.
fn release_target(target: &str) -> Option<&str> {
    if velnor_actions_contract::SUPPORTED_TARGETS.contains(&target) {
        return Some(target);
    }
    match target {
        "x86_64-linux" => Some("x86_64-unknown-linux-gnu"),
        "aarch64-macos" => Some("aarch64-apple-darwin"),
        "x86_64-macos" => Some("x86_64-apple-darwin"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use velnor_actions_contract::{PlanGenerator, digest_b3};

    /// Plan generator with `version`/`target`/`sha`.
    fn generator(version: &str, target: &str, sha: &str) -> Plan {
        Plan {
            schema: 1,
            run_key: "local".to_owned(),
            plan_id: "plan-local".to_owned(),
            base: None,
            head: "head".to_owned(),
            event: velnor_actions_contract::WorkflowEvent::PullRequest,
            runner: velnor_actions_contract::PlanRunner {
                label: "ubuntu-26.04".to_owned(),
                selection: velnor_actions_contract::RunnerSelection::LatestDefault,
            },
            trust: velnor_actions_contract::Trust::Pr,
            baseline: velnor_actions_contract::PlanBaseline::unavailable(None)
                .expect("bare baseline"),
            generator: PlanGenerator {
                version: version.to_owned(),
                target: target.to_owned(),
                sha256: sha.to_owned(),
            },
            packages: Vec::new(),
            obligations: Vec::new(),
            matrix: velnor_actions_contract::PlanMatrix {
                include: Vec::new(),
            },
            task_ids: Vec::new(),
            warnings: Vec::new(),
            edges: Vec::new(),
        }
    }

    /// Generator lock pinning `version` to `sha` on every target.
    fn lock_text(version: &str, sha: &str) -> String {
        use std::fmt::Write as _;
        let mut bins = String::new();
        for target in velnor_actions_contract::SUPPORTED_TARGETS {
            write!(
                bins,
                "[[generator.binaries]]\ntarget = \"{target}\"\nartifact = \"https://example.invalid/r\"\nsha256 = \"{sha}\"\n"
            )
            .expect("write");
        }
        format!(
            "schema = 1\n[generator]\nbinary = \"velnor-actions\"\nversion = \"{version}\"\n{bins}[mise-bootstrap]\nversion = \"2026.9.18\"\nartifact = \"https://example.invalid/mise\"\nsha256 = \"{}\"\n",
            "c".repeat(64)
        )
    }

    #[test]
    fn unverifiable_shas_detected() {
        assert!(is_source_build(""));
        assert!(is_source_build(&"0".repeat(64)));
        assert!(is_source_build(UNRESOLVED_GENERATOR_SHA));
        assert!(!is_source_build(&"1".repeat(64)));
        assert!(!is_source_build(&digest_b3(b"exe")));
    }

    #[test]
    fn lock_fills_markers_but_never_native_or_callers() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let version = env!("CARGO_PKG_VERSION");
        let sha = "e".repeat(64);
        std::fs::create_dir(tmp.path().join(".velnor")).expect("dir");
        std::fs::write(
            tmp.path().join(".velnor/generator.lock"),
            lock_text(version, &sha),
        )
        .expect("lock");
        let target = "x86_64-unknown-linux-gnu";
        let mut plan = generator(version, target, UNRESOLVED_GENERATOR_SHA);
        resolve_generator_identity(&mut plan, tmp.path());
        assert_eq!(plan.generator.sha256, sha);
        let native = digest_b3(b"exe");
        let mut plan = generator(version, target, &native);
        resolve_generator_identity(&mut plan, tmp.path());
        assert_eq!(plan.generator.sha256, native);
        let mut plan = generator(version, target, &"f".repeat(64));
        resolve_generator_identity(&mut plan, tmp.path());
        assert_eq!(plan.generator.sha256, "f".repeat(64));
        let mut plan = generator("9.9.9", target, UNRESOLVED_GENERATOR_SHA);
        resolve_generator_identity(&mut plan, tmp.path());
        assert_eq!(plan.generator.sha256, UNRESOLVED_GENERATOR_SHA);
    }

    #[test]
    fn sha256_matches_vectors_and_exe_verifies() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let exe = current_exe_sha256().expect("exe readable");
        assert_eq!(exe.len(), 64);
        assert!(exe.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(!exe.starts_with("b3-"), "comparable to release pins");
    }
}
