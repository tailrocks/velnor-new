//! Derived GitHub artifact names and target keys.
use crate::errors::ContractError;
use crate::ids::{is_component_byte, validate_matrix_key, validate_run_key};

/// Derive the plan artifact name `velnor-plan-<run-key>`.
/// # Errors
pub fn artifact_id_for_plan(run_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    let id = format!("velnor-plan-{run_key}");
    Ok(super::ArtifactId::parse(&id)?.into_inner())
}

/// Derive the matrix artifact name `velnor-matrix-<run-key>-<matrix-key>`.
/// # Errors
pub fn artifact_id_for_matrix(run_key: &str, matrix_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    validate_matrix_key(matrix_key)?;
    let id = format!("velnor-matrix-{run_key}-{matrix_key}");
    Ok(super::ArtifactId::parse(&id)?.into_inner())
}

/// Derive the final artifact name `velnor-final-<run-key>`.
/// # Errors
pub fn artifact_id_for_final(run_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    let id = format!("velnor-final-{run_key}");
    Ok(super::ArtifactId::parse(&id)?.into_inner())
}

/// Derive one job's report artifact `velnor-crate-<run-key>-<job-id>`.
///
/// Each matrix job uploads exactly one artifact carrying every entry's
/// matrix report plus task reports (implementation-plan contract); the
/// job ID is the stable crate-job (or plan-job) ID, validated by the
/// job-ID grammar so the name round-trips through validation.
/// # Errors
pub fn artifact_id_for_crate_job(run_key: &str, job_id: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    crate::workflow::jobs::validate_job_id(job_id)
        .map_err(|_| ContractError::identity("artifact_id", "bad_job_artifact"))?;
    let id = format!("velnor-crate-{run_key}-{job_id}");
    Ok(super::ArtifactId::parse(&id)?.into_inner())
}

/// Derive `velnor-baseline-<commit>-<compat>` with full IDs (par §5).
/// # Errors
pub fn artifact_id_for_baseline(commit: &str, compat: &str) -> Result<String, ContractError> {
    if !super::is_lower_hex_len(commit, 40) {
        return Err(ContractError::identity("artifact_id", "bad_source_commit"));
    }
    crate::canonical::validate_digest(compat)
        .map_err(|_| ContractError::identity("artifact_id", "bad_compatibility_id"))?;
    let id = format!("velnor-baseline-{commit}-{compat}");
    Ok(super::ArtifactId::parse(&id)?.into_inner())
}

/// Validate a target-key shape (matches [`target_key`] output grammar).
/// # Errors
pub fn validate_target_key(value: &str) -> Result<(), ContractError> {
    if is_target_key(value) {
        Ok(())
    } else {
        Err(ContractError::identity(
            "target_key",
            "malformed_target_key",
        ))
    }
}

/// Validate a derived artifact name (plan/matrix/final/crate/candidate).
/// # Errors
pub fn validate_artifact_id(value: &str) -> Result<(), ContractError> {
    if let Some(run_key) = value.strip_prefix("velnor-plan-") {
        return validate_run_key(run_key)
            .map_err(|_| ContractError::identity("artifact_id", "bad_plan_artifact"));
    }
    if let Some(run_key) = value.strip_prefix("velnor-final-") {
        return validate_run_key(run_key)
            .map_err(|_| ContractError::identity("artifact_id", "bad_final_artifact"));
    }
    if let Some(rest) = value.strip_prefix("velnor-matrix-") {
        let Some((run_key, hex)) = rest.rsplit_once("-m-") else {
            return Err(ContractError::identity(
                "artifact_id",
                "bad_matrix_artifact",
            ));
        };
        validate_run_key(run_key)
            .map_err(|_| ContractError::identity("artifact_id", "bad_matrix_artifact"))?;
        return validate_matrix_key(&format!("m-{hex}"))
            .map_err(|_| ContractError::identity("artifact_id", "bad_matrix_artifact"));
    }
    if let Some(rest) = value.strip_prefix("velnor-crate-") {
        return validate_job_artifact(rest);
    }
    if let Some(rest) = value.strip_prefix("velnor-candidate-") {
        return validate_candidate_artifact(rest);
    }
    if let Some(rest) = value.strip_prefix("velnor-baseline-") {
        return validate_baseline_artifact(rest);
    }
    Err(ContractError::identity(
        "artifact_id",
        "unknown_artifact_kind",
    ))
}

