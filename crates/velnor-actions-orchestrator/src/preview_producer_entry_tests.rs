use super::*;
use velnor_actions_contract::{
    PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlanProduct,
    package_release::{PackageReleaseChannel, PackageReleasePayloadFormat},
};

fn plan() -> PackageReleasePlan {
    PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: "owner/repo".to_owned(),
        default_branch: "main".to_owned(),
        base_version: "1.2.3".to_owned(),
        products: vec![PackageReleasePlanProduct {
            id: "cli".to_owned(),
            package: "cli".to_owned(),
            bins: vec!["cli".to_owned()],
            targets: vec!["aarch64-apple-darwin".to_owned()],
            format: PackageReleasePayloadFormat::TarGz,
        }],
        companions: Vec::new(),
    }
}

#[test]
fn preview_entry_rejects_mismatched_workflow_authority() {
    let plan = plan();
    let sha = "0123456789abcdef0123456789abcdef01234567";
    let reference = "refs/heads/main";
    let workflow_ref = format!("OWNER/REPO/.github/workflows/binary-preview.yml@{reference}");
    assert!(workflow_identity_matches(
        &plan,
        "OWNER/REPO",
        reference,
        sha,
        sha,
        &workflow_ref
    ));
    for (repository, branch_ref, source_sha, authority_sha, workflow) in [
        ("attacker/repo", reference, sha, sha, workflow_ref.as_str()),
        (
            "owner/repo",
            "refs/heads/other",
            sha,
            sha,
            workflow_ref.as_str(),
        ),
        (
            "owner/repo",
            reference,
            sha,
            "ffffffffffffffffffffffffffffffffffffffff",
            workflow_ref.as_str(),
        ),
        (
            "owner/repo",
            reference,
            sha,
            sha,
            "owner/repo/.github/workflows/other.yml@refs/heads/main",
        ),
    ] {
        assert!(!workflow_identity_matches(
            &plan,
            repository,
            branch_ref,
            source_sha,
            authority_sha,
            workflow
        ));
    }
}

fn expired_deadline() -> CheckDeadline {
    CheckDeadline::from_start(
        Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("expired fixture start"),
        Duration::from_millis(1),
    )
    .expect("expired deadline")
}

#[test]
fn expired_producer_cannot_stage_a_success_handoff_or_write_proof() {
    let root = tempfile::tempdir().expect("root");
    let verified = crate::package_release_artifacts::VerifiedPackageOutput {
        source_commit: "a".repeat(40),
        source_ref: "refs/heads/main".to_owned(),
        version: "1.2.3-preview.1+aaaaaaa".to_owned(),
        artifacts: Vec::new(),
    };
    assert!(
        stage_raw_output(
            root.path(),
            root.path(),
            "handoff.json",
            verified,
            expired_deadline()
        )
        .is_err()
    );
    assert!(!root.path().join("preview-output").exists());
    let proof = root.path().join(PROOF_NAME);
    assert!(write_proof(&proof, b"{}", expired_deadline()).is_err());
    assert!(!proof.exists());
}

#[test]
fn proof_write_reuses_outer_deadline_and_preserves_existing_evidence() {
    let root = tempfile::tempdir().expect("root");
    let proof = root.path().join(PROOF_NAME);
    let deadline = CheckDeadline::after(Duration::from_secs(5)).expect("deadline");
    write_proof(&proof, b"original", deadline).expect("positive proof write");
    assert!(!proof.with_extension("pending").exists());
    assert!(write_proof(&proof, b"replacement", deadline).is_err());
    assert_eq!(fs::read(&proof).expect("retained proof"), b"original");
    let late = root.path().join("late-proof.json");
    let outer = CheckDeadline::after(Duration::from_millis(10)).expect("deadline");
    let result = outer.scope(|| {
        std::thread::sleep(Duration::from_millis(30));
        write_proof(
            &late,
            b"{}",
            CheckDeadline::after(Duration::from_secs(5)).expect("fresh local budget"),
        )
    });
    assert!(result.is_err());
    assert!(!late.exists());
}

