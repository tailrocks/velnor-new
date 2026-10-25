use super::{
    BTreeMap, CheckDeadline, HOMEBREW_SOURCE_CONFIG_PATH, HOMEBREW_SOURCE_RELEASE_POLICY_PATH,
    HomebrewSourceAuthorityLocator, HomebrewTapConfig, OrchestratorError, PackageReleaseChannel,
    PackageReleasePayloadFormat, PackageReleasePlan, PackageReleasePolicy,
    RustBinaryReleaseArchive, SOURCE_DOCUMENT_LIMIT, internal,
};

fn read_source(
    locator: &HomebrewSourceAuthorityLocator,
    source_sha: &str,
    path: &str,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    crate::package_release_publish::read_protected_source_file(
        &locator.repository,
        locator.repository_id,
        &locator.default_branch,
        source_sha,
        path,
        SOURCE_DOCUMENT_LIMIT,
        deadline,
    )
}

fn resolve_source_plan(
    locator: &HomebrewSourceAuthorityLocator,
    source_sha: &str,
    policy: &PackageReleasePolicy,
    deadline: CheckDeadline,
) -> Result<PackageReleasePlan, OrchestratorError> {
    if policy.repository != locator.repository
        || policy.default_branch != locator.default_branch
        || policy.channel != PackageReleaseChannel::Preview
    {
        return Err(internal("homebrew_authority_policy_identity"));
    }
    let mut manifests = BTreeMap::new();
    for path in policy.manifest_paths() {
        manifests.insert(
            path.to_owned(),
            read_source(locator, source_sha, path, deadline)?,
        );
    }
    velnor_actions_rust::resolve_release_policy(policy, &manifests)
        .map_err(|_| internal("homebrew_authority_plan_resolution"))
}

fn validate_source_projection(
    expected_tap_repository: &str,
    locator: &HomebrewSourceAuthorityLocator,
    config: &velnor_actions_contract::config::VelnorConfig,
    policy: &PackageReleasePolicy,
    plan: &PackageReleasePlan,
) -> Result<
    (
        HomebrewTapConfig,
        velnor_actions_contract::homebrew_spec::HomebrewFormulaSpec,
    ),
    OrchestratorError,
> {
    let rust = config
        .stacks
        .rust
        .as_ref()
        .ok_or_else(|| internal("homebrew_authority_rust_release_missing"))?;
    let release = &rust.binary_release;
    let tap = config
        .distribution
        .homebrew_tap
        .clone()
        .filter(|tap| tap.enabled && tap.repository == expected_tap_repository)
        .ok_or_else(|| internal("homebrew_authority_tap_binding"))?;
    let spec = config
        .distribution
        .homebrew_formula
        .clone()
        .ok_or_else(|| internal("homebrew_authority_formula_spec_missing"))?;
    if !release.preview.enabled
        || release.version_package != policy.version.version_package
        || release.manifest_path.is_empty()
        || !policy
            .version
            .bindings
            .iter()
            .all(|binding| binding.workspace_manifest_path == release.manifest_path)
    {
        return Err(internal("homebrew_authority_release_config_mismatch"));
    }

    let mut products = release
        .products
        .iter()
        .map(|product| {
            let mut bins = product.bins.clone();
            let mut targets = product.targets.clone();
            bins.sort();
            targets.sort();
            velnor_actions_contract::PackageReleasePlanProduct {
                id: product.id.clone(),
                package: product.package.clone(),
                bins,
                targets,
                format: match product.archive {
                    RustBinaryReleaseArchive::TarGz => PackageReleasePayloadFormat::TarGz,
                },
            }
        })
        .collect::<Vec<_>>();
    products.sort_by(|left, right| left.id.cmp(&right.id));
    let mut companions = release.companions.clone();
    companions.sort_by(|left, right| left.id.cmp(&right.id));
    if products != policy.products || companions != policy.companions {
        return Err(internal("homebrew_authority_release_inventory_mismatch"));
    }
    if plan.repository != locator.repository || plan.default_branch != locator.default_branch {
        return Err(internal("homebrew_authority_resolved_plan_identity"));
    }
    velnor_actions_contract::homebrew_spec::validate_homebrew_projection(&spec, plan)
        .map_err(|_| internal("homebrew_authority_formula_projection"))?;
    Ok((tap, spec))
}

pub(super) struct SourceProjection {
    pub(super) source_tip: String,
    pub(super) source_config: velnor_actions_contract::config::VelnorConfig,
    pub(super) plan: PackageReleasePlan,
    pub(super) tap: HomebrewTapConfig,
    pub(super) spec: velnor_actions_contract::homebrew_spec::HomebrewFormulaSpec,
}

pub(super) fn load_source_projection(
    locator: &HomebrewSourceAuthorityLocator,
    expected_tap_repository: &str,
    deadline: CheckDeadline,
) -> Result<SourceProjection, OrchestratorError> {
    let source_tip = crate::package_release_publish::read_public_branch_tip(
        &locator.repository,
        locator.repository_id,
        &locator.default_branch,
        deadline,
    )?;
    let verified_source_tip = crate::package_release_publish::verify_public_protected_main_lineage(
        &locator.repository,
        locator.repository_id,
        &locator.default_branch,
        &source_tip,
        &source_tip,
        deadline,
    )?;
    if verified_source_tip != source_tip {
        return Err(internal("homebrew_authority_source_tip_mismatch"));
    }
    let source_config_bytes =
        read_source(locator, &source_tip, HOMEBREW_SOURCE_CONFIG_PATH, deadline)?;
    let source_config_text = std::str::from_utf8(&source_config_bytes)
        .map_err(|_| internal("homebrew_authority_source_config_utf8"))?;
    let source_config = crate::config::load_config_from_text(source_config_text)
        .map_err(|_| internal("homebrew_authority_source_config_invalid"))?;
    let policy_bytes = read_source(
        locator,
        &source_tip,
        HOMEBREW_SOURCE_RELEASE_POLICY_PATH,
        deadline,
    )?;
    let policy = velnor_actions_contract::parse_package_release_policy(&policy_bytes)
        .map_err(|_| internal("homebrew_authority_release_policy_invalid"))?;
    let plan = resolve_source_plan(locator, &source_tip, &policy, deadline)?;
    let (tap, spec) = validate_source_projection(
        expected_tap_repository,
        locator,
        &source_config,
        &policy,
        &plan,
    )?;

    let final_source_tip = crate::package_release_publish::verify_public_protected_main_lineage(
        &locator.repository,
        locator.repository_id,
        &locator.default_branch,
        &source_tip,
        &source_tip,
        deadline,
    )?;
    if final_source_tip != source_tip {
        return Err(internal("homebrew_authority_source_tip_changed"));
    }
    Ok(SourceProjection {
        source_tip,
        source_config,
        plan,
        tap,
        spec,
    })
}