/// Convert a target triple to a target key (lowercase, `-` runs collapsed).
/// # Errors
pub fn target_key(target: &str) -> Result<String, ContractError> {
    if target.is_empty() {
        return Err(ContractError::identity("target_key", "empty_target"));
    }
    let mut key = String::with_capacity(target.len());
    let mut dash_pending = true;
    for byte in target.bytes() {
        if byte.is_ascii_alphanumeric() {
            key.push(byte.to_ascii_lowercase() as char);
            dash_pending = false;
        } else if !dash_pending {
            key.push('-');
            dash_pending = true;
        }
    }
    while key.ends_with('-') {
        key.pop();
    }
    if key.is_empty() {
        return Err(ContractError::identity("target_key", "empty_target_key"));
    }
    Ok(super::TargetKey::parse(&key)?.into_inner())
}

/// Validate the run-key/job-ID tail of a crate-job artifact name.
///
/// Both halves admit `-`, so every split is tried (same approach as
/// the candidate tail): the name validates when some split yields a
/// valid run key plus a valid job ID.
fn validate_job_artifact(rest: &str) -> Result<(), ContractError> {
    for (index, _) in rest.match_indices('-') {
        let head = &rest[..index];
        let tail = &rest[index + 1..];
        if validate_run_key(head).is_ok() && crate::workflow::jobs::validate_job_id(tail).is_ok() {
            return Ok(());
        }
    }
    Err(ContractError::identity("artifact_id", "bad_job_artifact"))
}

/// Validate the run-key/target-key tail of a candidate artifact name.
fn validate_candidate_artifact(rest: &str) -> Result<(), ContractError> {
    for (index, _) in rest.match_indices('-') {
        let head = &rest[..index];
        let tail = &rest[index + 1..];
        if validate_run_key(head).is_ok() && is_target_key(tail) {
            return Ok(());
        }
    }
    Err(ContractError::identity(
        "artifact_id",
        "bad_candidate_artifact",
    ))
}

/// Validate the commit/compat tail of a baseline artifact name.
fn validate_baseline_artifact(rest: &str) -> Result<(), ContractError> {
    let bad = || ContractError::identity("artifact_id", "bad_baseline_artifact");
    let Some((commit, compat)) = rest.split_once('-') else {
        return Err(bad());
    };
    if !super::is_lower_hex_len(commit, 40) {
        return Err(bad());
    }
    crate::canonical::validate_digest(compat).map_err(|_| bad())
}

/// Check target-key shape (matches [`target_key`] output grammar).
fn is_target_key(tail: &str) -> bool {
    !tail.is_empty()
        && tail != "."
        && tail != ".."
        && !tail.starts_with('-')
        && !tail.ends_with('-')
        && !tail.contains("--")
        && tail.bytes().all(is_component_byte)
}

#[cfg(test)]
mod tests {
    use super::super::{
        ArtifactId, ManifestKey, MatrixId, MatrixKey, PlanId, ReportId, RunKey, TargetKey, TaskId,
        TaskReportId,
    };
    use super::validate_artifact_id;
    use super::{artifact_id_for_crate_job, artifact_id_for_plan, target_key};
    use crate::canonical::{Digest, PosixPath, digest_b3, digest_b3_typed};
    use crate::workflow::baseline::{
        BaselineProof, BaselineStatus, ManifestTaskProof, PlanBaseline,
    };

    /// Parse/serde round-trip must accept `good` and reject `bad`.
    macro_rules! check_id {
        ($t:ty, $good:expr, $bad:expr) => {{
            let parsed = <$t>::parse($good).expect("valid id");
            assert_eq!(parsed.as_str(), $good);
            assert!(<$t>::parse($bad).is_err(), "accepted {}", $bad);
            let json = serde_json::to_string(&parsed).expect("serialize");
            let back = serde_json::from_str::<$t>(&json).expect("deserialize");
            assert_eq!(back, parsed);
            let bad_json = format!("\"{}\"", $bad);
            assert!(
                serde_json::from_str::<$t>(&bad_json).is_err(),
                "serde {bad_json}"
            );
        }};
    }