#[test]
fn incomplete_pending_proof_is_never_admitted_or_overwritten() {
    let root = tempfile::tempdir().expect("root");
    let proof = root.path().join(PROOF_NAME);
    let pending = proof.with_extension("pending");
    fs::write(&pending, b"incomplete").expect("retained pending bytes");
    assert!(
        write_proof(
            &proof,
            b"{}",
            CheckDeadline::after(Duration::from_secs(5)).expect("deadline")
        )
        .is_err()
    );
    assert!(!proof.exists());
    assert_eq!(fs::read(&pending).expect("retained pending"), b"incomplete");
}

#[test]
fn artifact_admission_checks_expired_outer_budget_before_copying() {
    let root = tempfile::tempdir().expect("root");
    fs::write(root.path().join("handoff.json"), b"{}").expect("handoff");
    let staging = root.path().join("staging");
    let deadline = CheckDeadline::after(Duration::from_millis(10)).expect("deadline");
    let result = deadline.scope(|| {
        std::thread::sleep(Duration::from_millis(30));
        let error = verify_package_output(
            root.path(),
            "handoff.json",
            &staging,
            &[],
            &[],
            PackageOutputIdentity {
                source_commit: &"a".repeat(40),
                source_ref: "refs/heads/main",
                version: "1.2.3-preview.1+aaaaaaa",
            },
        )
        .expect_err("outer deadline precedes manifest parsing");
        assert!(
            error
                .to_string()
                .contains("package_output_verification_timeout")
        );
    });
    assert!(result.is_err());
    assert!(!staging.exists());
}

fn write_proof(
    path: &Path,
    bytes: &[u8],
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    with_proof_ownership(|guard| publish_proof(path, bytes, deadline, guard))
}

#[test]
fn post_publication_outer_deadline_failure_removes_only_owned_proof() {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join(PROOF_NAME);
    let live = CheckDeadline::after(Duration::from_secs(5)).expect("live deadline");
    let result = with_proof_ownership(|guard| {
        live.scope(|| publish_proof(&path, b"{}", live, guard))
            .map_err(|error| internal(&error.to_string()))??;
        assert!(
            path.is_file(),
            "publication succeeded before outer checkpoint"
        );
        // Deterministic checkpoint hook: inject the expired outer-boundary result
        // after publication, without sleeping or changing any global clock.
        checkpoint(expired_deadline())
    });
    assert!(result.is_err());
    assert!(
        !path.exists(),
        "outer failure cleans the operation's final proof"
    );
    assert!(!path.with_extension("pending").exists());
}

#[test]
fn outer_failure_preserves_a_replaced_foreign_proof_identity() {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join(PROOF_NAME);
    let retained = root.path().join("retained-original");
    let deadline = CheckDeadline::after(Duration::from_secs(5)).expect("deadline");
    let result = with_proof_ownership(|guard| {
        publish_proof(&path, b"owned", deadline, guard)?;
        fs::rename(&path, &retained).expect("preserve owned inode");
        fs::write(&path, b"foreign").expect("replacement identity");
        checkpoint(expired_deadline())
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&path).expect("foreign proof retained"), b"foreign");
    assert_eq!(fs::read(&retained).expect("owned bytes retained"), b"owned");
}

#[test]
fn unwind_after_publication_cleans_owned_proof() {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join(PROOF_NAME);
    let result = std::panic::catch_unwind(|| {
        drop(with_proof_ownership(|guard| {
            publish_proof(
                &path,
                b"{}",
                CheckDeadline::after(Duration::from_secs(5)).expect("deadline"),
                guard,
            )?;
            panic!("fixture outer-boundary unwind");
        }));
    });
    assert!(result.is_err());
    assert!(!path.exists());
}
