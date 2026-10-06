use super::*;

#[test]
fn measured_roots_and_primary_launch_are_exact() -> Result<(), MiseError> {
    let catalog = ToolCatalog::pinned();
    for (domain, tool) in [
        (ToolCacheDomain::NpmBootstrap, PinnedTool::Node),
        (ToolCacheDomain::BunBootstrap, PinnedTool::Bun),
        (ToolCacheDomain::TofuBootstrap, PinnedTool::Opentofu),
        (ToolCacheDomain::GradleBootstrap, PinnedTool::Java),
    ] {
        let launch = catalog.native_launch_context(DistributionHost::MacosArm64, domain, tool)?;
        let distribution = catalog.native_distribution(DistributionHost::MacosArm64, tool)?;
        assert_eq!(launch.owned_root(), domain.root());
        assert_eq!(launch.host(), DistributionHost::MacosArm64);
        assert_eq!(launch.tool(), tool);
        assert_eq!(launch.domain(), domain);
        assert_eq!(launch.selector(), distribution.selector());
        assert_eq!(launch.launch_sha256(), distribution.binary_sha256());
        assert_eq!(launch.distribution(), &distribution);
        assert_eq!(
            launch.qualification_digest(),
            distribution.qualification_digest()
        );
        assert_eq!(
            launch.install_relative_root(),
            distribution.required_install_plan()?.root_relative_path()
        );
        assert_eq!(
            launch.launch_relative_path(),
            distribution.required_installed_binary_path()?
        );
        assert_eq!(
            launch.executable(),
            format!(
                "{}/{}",
                domain.root(),
                distribution.required_installed_binary_path()?
            )
        );
        for (key, path) in launch.environment()? {
            assert!(path.starts_with(&launch.install_root()), "{key}: {path}");
        }
    }
    Ok(())
}

#[test]
fn wrong_owner_and_unmeasured_host_fail_closed() {
    let catalog = ToolCatalog::pinned();
    assert!(
        catalog
            .native_launch_context(
                DistributionHost::MacosArm64,
                ToolCacheDomain::GradleBootstrap,
                PinnedTool::Gradle
            )
            .is_err()
    );
    for domain in [
        ToolCacheDomain::Planning,
        ToolCacheDomain::BunBootstrap,
        ToolCacheDomain::TofuBootstrap,
        ToolCacheDomain::GradleBootstrap,
    ] {
        assert!(
            catalog
                .native_launch_context(DistributionHost::MacosArm64, domain, PinnedTool::Node,)
                .is_err()
        );
    }
    for host in [DistributionHost::LinuxAmd64, DistributionHost::LinuxArm64] {
        assert!(
            catalog
                .native_launch_context(host, ToolCacheDomain::NpmBootstrap, PinnedTool::Node,)
                .is_err()
        );
    }
    assert!(
        catalog
            .native_launch_context(
                DistributionHost::MacosArm64,
                ToolCacheDomain::Full,
                PinnedTool::Rust,
            )
            .is_err()
    );
}

#[test]
fn altered_catalog_pin_cannot_retain_launch_authority() -> Result<(), MiseError> {
    let pinned = ToolCatalog::pinned();
    let altered = ToolCatalog::new(
        pinned.version(PinnedTool::Rust),
        pinned.version(PinnedTool::MrBoxington),
        pinned.version(PinnedTool::Gh),
        pinned.version(PinnedTool::Actionlint),
        pinned.version(PinnedTool::Shellcheck),
        pinned.version(PinnedTool::Zizmor),
        pinned.version(PinnedTool::Nextest),
        "1.0.0",
    )?;
    assert!(
        altered
            .native_launch_context(
                DistributionHost::MacosArm64,
                ToolCacheDomain::TofuBootstrap,
                PinnedTool::Opentofu,
            )
            .is_err()
    );
    Ok(())
}

#[test]
fn compiler_free_environment_rejects_empty_and_compiler_selection() {
    let catalog = ToolCatalog::pinned();
    for tools in [&[][..], &[PinnedTool::Rust][..]] {
        assert!(
            qualified_native_execution_environment(
                &catalog,
                DistributionHost::MacosArm64,
                ToolCacheDomain::Full,
                tools,
            )
            .is_err()
        );
    }
}

#[test]
fn native_environment_merge_is_canonical_and_rejects_conflicting_homes() -> Result<(), MiseError> {
    let catalog = ToolCatalog::pinned();
    let host = DistributionHost::MacosArm64;
    let domain = ToolCacheDomain::Full;
    let mut first = BTreeMap::new();
    let mut second = BTreeMap::new();
    merge_native_environment(
        &catalog,
        host,
        domain,
        &[PinnedTool::Node, PinnedTool::Java],
        &mut first,
    )?;
    merge_native_environment(
        &catalog,
        host,
        domain,
        &[PinnedTool::Java, PinnedTool::Node, PinnedTool::Java],
        &mut second,
    )?;
    assert_eq!(first, second);
    assert!(
        first
            .get("PATH")
            .is_some_and(|path| path.ends_with("/usr/bin:/bin:/usr/sbin:/sbin"))
    );
    let java = catalog.native_launch_context(host, domain, PinnedTool::Java)?;
    for (key, value) in java.environment()? {
        if key != "PATH" {
            assert_eq!(first.get(&key), Some(&value));
            first.insert(key, "counterfeit-home".to_owned());
        }
    }
    assert!(
        merge_native_environment(&catalog, host, domain, &[PinnedTool::Java], &mut first).is_err()
    );
    Ok(())
}

#[test]
fn github_launch_belongs_only_to_measured_planning_domain() -> Result<(), MiseError> {
    let catalog = ToolCatalog::pinned();
    let host = DistributionHost::MacosArm64;
    let launch = catalog.native_launch_context(host, ToolCacheDomain::Planning, PinnedTool::Gh)?;
    let record = catalog.native_distribution(host, PinnedTool::Gh)?;
    assert_eq!(launch.owned_root(), ToolCacheDomain::Planning.root());
    assert_eq!(launch.distribution(), &record);
    assert_eq!(
        launch.launch_relative_path(),
        record.required_installed_binary_path()?
    );
    for domain in [
        ToolCacheDomain::Full,
        ToolCacheDomain::NpmBootstrap,
        ToolCacheDomain::BunBootstrap,
        ToolCacheDomain::TofuBootstrap,
        ToolCacheDomain::GradleBootstrap,
    ] {
        assert!(
            catalog
                .native_launch_context(host, domain, PinnedTool::Gh)
                .is_err()
        );
    }
    for host in [DistributionHost::LinuxAmd64, DistributionHost::LinuxArm64] {
        assert!(
            catalog
                .native_launch_context(host, ToolCacheDomain::Planning, PinnedTool::Gh)
                .is_err()
        );
    }
    assert!(
        qualified_native_execution_environment(
            &catalog,
            DistributionHost::MacosArm64,
            ToolCacheDomain::Full,
            &[PinnedTool::Python, PinnedTool::Gh],
        )
        .is_err()
    );
    Ok(())
}
