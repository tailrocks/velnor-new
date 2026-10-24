use velnor_actions_contract::{
    ActionPin, ContractError, GeneratorBinary, GeneratorLock, LockedGenerator, MiseBootstrap,
    ReleaseManifest, SUPPORTED_TARGETS, TargetRecord, asset_filename,
};

#[test]
fn manifest_schemas_validate_and_lookup_targets() -> Result<(), ContractError> {
    let sha = "ab".repeat(32);
    let manifest = ReleaseManifest {
        schema: 1,
        version: "0.1.0".to_owned(),
        repository: "tailrocks/velnor-new".to_owned(),
        commit: "ab".repeat(20),
        targets: SUPPORTED_TARGETS
            .iter()
            .map(|target| TargetRecord {
                target: (*target).to_owned(),
                artifact: format!(
                    "https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/{}",
                    asset_filename("0.1.0", target)
                ),
                sha256: sha.clone(),
            })
            .collect(),
    };
    manifest.validate("release.toml")?;
    assert!(
        manifest
            .record_for_target("x86_64-unknown-linux-gnu")
            .is_some()
    );
    assert!(manifest.record_for_target("other").is_none());
    let lock = GeneratorLock {
        schema: 1,
        generator: LockedGenerator {
            binary: "velnor-actions".to_owned(),
            version: "0.1.0".to_owned(),
            commit: "ab".repeat(20),
            binaries: vec![GeneratorBinary {
                target: "x86_64-unknown-linux-gnu".to_owned(),
                artifact: "https://example.com/velnor-actions-0.1.0".to_owned(),
                sha256: sha,
            }],
        },
        mise_bootstrap: MiseBootstrap {
            version: "2025.1.0".to_owned(),
            artifact: "https://example.com/mise-2025.1.0".to_owned(),
            sha256: "cd".repeat(32),
        },
        actions: vec![ActionPin {
            name: "actions/checkout".to_owned(),
            version: "v4.2.2".to_owned(),
            sha: "ab".repeat(20),
            reviewed: "2026-01-15".to_owned(),
        }],
    };
    lock.validate("generator.lock")?;
    assert!(lock.binary_for_target("x86_64-unknown-linux-gnu").is_some());
    let mut bad = lock.clone();
    bad.actions[0].sha = "xyz".to_owned();
    assert!(bad.validate("generator.lock").is_err());
    for commit in [
        String::new(),
        "xyz".to_owned(),
        "A".repeat(40),
        "c".repeat(39),
    ] {
        let mut bad = lock.clone();
        bad.generator.commit = commit;
        let err = bad
            .validate("generator.lock")
            .expect_err("lock commit shares manifest strictness");
        assert!(err.to_string().contains("malformed_commit"), "{err}");
    }
    Ok(())
}
