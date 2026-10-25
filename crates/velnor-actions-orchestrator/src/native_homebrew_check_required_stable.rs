use velnor_actions_contract::{
    config::{
        CheckExecutor, CheckPlatform, CheckRunner, NativeHomebrewCheck, NativeHomebrewOperation,
    },
    homebrew_spec::{parse_generated_formula, validate_homebrew_projection},
    package_release::PackageReleaseChannel,
};
use velnor_actions_mise::CheckDeadline;

use crate::{
    OrchestratorError, homebrew::authority::VerifiedHomebrewAuthority, internal::internal,
};

pub(super) fn stable_or_preview_install_check(
    channel: PackageReleaseChannel,
) -> NativeHomebrewCheck {
    let channel_name = match channel {
        PackageReleaseChannel::Preview => "preview",
        PackageReleaseChannel::Stable => "stable",
    };
    NativeHomebrewCheck {
        id: format!("install_{channel_name}"),
        operation: NativeHomebrewOperation::Install,
        formula_channel: Some(channel),
        runner: CheckRunner {
            label: "macos-15".to_owned(),
            platform: CheckPlatform::MacosArm64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        timeout_minutes: 30,
    }
}

pub(super) const fn stable_install_required(base_active: bool, head_active: bool) -> bool {
    base_active || head_active
}

pub(super) fn protected_stable_head_is_activated(
    authority: &VerifiedHomebrewAuthority,
    deadline: CheckDeadline,
) -> Result<bool, OrchestratorError> {
    let channel = authority
        .spec()
        .channels
        .iter()
        .find(|channel| channel.channel == PackageReleaseChannel::Stable)
        .ok_or_else(|| internal("native_stable_formula_policy_missing"))?;
    let bytes = crate::package_release_publish::read_protected_source_file(
        &authority.tap().repository,
        authority.tap_repository_id(),
        "main",
        authority.tap_head_sha(),
        &channel.destination,
        1024 * 1024,
        deadline,
    )?;
    stable_head_formula_is_activated(authority.spec(), authority.plan(), Some(&bytes))
}

pub(super) fn stable_head_formula_is_activated(
    spec: &velnor_actions_contract::homebrew_spec::HomebrewFormulaSpec,
    plan: &velnor_actions_contract::PackageReleasePlan,
    bytes: Option<&[u8]>,
) -> Result<bool, OrchestratorError> {
    let bytes = bytes.ok_or_else(|| internal("native_stable_head_formula_missing"))?;
    let mut stable_plan = plan.clone();
    stable_plan.channel = PackageReleaseChannel::Stable;
    let projection = validate_homebrew_projection(spec, &stable_plan)
        .map_err(|_| internal("native_stable_head_formula_projection"))?;
    let formula = parse_generated_formula(spec, &projection, bytes)
        .map_err(|_| internal("native_stable_head_formula_invalid"))?;
    Ok(!formula.is_disabled_stable_sentinel())
}
