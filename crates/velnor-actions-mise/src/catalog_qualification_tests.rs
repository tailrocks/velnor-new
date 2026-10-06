//! Identity mutation and absent publication regression proofs.

use super::{
    DistributionAssetFormat, DistributionHost, DistributionRequirement, DistributionTool,
    ProvisioningMode, QualifiedDistribution,
};

#[test]
fn every_nonversion_field_changes_same_version_qualification_digest() -> Result<(), crate::MiseError>
{
    let original = QualifiedDistribution::qualify_official(
        DistributionTool::Mise,
        DistributionHost::LinuxAmd64,
    )?;
    let mutations: [fn(&mut QualifiedDistribution); 16] = [
        |record| record.selection_version = "2026.10.1",
        |record| record.tool = DistributionTool::Mbx,
        |record| record.host = DistributionHost::LinuxArm64,
        |record| record.selector = "github:owner/mise@2026.10.0",
        |record| {
            record.asset_url = "https://github.com/owner/mise/releases/download/v2026.10.0/mise"
        },
        |record| record.archive_sha256 = "differing-archive",
        |record| record.binary_sha256 = "differing-binary",
        |record| record.asset_format = DistributionAssetFormat::TarGzip,
        |record| record.binary_member = "mise/bin/mise",
        |record| record.source_repository = "https://github.com/owner/mise",
        |record| record.source_commit = "differing-commit",
        |record| record.source_tree = "differing-tree",
        |record| record.owner = "owner/mise",
        |record| record.abi = "differing-abi",
        |record| record.provisioning_mode = ProvisioningMode::MiseNoMisercExclusiveConfig,
        |record| {
            record.installed_binary_relative_path = Some("installs/fixture-only/0.0.1/bin/mise")
        },
    ];
    for mutation in mutations {
        let mut changed = original.clone();
        mutation(&mut changed);
        assert_eq!(original.version(), changed.version());
        assert_ne!(
            original.qualification_digest(),
            changed.qualification_digest()
        );
    }
    Ok(())
}

impl QualifiedDistribution {
    /// Synthetic unit-test fixture; never enters the production record factory.
    pub(crate) fn owned_test_fixture(host: DistributionHost) -> Result<Self, crate::MiseError> {
        Self {
            tool: DistributionTool::Mise,
            host,
            selector: "github:velnor-test-fixtures/mise@0.0.1-fixture",
            asset_url: "https://github.com/velnor-test-fixtures/mise/releases/download/v0.0.1-fixture/mise.tar.gz",
            archive_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            binary_sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            asset_format: DistributionAssetFormat::TarGzip,
            binary_member: "mise/bin/mise",
            source_repository: "https://github.com/velnor-test-fixtures/mise",
            source_commit: "cccccccccccccccccccccccccccccccccccccccc",
            source_tree: "dddddddddddddddddddddddddddddddddddddddd",
            owner: "velnor-test-fixtures/mise",
            version: "0.0.1-fixture",
            selection_version: "0.0.1-fixture",
            abi: "mise-owned-cargo-wrapper-v1",
            provisioning_mode: ProvisioningMode::MiseNoMisercExclusiveConfig,
            installed_binary_relative_path: None,
            launch_entries: &[],
            source_lineage: &[],
            install_plan: None,
        }
        .validate()
    }
}

#[test]
fn installed_path_requires_evidence_and_rejects_noncanonical_aliases()
-> Result<(), crate::MiseError> {
    let original = QualifiedDistribution::owned_test_fixture(DistributionHost::LinuxAmd64)?;
    assert!(original.required_installed_binary_path().is_err());
    for path in [
        "",
        "/tmp/mbx",
        "../mbx",
        "installs/../mbx",
        "installs/./mbx",
        "installs//mbx",
        "installs\\mbx",
        "installs/mbx\0",
    ] {
        let mut changed = original.clone();
        changed.installed_binary_relative_path = Some(path);
        assert!(changed.required_installed_binary_path().is_err());
        assert!(changed.validate().is_err());
    }
    let mut changed = original.clone();
    changed.installed_binary_relative_path = Some("installs/fixture-only/0.0.1/bin/mise");
    assert_eq!(
        changed.required_installed_binary_path()?,
        "installs/fixture-only/0.0.1/bin/mise"
    );
    assert_ne!(
        changed.qualification_digest(),
        original.qualification_digest()
    );
    Ok(())
}

#[test]
fn container_and_member_are_explicit_and_validate_together() -> Result<(), crate::MiseError> {
    let original = QualifiedDistribution::owned_test_fixture(DistributionHost::LinuxAmd64)?;
    for invalid_member in ["", "mbx", "../mise", "/mise/bin/mise"] {
        let mut changed = original.clone();
        changed.binary_member = invalid_member;
        assert!(changed.validate().is_err());
    }
    let mut changed = original.clone();
    changed.asset_format = DistributionAssetFormat::Binary;
    assert!(changed.validate().is_err());
    let standalone = QualifiedDistribution::qualify_official(
        DistributionTool::Mise,
        DistributionHost::LinuxAmd64,
    )?;
    assert_eq!(standalone.asset_format(), DistributionAssetFormat::Binary);
    assert!(standalone.binary_member().is_empty());
    Ok(())
}

#[test]
fn absent_owned_publication_never_falls_back_to_official() {
    for tool in [DistributionTool::Mise, DistributionTool::Mbx] {
        for host in [
            DistributionHost::LinuxAmd64,
            DistributionHost::LinuxArm64,
            DistributionHost::MacosArm64,
        ] {
            assert!(
                QualifiedDistribution::require_for_generator(
                    tool,
                    host,
                    match tool {
                        DistributionTool::Mise => DistributionRequirement::RequiresNoMiserc,
                        _ => DistributionRequirement::MbxTransport,
                    }
                )
                .is_err()
            );
        }
    }
}

#[test]
fn official_mise_has_separate_archive_and_installed_bytes() -> Result<(), crate::MiseError> {
    let distribution = QualifiedDistribution::qualify_official(
        DistributionTool::Mise,
        DistributionHost::MacosArm64,
    )?;
    assert_ne!(distribution.archive_sha256(), distribution.binary_sha256());
    assert_eq!(distribution.provisioning_mode(), ProvisioningMode::Official);
    assert_eq!(
        distribution.source_tree(),
        "172af04ce7a3cbc05920575992459a8e7d92135a"
    );
    Ok(())
}

#[test]
fn owner_version_changes_qualification_digest() -> Result<(), crate::MiseError> {
    let original = QualifiedDistribution::qualify_official(
        DistributionTool::Mise,
        DistributionHost::LinuxAmd64,
    )?;
    let mut changed = original.clone();
    changed.version = "2026.10.1";
    assert_ne!(
        original.qualification_digest(),
        changed.qualification_digest()
    );
    Ok(())
}

#[test]
fn exact_owned_reported_prerelease_is_preserved() -> Result<(), crate::MiseError> {
    for version in [
        "2026.10.0-owned-cargo-wrapper",
        "2026.10.0-owned-cargo-wrapper-DEBUG",
    ] {
        super::validate_distribution_version("owned/mise", version)?;
    }
    for version in [
        "latest",
        "01.2.3",
        "2026.01.03",
        "2026.10",
        "2026.10.0-",
        "2026.10.0-01",
        "2026.10.0-${{bad}}",
    ] {
        assert!(super::validate_distribution_version("owned/mise", version).is_err());
    }
    Ok(())
}