    #[test]
    fn newtypes_accept_valid_and_reject_invalid() {
        check_id!(RunKey, "local", "Local");
        check_id!(RunKey, "r12-a3", "r-a");
        check_id!(ManifestKey, "root", "Root");
        check_id!(ManifestKey, "crates/foo", "../escape");
        check_id!(TaskId, "stack/rust/root/clippy/default", "stack/RUST");
        check_id!(TaskId, "internal/plan/default", "internal/only");
        check_id!(
            MatrixId,
            "stack:rust|task:stack/rust/root/clippy/default",
            "stack:rust"
        );
        check_id!(MatrixKey, "m-0123456789abcdef", "m-XYZ");
        check_id!(PlanId, "plan-local", "plan-");
        check_id!(ReportId, "report-local-m-0123456789abcdef", "report-x");
        check_id!(
            TaskReportId,
            "task-local-m-0123456789abcdef-0123456789abcdef",
            "task-local"
        );
        check_id!(ArtifactId, "velnor-plan-local", "velnor-nope-x");
        check_id!(TargetKey, "x86-64-unknown-linux-gnu", "bad--key");
        assert!(
            target_key("X86_64-Unknown-Linux-GNU").is_ok_and(|k| k == "x86-64-unknown-linux-gnu")
        );
        assert_eq!(
            artifact_id_for_plan("local").expect("plan"),
            "velnor-plan-local"
        );
    }

    #[test]
    fn crate_job_artifacts_derive_and_validate_per_job() {
        let id = artifact_id_for_crate_job("local", "rust-demo").expect("derive");
        assert_eq!(id, "velnor-crate-local-rust-demo");
        assert_eq!(
            artifact_id_for_crate_job("r7-a2", "plan").expect("plan job"),
            "velnor-crate-r7-a2-plan"
        );
        assert!(validate_artifact_id(&id).is_ok());
        assert!(validate_artifact_id("velnor-crate-r7-a2-rust-demo").is_ok());
        for bad in [
            "velnor-crate-local",
            "velnor-crate-",
            "velnor-crate-local-",
            "velnor-crate-Local-rust-demo",
            "velnor-crate-local-RUST-DEMO",
            "velnor-crate-local-velnor-plan",
        ] {
            assert!(validate_artifact_id(bad).is_err(), "accepted {bad}");
        }
        assert!(artifact_id_for_crate_job("Local", "rust-demo").is_err());
        assert!(artifact_id_for_crate_job("local", "").is_err());
        assert!(artifact_id_for_crate_job("local", "velnor-plan").is_err());
    }

    #[test]
    fn digest_and_path_newtypes_validate() {
        let raw = digest_b3(b"bytes");
        let typed = digest_b3_typed(b"bytes");
        assert_eq!(typed.as_str(), raw);
        assert_eq!(typed.prefix16().len(), 16);
        check_id!(Digest, &raw, "b3-not-hex");
        assert!(Digest::parse("B3-AAAA").is_err());
        let path = PosixPath::parse("crates/foo").expect("posix");
        assert_eq!(path.as_str(), "crates/foo");
        assert_eq!(PosixPath::parse("a\\b").expect("slash").as_str(), "a/b");
        assert!(PosixPath::parse("/abs").is_err());
        assert!(PosixPath::parse("a/../b").is_err());
        assert!(PosixPath::parse("").is_err());
        let json = serde_json::to_string(&path).expect("serialize");
        assert_eq!(serde_json::from_str::<PosixPath>(&json).expect("de"), path);
        assert!(serde_json::from_str::<PosixPath>("\"/abs\"").is_err());
    }

    /// Sample manifest digest for proof tests.
    fn proof_digest() -> String {
        digest_b3(b"manifest")
    }

    /// Sample source commit for proof tests.
    fn proof_commit() -> String {
        "ab".repeat(20)
    }

