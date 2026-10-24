use super::super::github::WorkflowJob;
use super::super::*;

#[test]
fn public_release_requires_only_exact_successful_assembly_signer_and_publisher_jobs() {
    let authority_sha = "b".repeat(40);
    let jobs = [
        "preview-assemble",
        "preview-signer",
        "Publish authenticated channel release",
    ]
    .map(|name| WorkflowJob {
        id: 0,
        run_id: 22,
        name: name.to_owned(),
        head_sha: Some(authority_sha.clone()),
        status: Some("completed".to_owned()),
        conclusion: Some("success".to_owned()),
    });
    verify_public_release_producer_jobs(
        &jobs,
        22,
        &authority_sha,
        "preview-assemble",
        "preview-signer",
    )
    .expect("all producing jobs succeeded despite a downstream run failure");
    let mut wrong_head = jobs.clone();
    wrong_head[1].head_sha = Some("a".repeat(40));
    assert!(
        verify_public_release_producer_jobs(
            &wrong_head,
            22,
            &authority_sha,
            "preview-assemble",
            "preview-signer"
        )
        .is_err()
    );
    let mut failed_publisher = jobs.clone();
    failed_publisher[2].conclusion = Some("failure".to_owned());
    assert!(
        verify_public_release_producer_jobs(
            &failed_publisher,
            22,
            &authority_sha,
            "preview-assemble",
            "preview-signer"
        )
        .is_err()
    );
}

#[test]
fn historical_recovery_requires_exact_cancelled_original_publisher_job() {
    let authority = "b".repeat(40);
    let jobs = vec![
        WorkflowJob {
            id: 1,
            run_id: 22,
            name: "preview-assemble".to_owned(),
            head_sha: Some(authority.clone()),
            status: Some("completed".to_owned()),
            conclusion: Some("success".to_owned()),
        },
        WorkflowJob {
            id: 2,
            run_id: 22,
            name: "preview-signer".to_owned(),
            head_sha: Some(authority.clone()),
            status: Some("completed".to_owned()),
            conclusion: Some("success".to_owned()),
        },
        WorkflowJob {
            id: 120_711_604_532,
            run_id: 22,
            name: "Publish authenticated channel release".to_owned(),
            head_sha: Some(authority.clone()),
            status: Some("completed".to_owned()),
            conclusion: Some("cancelled".to_owned()),
        },
    ];
    assert!(
        verify_public_recovery_original_jobs(
            &jobs,
            22,
            &authority,
            "preview-assemble",
            "preview-signer",
            120_711_604_532,
        )
        .is_ok()
    );
    let mut publisher_succeeded = jobs.clone();
    publisher_succeeded[2].conclusion = Some("success".to_owned());
    assert!(
        verify_public_recovery_original_jobs(
            &publisher_succeeded,
            22,
            &authority,
            "preview-assemble",
            "preview-signer",
            120_711_604_532,
        )
        .is_err()
    );
    assert!(
        verify_public_recovery_original_jobs(
            &jobs,
            22,
            &authority,
            "preview-assemble",
            "preview-signer",
            120_711_604_533,
        )
        .is_err()
    );
}

#[test]
fn historical_recovery_run_must_skip_every_rebuild_job() {
    let admission_bytes =
        include_bytes!("../../../.velnor/qualification/homebrew/preview-a-source-admission.json")
            .to_vec();
    let recovery =
        crate::package_release_public::HistoricalPackageReleaseRecovery::new_from_protected_policy(
            crate::package_release_public::HistoricalPackageReleaseRecoveryInput {
                original_publisher_job_id: 10,
                run_id: 22,
                run_attempt: 2,
                authority_sha: "b".repeat(40),
                recovery_job_id: 11,
                artifact_id: 12,
                artifact_name: "velnor-preview-source-r22-a2".to_owned(),
                admission_size: 34_639,
                admission_sha256:
                    "73eacadf794d40c16e39e6693d09741ce9146cefc867d135f4a1fee023daa972".to_owned(),
                policy_sha256: "d".repeat(64),
                admission_bytes,
            },
        )
        .expect("valid pinned recovery identity");
    let make_job = |id, name: &str, conclusion: &str| WorkflowJob {
        id,
        run_id: 22,
        name: name.to_owned(),
        head_sha: Some("b".repeat(40)),
        status: Some("completed".to_owned()),
        conclusion: Some(conclusion.to_owned()),
    };
    let jobs = vec![
        make_job(1, "Select authenticated main source for preview", "success"),
        make_job(
            11,
            "Recover the original signed release without rebuilding",
            "success",
        ),
        make_job(2, "Build product / target", "skipped"),
        make_job(3, "preview-assemble", "skipped"),
        make_job(4, "preview-signer", "skipped"),
        make_job(5, "Publish authenticated channel release", "skipped"),
    ];
    assert!(verify_public_recovery_jobs(&jobs, &recovery).is_ok());
    let mut rebuilt = jobs.clone();
    rebuilt[2].conclusion = Some("success".to_owned());
    assert!(verify_public_recovery_jobs(&rebuilt, &recovery).is_err());
}

