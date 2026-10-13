//! Helper provisioning: staged-helper gate plus provenance typing.
use std::collections::BTreeMap;
use velnor_actions_contract_workflow::StepKind;
use velnor_actions_workflow_jobs::{HelperProvenance, provision_acquire_step};
use velnor_actions_workflow_steps::{
    ACQUIRE_NAME, ASSET_SHA_ENV, ASSET_URL_ENV, RELEASE_COMMIT_ENV, RenderError,
    STAGED_BINARY_PREFIX, acquire_velnor_step,
};

use super::impl_jobs_fixtures::*;

#[test]
fn provenance_release_ok_seed_fails() -> Result<(), RenderError> {
    let staged = format!("{STAGED_BINARY_PREFIX}{VERSION}");
    let argv = vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!(
            "curl \"$VELNOR_ASSET_URL\" -o {staged} && echo \"$VELNOR_ASSET_SHA256\" | sha256sum -c -"
        ),
    ];
    let provenance = HelperProvenance::ReleaseAsset {
        url: "https://example.invalid/r".to_owned(),
        sha256: "c".repeat(64),
        commit: "c".repeat(40),
    };
    let step = provision_acquire_step(&provenance, argv)?;
    assert_eq!(step.name, ACQUIRE_NAME);
    let err = provision_acquire_step(&HelperProvenance::SeedRequired, Vec::new())
        .expect_err("seed must fail closed");
    assert!(format!("{err:?}").contains("seed_required"), "{err:?}");
    Ok(())
}

#[test]
fn provenance_commit_records_and_validates() -> Result<(), RenderError> {
    let staged = format!("{STAGED_BINARY_PREFIX}{VERSION}");
    let argv = || {
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!(
                "curl \"$VELNOR_ASSET_URL\" -o {staged} && echo \"$VELNOR_ASSET_SHA256\" | sha256sum -c -"
            ),
        ]
    };
    let commit = "d".repeat(40);
    let provenance = HelperProvenance::ReleaseAsset {
        url: "https://example.invalid/r".to_owned(),
        sha256: "c".repeat(64),
        commit: commit.clone(),
    };
    let step = provision_acquire_step(&provenance, argv())?;
    let StepKind::Shell { env, .. } = &step.kind else {
        panic!("acquire must be a shell step");
    };
    assert_eq!(
        env.get(RELEASE_COMMIT_ENV).map(String::as_str),
        Some(commit.as_str())
    );
    for bad in [
        String::new(),
        "xyz".to_owned(),
        "A".repeat(40),
        "c".repeat(39),
    ] {
        let provenance = HelperProvenance::ReleaseAsset {
            url: "https://example.invalid/r".to_owned(),
            sha256: "c".repeat(64),
            commit: bad.clone(),
        };
        assert!(
            provision_acquire_step(&provenance, argv())
                .is_err_and(|err| format!("{err:?}").contains("bad_release_commit")),
            "malformed commit must fail: {bad:?}"
        );
    }
    Ok(())
}

#[test]
fn acquire_requires_verify_wiring() {
    let staged = format!("{STAGED_BINARY_PREFIX}{VERSION}");
    let env = BTreeMap::from([
        (ASSET_SHA_ENV.to_owned(), "a".repeat(64)),
        (
            ASSET_URL_ENV.to_owned(),
            "https://example.invalid/bin".to_owned(),
        ),
        (RELEASE_COMMIT_ENV.to_owned(), "b".repeat(40)),
    ]);
    let bare = vec!["fetch".to_owned(), staged];
    assert!(
        acquire_velnor_step(bare, &env)
            .is_err_and(|err| { format!("{err:?}").contains("acquire_without_verify") })
    );
}
