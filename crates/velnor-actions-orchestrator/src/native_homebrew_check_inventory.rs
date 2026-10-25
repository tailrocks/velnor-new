use super::{BTreeMap, CheckPlatform, NativeExpectedInstallFile, PathBuf};

pub(crate) fn expected_install_files(
    spec: &velnor_actions_contract::homebrew_spec::HomebrewFormulaSpec,
    projection: &velnor_actions_contract::homebrew_spec::ValidatedHomebrewProjection,
    platform: CheckPlatform,
) -> Result<Vec<NativeExpectedInstallFile>, crate::OrchestratorError> {
    use crate::internal::internal;
    use velnor_actions_contract::homebrew_spec::{HomebrewAuxiliaryTest, HomebrewPayloadRole};

    if platform != CheckPlatform::MacosArm64 {
        return Err(internal("native_homebrew_install_platform_unqualified"));
    }
    let mut expected = BTreeMap::new();
    for selected in projection.selected_payloads() {
        let host = selected.role() == HomebrewPayloadRole::Host;
        let architecture = selected.target().split('-').next().unwrap_or_default();
        if host && selected.target() != platform.target()
            || !host && (architecture != platform.arch() || !selected.target().contains("linux"))
        {
            continue;
        }
        for member in selected.members() {
            let file = if host {
                if !spec.host.bins.contains(member) {
                    return Err(internal("native_check_host_member_unmapped"));
                }
                NativeExpectedInstallFile {
                    asset: selected.asset_name().to_owned(),
                    member: member.clone(),
                    destination: PathBuf::from("bin").join(member),
                    mode: crate::homebrew::install::HOMEBREW_HOST_INSTALLED_MODE,
                    test: HomebrewAuxiliaryTest::None,
                    elf: None,
                }
            } else {
                let file = spec
                    .auxiliaries
                    .iter()
                    .filter(|item| item.product == selected.product())
                    .flat_map(|item| item.files.iter())
                    .find(|file| file.target == selected.target() && file.member == *member)
                    .ok_or_else(|| internal("native_check_auxiliary_member_unmapped"))?;
                let mode = crate::homebrew::install::auxiliary_installed_mode(file.mode);
                NativeExpectedInstallFile {
                    asset: selected.asset_name().to_owned(),
                    member: member.clone(),
                    destination: PathBuf::from("libexec").join(&file.destination),
                    mode,
                    test: file.test,
                    elf: (file.test == HomebrewAuxiliaryTest::LinuxElf).then_some((2, 1, 183)),
                }
            };
            if expected
                .insert((file.asset.clone(), file.member.clone()), file)
                .is_some()
            {
                return Err(internal("native_check_install_inventory_duplicate"));
            }
        }
    }
    if expected.is_empty() {
        return Err(internal("native_check_install_inventory_empty"));
    }
    Ok(expected.into_values().collect())
}

pub(crate) fn protected_stable_is_activated(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    cutover: Option<&crate::homebrew::structural_cutover::PrivateVerifiedSeedOutputBase>,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<bool, crate::OrchestratorError> {
    use crate::internal::internal;
    use velnor_actions_contract::{
        homebrew_spec::{
            HomebrewFormulaReleaseLiterals, parse_generated_formula, render_homebrew_formula,
            validate_homebrew_projection,
        },
        package_release::PackageReleaseChannel,
    };

    let channel = authority
        .spec()
        .channels
        .iter()
        .find(|item| item.channel == PackageReleaseChannel::Stable)
        .ok_or_else(|| internal("native_stable_formula_policy_missing"))?;
    let bytes = authority.protected_tap_file(&channel.destination, 1024 * 1024, deadline)?;
    let mut stable_plan = authority.plan().clone();
    stable_plan.channel = PackageReleaseChannel::Stable;
    let projection = validate_homebrew_projection(authority.spec(), &stable_plan)
        .map_err(|_| internal("native_stable_formula_projection"))?;
    let Ok(parsed) = parse_generated_formula(authority.spec(), &projection, &bytes) else {
        if let Some(witness) = cutover
            && crate::homebrew::structural_cutover::is_exact_legacy_stable_formula(
                authority, witness, deadline,
            )?
        {
            return Ok(false);
        }
        return Err(internal("native_stable_formula_invalid"));
    };
    if parsed.is_disabled_stable_sentinel() {
        let sentinel =
            HomebrewFormulaReleaseLiterals::stable_sentinel(authority.spec(), &projection)
                .map_err(|_| internal("native_stable_sentinel_invalid"))?;
        if render_homebrew_formula(authority.spec(), &sentinel)
            .map_err(|_| internal("native_stable_formula_render"))?
            != bytes
        {
            return Err(internal("native_stable_sentinel_mismatch"));
        }
        return Ok(false);
    }
    let source_sha = parsed
        .source_sha()
        .ok_or_else(|| internal("native_stable_source_sha_missing"))?
        .to_owned();
    let tag = parsed
        .tag()
        .ok_or_else(|| internal("native_stable_tag_missing"))?
        .to_owned();
    if tag.is_empty() || source_sha.is_empty() {
        return Err(internal("native_stable_selector_missing"));
    }
    let policy = authority.public_policy()?;
    let selection = crate::package_release_public::PackageReleasePublicSelection::new(
        PackageReleaseChannel::Stable,
        tag,
        source_sha,
    )?;
    let release = crate::package_release_public::verify_public_package_release(
        &policy, &selection, deadline,
    )?;
    if release.receipt().channel != PackageReleaseChannel::Stable {
        return Err(internal("native_stable_release_channel_mismatch"));
    }
    Ok(true)
}
