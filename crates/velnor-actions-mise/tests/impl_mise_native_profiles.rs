//! Provider selection and consumer wrapper separation.

use velnor_actions_mise::{
    PinnedTool, ToolCatalog,
    catalog::{
        GRADLE_WRAPPER_VERSION,
        native_profiles::{JavaProvider, NativeToolProfile},
        qualification::{DistributionHost, DistributionTool, QualifiedDistribution},
    },
};

#[test]
fn community_profile_uses_the_complete_installed_record() {
    let profile = NativeToolProfile::Java(JavaProvider::GraalvmCommunity);
    let record = profile
        .distribution(DistributionHost::MacosArm64)
        .expect("qualified Mac Community JDK");
    assert_eq!(record.tool(), DistributionTool::Java);
    assert_eq!(record.selection_version(), profile.selection_version());
    assert_eq!(
        profile.selector(DistributionHost::MacosArm64),
        Ok(record.selector())
    );
    let catalog = ToolCatalog::pinned();
    assert!(catalog.tool_spec(PinnedTool::Java).is_err());
    assert_eq!(
        catalog.native_tool_spec(DistributionHost::MacosArm64, PinnedTool::Java),
        Ok(record.selector().to_owned())
    );
    assert!(record.selector().starts_with("http:graalvm-community-jdk["));
    assert!(record.required_install_plan().is_ok());
    assert_eq!(
        ToolCatalog::pinned().tool_identity(PinnedTool::Java).source,
        format!(
            "{}/tree/{}",
            record.source_repository(),
            record.source_commit()
        )
    );
}

#[test]
fn audited_archives_cannot_authorize_unmeasured_linux_installs() {
    let profile = NativeToolProfile::Java(JavaProvider::GraalvmCommunity);
    for host in [DistributionHost::LinuxAmd64, DistributionHost::LinuxArm64] {
        assert!(
            QualifiedDistribution::qualify_native(
                DistributionTool::Java,
                host,
                profile.selection_version()
            )
            .is_ok()
        );
        assert!(profile.distribution(host).is_err());
        assert!(profile.selector(host).is_err());
        assert!(
            ToolCatalog::pinned()
                .native_tool_spec(host, PinnedTool::Java)
                .is_err()
        );
    }
}

#[test]
fn managed_gradle_has_no_consumer_wrapper_authority() {
    let profile = NativeToolProfile::ManagedGradle;
    let record = profile
        .distribution(DistributionHost::MacosArm64)
        .expect("qualified Mac managed Gradle");
    assert_eq!(record.tool(), DistributionTool::Gradle);
    assert_eq!(record.selection_version(), profile.selection_version());
    assert_eq!(
        profile.selector(DistributionHost::MacosArm64),
        Ok(record.selector())
    );
    assert!(ToolCatalog::pinned().tool_spec(PinnedTool::Gradle).is_err());
    assert_ne!(profile.selection_version(), GRADLE_WRAPPER_VERSION);
    assert_eq!(GRADLE_WRAPPER_VERSION, "9.5.1");
    assert_eq!(
        NativeToolProfile::for_tool(PinnedTool::Java),
        Some(NativeToolProfile::Java(JavaProvider::GraalvmCommunity))
    );
    assert_eq!(NativeToolProfile::for_tool(PinnedTool::Rust), None);
}
