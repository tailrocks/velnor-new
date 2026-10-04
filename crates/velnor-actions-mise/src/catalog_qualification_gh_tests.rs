//! Measured native GH admission is host- and recipe-specific.

use super::super::{PinnedTool, ToolCatalog};
use super::*;

#[test]
fn macos_gh_has_its_measured_http_plan() -> Result<(), crate::MiseError> {
    let gh = QualifiedDistribution::require_native(
        DistributionTool::Gh,
        DistributionHost::MacosArm64,
        "2.102.0",
    )?;
    assert_eq!(gh.asset_format(), DistributionAssetFormat::Zip);
    assert_eq!(
        gh.required_installed_binary_path()?,
        "installs/http-gh/2.102.0/bin/gh"
    );
    assert_eq!(
        gh.required_install_plan()?.root_relative_path(),
        "installs/http-gh/2.102.0"
    );
    assert!(gh.selector().starts_with("http:gh["));
    assert_eq!(
        ToolCatalog::pinned()
            .native_distribution(DistributionHost::MacosArm64, PinnedTool::Gh)?
            .qualification_digest(),
        gh.qualification_digest()
    );
    assert!(
        QualifiedDistribution::require_native(
            DistributionTool::Gh,
            DistributionHost::MacosArm64,
            "2.101.0"
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn linux_gh_archive_audit_does_not_admit_native_installs() {
    for host in [DistributionHost::LinuxAmd64, DistributionHost::LinuxArm64] {
        assert!(
            QualifiedDistribution::qualify_native(DistributionTool::Gh, host, "2.102.0").is_ok()
        );
        assert!(
            QualifiedDistribution::require_native(DistributionTool::Gh, host, "2.102.0").is_err()
        );
        assert!(
            ToolCatalog::pinned()
                .native_distribution(host, PinnedTool::Gh)
                .is_err()
        );
    }
}