    #[test]
    fn proof_constructor_validates_every_input() {
        let digest = proof_digest();
        let commit = proof_commit();
        let proof =
            BaselineProof::new(&commit, 7, 9, "velnor-plan-local", &digest).expect("valid proof");
        assert_eq!(proof.run_id(), 7);
        assert_eq!(proof.artifact_id(), 9);
        assert_eq!(proof.source_commit(), commit);
        assert_eq!(proof.artifact_name(), "velnor-plan-local");
        assert_eq!(proof.manifest_digest(), digest);
        proof.validate().expect("revalidate");
        assert!(BaselineProof::new("short", 7, 9, "velnor-plan-local", &digest).is_err());
        assert!(BaselineProof::new(&commit, 0, 9, "velnor-plan-local", &digest).is_err());
        assert!(BaselineProof::new(&commit, 7, 0, "velnor-plan-local", &digest).is_err());
        assert!(BaselineProof::new(&commit, 7, 9, "velnor-nope-x", &digest).is_err());
        assert!(BaselineProof::new(&commit, 7, 9, "velnor-plan-local", "b3-nope").is_err());
        let json = serde_json::to_string(&proof).expect("serialize");
        assert!(serde_json::from_str::<BaselineProof>(&json).is_ok());
        let forged = json.replace(&digest, "b3-nope");
        assert!(serde_json::from_str::<BaselineProof>(&forged).is_err());
    }

    #[test]
    fn baseline_states_are_exhaustive() {
        let digest = proof_digest();
        let commit = proof_commit();
        let used = PlanBaseline::used(&commit, 7, 9, "velnor-plan-local", &digest).expect("used");
        assert_eq!(used.status(), BaselineStatus::Used);
        assert!(PlanBaseline::used("short", 7, 9, "velnor-plan-local", &digest).is_err());
        let mut stale = used;
        stale.mark_unavailable("baseline_expired").expect("mark");
        assert_eq!(stale.status(), BaselineStatus::Unavailable);
        assert_eq!(stale.reason(), Some("baseline_expired"));
        assert!(stale.mark_unavailable("").is_err());
        let wire = serde_json::to_string(&stale).expect("serialize");
        assert!(!wire.contains("base_commit"));
        let with_stale = r#"{"status":"unavailable","run_id":7}"#;
        assert!(serde_json::from_str::<PlanBaseline>(with_stale).is_err());
        assert!(serde_json::from_str::<PlanBaseline>(r#"{"status":"used"}"#).is_err());
        let fresh = PlanBaseline::used(&commit, 7, 9, "velnor-plan-local", &digest).expect("used");
        let mut wire_used: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&fresh).expect("ser")).expect("de");
        wire_used["reason"] = serde_json::json!("stale");
        assert!(serde_json::from_value::<PlanBaseline>(wire_used).is_err());
    }

    #[test]
    fn task_proof_needs_valid_ids_and_digests() {
        let task = "stack/rust/root/clippy/default";
        let good = proof_digest();
        let build = |task: &str, td: &str, run: u64| {
            ManifestTaskProof::new(task, td, &good, &good, &good, &good, &good, "default", run)
        };
        let proof = build(task, &good, 7).expect("valid task proof");
        assert_eq!(proof.task_id(), task);
        assert_eq!(proof.proof_run_id(), 7);
        proof.validate().expect("revalidate");
        assert!(build("bogus", &good, 7).is_err());
        assert!(build(task, "b3-nope", 7).is_err());
        assert!(build(task, &good, 0).is_err());
        let json = serde_json::to_string(&proof).expect("serialize");
        assert!(serde_json::from_str::<ManifestTaskProof>(&json).is_ok());
        let forged = json.replace(&good, "b3-nope");
        assert!(serde_json::from_str::<ManifestTaskProof>(&forged).is_err());
    }

    #[test]
    fn canonical_hex_pins_charset_and_rejects_empty_at_width() {
        use super::super::{is_lower_hex, is_lower_hex_len, validate_matrix_key};
        assert!(is_lower_hex("0123456789abcdef"));
        assert!(is_lower_hex(""), "charset-only vacuity is deliberate");
        for bad in ["ABCDEF", "ab cd", "ab\n", "xyz"] {
            assert!(!is_lower_hex(bad), "charset must reject {bad:?}");
        }
        assert!(is_lower_hex_len(&"a".repeat(40), 40));
        assert!(!is_lower_hex_len("", 40), "empty fails every width");
        assert!(!is_lower_hex_len(&"a".repeat(39), 40));
        assert!(!is_lower_hex_len(&"A".repeat(40), 40));
        assert!(validate_matrix_key("m-").is_err(), "empty hex fails");
        assert!(validate_matrix_key("m-0123456789ABCDEF").is_err());
    }
}