#[test]
fn public_asset_metadata_is_rejected_by_kind_and_total_before_any_download() {
    use std::collections::BTreeMap;

    let limits = BTreeMap::from([
        ("release.json".to_owned(), 1024 * 1024),
        ("release.json.bundle".to_owned(), 8 * 1024 * 1024),
        ("payload.tar.gz".to_owned(), 1024 * 1024 * 1024),
        ("payload.tar.gz.sbom.json".to_owned(), 32 * 1024 * 1024),
    ]);
    let base = vec![
        ("release.json".to_owned(), 512 * 1024),
        ("release.json.bundle".to_owned(), 4 * 1024 * 1024),
        ("payload.tar.gz".to_owned(), 512 * 1024 * 1024),
        ("payload.tar.gz.sbom.json".to_owned(), 16 * 1024 * 1024),
    ];
    assert!(validate_public_asset_size_inventory(&base, &limits).is_ok());
    for (name, over_limit) in [
        ("release.json", 1024 * 1024 + 1),
        ("release.json.bundle", 8 * 1024 * 1024 + 1),
        ("payload.tar.gz", 1024 * 1024 * 1024 + 1),
        ("payload.tar.gz.sbom.json", 32 * 1024 * 1024 + 1),
    ] {
        let mut assets = base.clone();
        assets
            .iter_mut()
            .find(|(asset, _)| asset == name)
            .expect("listed asset")
            .1 = over_limit;
        assert!(
            validate_public_asset_size_inventory(&assets, &limits).is_err(),
            "oversized {name} was admitted"
        );
    }
    let huge_limits = BTreeMap::from([
        ("a".to_owned(), 2 * 1024 * 1024 * 1024),
        ("b".to_owned(), 2 * 1024 * 1024 * 1024),
        ("c".to_owned(), 2 * 1024 * 1024 * 1024),
        ("d".to_owned(), 2 * 1024 * 1024 * 1024),
    ]);
    let huge_assets = huge_limits
        .iter()
        .map(|(name, size)| (name.clone(), *size))
        .collect::<Vec<_>>();
    assert!(validate_public_asset_size_inventory(&huge_assets, &huge_limits).is_err());
}

#[test]
fn stable_and_preview_publications_never_request_latest() {
    assert!(!make_latest(PackageReleaseChannel::Stable));
    assert!(!make_latest(PackageReleaseChannel::Preview));
}

#[test]
fn release_json_lifecycle_admission_preserves_channel_prerelease_state() {
    use serde_json::{Value, json};
    let draft = json!({"id":42,"tag_name":"v1.2.3","draft":true,"prerelease":false,"immutable":false,"assets":[]});
    let parse =
        |value| serde_json::from_value::<ReleaseResponse>(value).expect("actual release response");
    admit_release_lifecycle(
        &parse(draft.clone()),
        "v1.2.3",
        PackageReleaseChannel::Stable,
    )
    .expect("native mutable stable draft");
    for (field, replacement) in [
        ("id", json!(0)),
        ("tag_name", json!("preview")),
        ("draft", Value::Null),
        ("prerelease", json!(true)),
        ("prerelease", Value::Null),
        ("immutable", json!(true)),
        ("immutable", Value::Null),
    ] {
        let mut changed = draft.clone();
        changed[field] = replacement;
        assert!(
            admit_release_lifecycle(&parse(changed), "v1.2.3", PackageReleaseChannel::Stable)
                .is_err(),
            "{field}"
        );
    }
    for missing in ["draft", "prerelease", "immutable"] {
        let mut changed = draft.clone();
        changed.as_object_mut().expect("object").remove(missing);
        assert!(
            admit_release_lifecycle(&parse(changed), "v1.2.3", PackageReleaseChannel::Stable)
                .is_err(),
            "missing {missing}"
        );
    }
    let mut published = draft;
    published["draft"] = json!(false);
    published["immutable"] = json!(true);
    admit_release_lifecycle(
        &parse(published.clone()),
        "v1.2.3",
        PackageReleaseChannel::Stable,
    )
    .expect("immutable stable publication");
    for immutable in [json!(false), Value::Null] {
        published["immutable"] = immutable;
        let response = parse(published.clone());
        admit_release_lifecycle(&response, "v1.2.3", PackageReleaseChannel::Stable)
            .expect("published planner owns immutability verdict");
        assert_ne!(response.immutable, Some(true));
    }

    let preview_draft = json!({"id":43,"tag_name":"preview-abc","draft":true,"prerelease":true,"immutable":false,"assets":[]});
    admit_release_lifecycle(
        &parse(preview_draft.clone()),
        "preview-abc",
        PackageReleaseChannel::Preview,
    )
    .expect("preview remains a prerelease draft");
    let mut invalid_preview = preview_draft;
    invalid_preview["prerelease"] = json!(false);
    assert!(
        admit_release_lifecycle(
            &parse(invalid_preview),
            "preview-abc",
            PackageReleaseChannel::Preview
        )
        .is_err()
    );
}
