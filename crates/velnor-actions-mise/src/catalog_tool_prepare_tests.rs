use super::*;

#[test]
fn selector_validation_closes_domain_and_order() {
    let catalog = ToolCatalog::pinned();
    let gh = catalog.tool_spec(PinnedTool::Gh).expect("generic Gh");
    let node = catalog
        .native_tool_spec(DistributionHost::MacosArm64, PinnedTool::Node)
        .expect("qualified Node");
    assert!(
        validate(
            &catalog,
            ToolCacheDomain::Planning,
            DistributionHost::MacosArm64,
            &[gh.clone()]
        )
        .is_ok()
    );
    assert!(
        validate(
            &catalog,
            ToolCacheDomain::Planning,
            DistributionHost::MacosArm64,
            &[node.clone()]
        )
        .is_err()
    );
    assert!(
        validate(
            &catalog,
            ToolCacheDomain::NpmBootstrap,
            DistributionHost::MacosArm64,
            &[node]
        )
        .is_ok()
    );
    assert!(
        validate(
            &catalog,
            ToolCacheDomain::Planning,
            DistributionHost::MacosArm64,
            &[gh.clone(), gh]
        )
        .is_err()
    );
    assert!(
        validate(
            &catalog,
            ToolCacheDomain::Full,
            DistributionHost::MacosArm64,
            &[]
        )
        .is_err()
    );
    assert!(
        validate(
            &catalog,
            ToolCacheDomain::Full,
            DistributionHost::MacosArm64,
            &["node@latest".to_owned()]
        )
        .is_err()
    );
}

#[test]
fn pure_source_binds_every_closed_root_and_verifies_after_install() {
    let mise = QualifiedDistribution::qualify_official(
        DistributionTool::Mise,
        DistributionHost::LinuxAmd64,
    )
    .expect("official qualification for source shape only");
    let catalog = ToolCatalog::pinned();
    let record = compiled(
        &catalog,
        ToolCacheDomain::Planning,
        &[catalog.tool_spec(PinnedTool::Gh).expect("generic Gh")],
        &[PinnedTool::Gh],
        "1.0.0",
        &mise,
    )
    .expect("compiled fixture");
    let source = record.source();
    assert!(source.contains("$temp/velnor/npm-source/mise"));
    assert!(source.contains("$temp/velnor/mise"));
    assert_eq!(
        record
            .environment()
            .get("VELNOR_MISE_SHA256")
            .map(String::as_str),
        Some(mise.binary_sha256())
    );
    assert!(source.contains("sha256sum -c -"));
    assert!(
        source
            .find("cold_prepare(sys.argv[1], sys.argv[2])")
            .expect("cold admission")
            < source.find("export MISE_CONFIG_DIR").expect("fresh config")
    );
    assert!(
        source.find("export MISE_CONFIG_DIR").expect("fresh config")
            < source
                .find("\"install\"")
                .expect("first installer execution")
    );
    assert!(
        source.find("\"install\"").expect("install")
            < source.find("for selector, tool").expect("verify")
    );
    assert!(
        source.find("for selector, tool").expect("verify")
            < source.find("verified=true").expect("publish")
    );
    assert!(!source.contains("GITHUB_WORKSPACE"));
    assert_eq!(
        record
            .environment()
            .get("MISE_DATA_DIR")
            .map(String::as_str),
        Some(ToolCacheDomain::Planning.root())
    );
}

#[test]
fn public_factory_never_substitutes_official_for_missing_owned_qualification() {
    let result = helper_for_tools(
        &ToolCatalog::pinned(),
        ToolCacheDomain::Planning,
        DistributionHost::LinuxAmd64,
        &[ToolCatalog::pinned()
            .tool_spec(PinnedTool::Gh)
            .expect("generic Gh")],
        "1.0.0",
    );
    assert!(result.is_err());
}

#[test]
fn actual_host_binds_invocation_environment_without_source_path_conflicts() {
    let catalog = ToolCatalog::pinned();
    let selectors = vec![catalog.tool_spec(PinnedTool::Gh).expect("generic Gh")];
    let records: Vec<_> = [
        DistributionHost::LinuxAmd64,
        DistributionHost::LinuxArm64,
        DistributionHost::MacosArm64,
    ]
    .into_iter()
    .map(|host| {
        let mise = QualifiedDistribution::qualify_official(DistributionTool::Mise, host)
            .expect("official source-shape fixture");
        let record = compiled(
            &catalog,
            ToolCacheDomain::Planning,
            &selectors,
            &[PinnedTool::Gh],
            "1.0.0",
            &mise,
        )
        .expect("host-bound fixture");
        assert_eq!(record.invocation().args()[1], host.abi());
        assert_eq!(
            record
                .environment()
                .get("VELNOR_MISE_SHA256")
                .map(String::as_str),
            Some(mise.binary_sha256())
        );
        record
    })
    .collect();
    assert!(
        records
            .windows(2)
            .all(|pair| pair[0].source() == pair[1].source()
                && pair[0].environment() != pair[1].environment())
    );
    assert!(host_from_argument("x86_64-apple-darwin").is_err());
}

#[test]
fn compiler_host_must_match_actual_runner_host() {
    let catalog = ToolCatalog::pinned();
    let selectors = vec![
        catalog
            .tool_spec(catalog.compiler_tool())
            .expect("compiler"),
    ];
    assert!(
        helper_for_tools(
            &catalog,
            ToolCacheDomain::Full,
            DistributionHost::LinuxArm64,
            &selectors,
            "1.0.0"
        )
        .is_err()
    );
    assert!(
        helper_for_tools(
            &catalog,
            ToolCacheDomain::Full,
            DistributionHost::MacosArm64,
            &selectors,
            "1.0.0"
        )
        .is_err()
    );
}

