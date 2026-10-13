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

/// True for native executable hashes, which the release lock overrides.
///
/// Native `b3-` hashes name the running binary, not a release; when the
/// lock pins this exact version and target, the release pin wins so
/// release binaries validate release evidence.
fn is_native_exe_hash(sha: &str) -> bool {
    sha.starts_with("b3-")
}

/// Resolve the plan generator SHA against the release lock.
///
/// Markers (empty, all-zero, unresolved) and native executable hashes
/// fill from the lock when it pins this exact version and target;
/// explicit 64-hex caller-supplied SHAs always win untouched. Anything
/// unresolvable keeps its marker and skips live lookup.
pub(crate) fn resolve_generator_identity(plan: &mut Plan, root: &std::path::Path) {
    if !is_source_build(&plan.generator.sha256) && !is_native_exe_hash(&plan.generator.sha256) {
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
            baseline: velnor_actions_contract::PlanBaseline {
                status: velnor_actions_contract::BaselineStatus::Unavailable,
                base_commit: None,
                run_id: None,
                artifact_id: None,
                artifact_name: None,
                manifest_digest: None,
                reason: None,
            },
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
            "schema = 1\n[generator]\nbinary = \"velnor-actions\"\nversion = \"{version}\"\n{bins}[mise-bootstrap]\nversion = \"2026.9.16\"\nartifact = \"https://example.invalid/mise\"\nsha256 = \"{}\"\n",
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
        assert!(is_native_exe_hash(&digest_b3(b"exe")));
        assert!(!is_native_exe_hash(&"1".repeat(64)));
    }

    #[test]
    fn lock_fills_markers_and_native_but_never_callers() {
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
        let mut plan = generator(version, target, &digest_b3(b"exe"));
        resolve_generator_identity(&mut plan, tmp.path());
        assert_eq!(plan.generator.sha256, sha);
        let mut plan = generator(version, target, &"f".repeat(64));
        resolve_generator_identity(&mut plan, tmp.path());
        assert_eq!(plan.generator.sha256, "f".repeat(64));
        let mut plan = generator("9.9.9", target, UNRESOLVED_GENERATOR_SHA);
        resolve_generator_identity(&mut plan, tmp.path());
        assert_eq!(plan.generator.sha256, UNRESOLVED_GENERATOR_SHA);
    }
}
