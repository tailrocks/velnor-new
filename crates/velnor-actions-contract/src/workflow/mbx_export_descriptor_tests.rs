use super::*;

fn owner() -> MbxOwnerIdentity {
    MbxOwnerIdentity {
        version: "1.21.1-owned-cache-transport".to_owned(),
        binary_sha256: "a".repeat(64),
        qualification_identity: "b".repeat(64),
        source_sha: "c".repeat(40),
    }
}

fn descriptor() -> MbxExportDescriptor {
    MbxExportDescriptor {
        domain: MbxCacheDomain::Validation,
        producer_job_id: "rust-demo".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        target: "x86_64-unknown-linux-gnu".to_owned(),
        workspace_roots: vec![".".to_owned()],
        configuration_digest: crate::digest_b3(b"configuration"),
        task_digests: BTreeMap::from([(
            "stack/rust/root/clippy/default".to_owned(),
            crate::digest_b3(b"clippy"),
        )]),
        owner: owner(),
        action_sha: "d".repeat(40),
    }
}

#[test]
fn owned_reported_version_preserves_exact_prerelease_identity() {
    let owned = owner();
    owned.validate().expect("owned reported version syntax");
    let mut alias = owned.clone();
    alias.version = "1.21.1".to_owned();
    alias.validate().expect("official version is valid syntax");
    assert_ne!(alias, owned, "syntax does not qualify an official alias");
    let original = descriptor();
    let mut changed = original.clone();
    changed.owner = alias;
    assert_ne!(
        original.identity().expect("owned identity"),
        changed.identity().expect("alias identity")
    );
}

#[test]
fn malformed_owned_version_and_floating_selectors_are_rejected() {
    for version in [
        "01.21.1-owned-cache-transport",
        "1.021.1-owned-cache-transport",
        "1.21.1-",
        "1.21.1-owned..transport",
        "1.21.1-01",
        "1.21.1+",
        "latest",
        "1.21",
        "1.21.18446744073709551616-owned",
    ] {
        let mut actual = owner();
        actual.version = version.to_owned();
        assert!(actual.validate().is_err(), "{version}");
    }
}

#[test]
fn transport_roles_cannot_escape_closed_job_and_path_boundaries() {
    descriptor().validate().expect("validation cohort");
    for job in ["plan", "required", "-job", "1job", "workload-node"] {
        let mut actual = descriptor();
        actual.producer_job_id = job.to_owned();
        assert!(actual.validate().is_err(), "{job}");
    }
    for root in [
        "../secret",
        "a/../secret",
        "/tmp/secret",
        "a//b",
        "a\\b",
        "${{ env.ROOT }}",
    ] {
        let mut actual = descriptor();
        actual.workspace_roots = vec![root.to_owned()];
        assert!(actual.validate().is_err(), "{root}");
    }
    let mut helper = descriptor();
    helper.domain = MbxCacheDomain::Helper;
    assert!(helper.validate().is_err());
    helper.producer_job_id = "plan".to_owned();
    helper.task_digests.clear();
    helper.validate().expect("separate helper cohort");
    assert_ne!(
        helper.cache_prefix().expect("helper namespace"),
        descriptor().cache_prefix().expect("validation namespace")
    );
}

#[test]
fn actual_descriptor_group_fits_the_native_and_action_transport_limit() {
    let group = descriptor().export_group().expect("exact descriptor group");
    for (run, attempt, expected) in [
        (
            "37012391691",
            "1",
            "velnor-mbx-b3-554e69d4f330d0f2fd13e6860553d49303071f172e3b686aadfab7ec3d67ec57-r37012391691-a1",
        ),
        (
            "18446744073709551615",
            "18446744073709551615",
            "velnor-mbx-b3-554e69d4f330d0f2fd13e6860553d49303071f172e3b686aadfab7ec3d67ec57-r18446744073709551615-a18446744073709551615",
        ),
    ] {
        let expanded = group
            .replace("${{ github.run_id }}", run)
            .replace("${{ github.run_attempt }}", attempt);
        assert_eq!(expanded, expected);
        assert!(expanded.len() <= 256);
        assert!(expanded.bytes().all(|byte| byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(byte, b'.' | b'_' | b'-')));
    }
}