#[test]
fn selected_native_selector_rejects_host_and_artifact_substitution() {
    let catalog = ToolCatalog::pinned();
    let host = DistributionHost::MacosArm64;
    let selector = catalog
        .native_tool_spec(host, PinnedTool::Opentofu)
        .expect("qualified tofu");
    assert!(
        validate(
            &catalog,
            ToolCacheDomain::TofuBootstrap,
            host,
            &[selector.clone()]
        )
        .is_ok()
    );
    assert!(
        validate(
            &catalog,
            ToolCacheDomain::TofuBootstrap,
            DistributionHost::LinuxAmd64,
            &[selector.clone()]
        )
        .is_err()
    );
    assert!(
        validate(
            &catalog,
            ToolCacheDomain::TofuBootstrap,
            host,
            &[selector.replace("checksum=", "foreign_checksum=")]
        )
        .is_err()
    );
    assert!(
        validate(
            &catalog,
            ToolCacheDomain::TofuBootstrap,
            host,
            &[format!(
                "opentofu@{}",
                catalog.version(PinnedTool::Opentofu)
            )]
        )
        .is_err()
    );
}

#[test]
fn universal_source_binds_distinct_selected_native_configurations() {
    let catalog = ToolCatalog::pinned();
    let mut records = Vec::new();
    for (host, tool) in [
        (DistributionHost::MacosArm64, PinnedTool::Opentofu),
        (DistributionHost::LinuxAmd64, PinnedTool::Opentofu),
        (DistributionHost::MacosArm64, PinnedTool::Java),
    ] {
        let mise = QualifiedDistribution::qualify_official(DistributionTool::Mise, host)
            .expect("source-shape manager fixture");
        let native = catalog
            .native_distribution(host, tool)
            .expect("qualified native");
        let record = compiled(
            &catalog,
            ToolCacheDomain::Full,
            &[native.selector().to_owned()],
            &[tool],
            "1.0.0",
            &mise,
        )
        .expect("selected native record");
        let config = configuration::text(&catalog, host, &[tool]).expect("canonical config");
        assert!(config.contains(native.selector()));
        assert!(config.contains(native.archive_sha256()));
        assert!(config.contains(native.binary_sha256()));
        assert!(config.contains(native.version()));
        assert!(!record.source().contains(native.asset_url()));
        records.push(record);
    }
    assert!(
        records
            .windows(2)
            .all(|pair| pair[0].source() == pair[1].source()
                && pair[0].invocation() != pair[1].invocation())
    );
    let source = records[0].source();
    let install = source
        .find("subprocess.run([mise, *FLAGS, \"install\"")
        .expect("install");
    let verify = source
        .find("    verify_launches(root, tools)")
        .expect("closure verification");
    let execute = source
        .find("result = subprocess.run")
        .expect("first native probe");
    assert!(install < verify && verify < execute);
}

#[test]
fn canonical_configuration_rejects_field_delimiters() {
    let catalog = ToolCatalog::pinned();
    let config = configuration::text(&catalog, DistributionHost::LinuxAmd64, &[PinnedTool::Gh])
        .expect("generic configuration needs no native record");
    assert_eq!(config.lines().count(), 2);
    assert!(config.starts_with("velnor-tool-prepare-v1\ntool\tgh@"));
}

#[test]
fn owner_reconstruction_rejects_mutated_configuration() {
    let catalog = ToolCatalog::pinned();
    let mise = QualifiedDistribution::qualify_official(
        DistributionTool::Mise,
        DistributionHost::LinuxAmd64,
    )
    .expect("source-shape fixture");
    let expected = compiled(
        &catalog,
        ToolCacheDomain::Planning,
        &[catalog.tool_spec(PinnedTool::Gh).expect("generic Gh")],
        &[PinnedTool::Gh],
        "1.0.0",
        &mise,
    )
    .expect("owner fixture");
    assert!(
        verify_reconstruction(
            expected.clone(),
            expected.invocation(),
            expected.environment()
        )
        .is_ok()
    );
    let mut args = expected.invocation().args().to_vec();
    args[2].replace_range(..2, "00");
    let altered = HelperInvocation::compiled(
        expected.invocation().descriptor().clone(),
        args,
        expected.invocation().installed_selectors().to_vec(),
    )
    .expect("well-shaped mutated invocation");
    assert!(verify_reconstruction(expected.clone(), &altered, expected.environment()).is_err());
}

#[test]
fn compiler_free_preparation_binds_homes_only_to_full_payload() -> Result<(), MiseError> {
    let catalog = ToolCatalog::pinned();
    let host = DistributionHost::MacosArm64;
    let mise = QualifiedDistribution::qualify_official(DistributionTool::Mise, host)?;
    for (domain, tool) in [
        (ToolCacheDomain::Full, PinnedTool::Python),
        (ToolCacheDomain::Planning, PinnedTool::Gh),
    ] {
        let selector = if tool == PinnedTool::Gh {
            catalog.tool_spec(tool)?
        } else {
            catalog.native_tool_spec(host, tool)?
        };
        // Official qualification is source-shape evidence, never owned execution authority.
        let helper = compiled(
            &catalog,
            domain,
            &[selector.clone()],
            &[tool],
            "1.0.0",
            &mise,
        )?;
        for (key, value) in ToolCacheDomain::Full.home_environment() {
            assert_eq!(
                helper.environment().get(&key),
                (domain == ToolCacheDomain::Full).then_some(&value),
                "{key}",
            );
        }
        assert!(!helper.environment().contains_key("RUSTUP_TOOLCHAIN"));
        assert_eq!(helper.invocation().installed_selectors(), &[selector]);
    }
    Ok(())
}
