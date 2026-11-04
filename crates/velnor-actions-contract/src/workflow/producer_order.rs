// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::cache_family::{TOFU_PROVIDERS_PROBE_MISS_CONDITION, TOFU_PROVIDERS_SAVE_CONDITION};
use super::crate_job::TOFU_PROVIDERS_PRODUCER_JOB_ID_PREFIX;
use super::producer::TOOLS_PRODUCER_INSTALL_CONDITION;
use super::step_identity::RUST_VERIFIER_USES;
use super::step_protocol::ProviderRestoreForm;
use super::step_tofu_prepare::valid_provider_prepare;
use super::{Step, StepKind, StepRole, TOFU_JOB_ID_PREFIX};

pub(super) fn state_seal_order_ok(steps: &[Step]) -> bool {
    const ORDER: [StepRole; 5] = [
        StepRole::StateSealDownload,
        StepRole::StateSealSubject,
        StepRole::StateSealSave,
        StepRole::StateSealBind,
        StepRole::StateSealAttest,
    ];
    steps.len() == ORDER.len()
        && steps
            .iter()
            .zip(ORDER)
            .all(|(step, role)| step.role == Some(role))
}

pub(super) fn tofu_lane_needs_ok(needs: &[String]) -> bool {
    match needs {
        [plan] => plan == "plan",
        [plan, previous] => {
            plan == "plan"
                && (previous.starts_with(TOFU_JOB_ID_PREFIX)
                    || previous.starts_with(TOFU_PROVIDERS_PRODUCER_JOB_ID_PREFIX))
        }
        _ => false,
    }
}

pub(super) fn tofu_order_ok(steps: &[Step]) -> bool {
    let at = |role| steps.iter().position(|step| step.role == Some(role));
    let (Some(mise), Some(probe), Some(tools), Some(prepare), Some(init), Some(save)) = (
        at(StepRole::MiseSetup),
        at(StepRole::TofuProvidersRestore),
        at(StepRole::PreparePinnedTools),
        at(StepRole::TofuProvidersPrepare),
        at(StepRole::TofuProviderUse),
        at(StepRole::TofuProvidersSave),
    ) else {
        return false;
    };
    let gated = |index: usize| {
        steps[index].condition.as_deref() == Some(TOFU_PROVIDERS_PROBE_MISS_CONDITION)
    };
    mise < probe
        && probe < tools
        && tools < prepare
        && prepare < init
        && init + 1 == save
        && save + 1 == steps.len()
        && gated(tools)
        && gated(prepare)
        && gated(init)
        && steps[probe].condition.is_none()
        && steps[save].condition.as_deref() == Some(TOFU_PROVIDERS_SAVE_CONDITION)
        && ProviderRestoreForm::of(&steps[probe].kind) == Some(ProviderRestoreForm::LookupOnly)
        && matches!(&steps[probe].kind, StepKind::Action { with, .. }
            if with.get("path").is_some_and(|path| valid_provider_prepare(&steps[prepare].kind, path)))
}

pub(super) fn sources_order_ok(steps: &[Step]) -> bool {
    let last_fetch = steps
        .iter()
        .rposition(|step| step.role == Some(StepRole::CargoSourcesFetch));
    let save = steps
        .iter()
        .position(|step| step.role == Some(StepRole::CargoSourcesSave));
    matches!((last_fetch, save), (Some(fetch), Some(save)) if fetch < save && save + 1 == steps.len())
}

pub(super) fn tools_order_ok(steps: &[Step]) -> bool {
    let at = |role| steps.iter().position(|step| step.role == Some(role));
    let (Some(checkout), Some(mise), Some(identity), Some(probe), Some(scrub), Some(save)) = (
        at(StepRole::Checkout),
        at(StepRole::MiseSetup),
        at(StepRole::ToolsCacheIdentity),
        at(StepRole::ToolsCacheRestore),
        at(StepRole::ToolsCacheScrub),
        at(StepRole::ToolsCacheSave),
    ) else {
        return false;
    };
    checkout < mise
        && mise < identity
        && identity < probe
        && probe < scrub
        && save + 1 == steps.len()
        && steps.iter().enumerate().all(|(index, step)| {
            if step.role == Some(StepRole::ToolsCacheScrub) || is_install_role(step.role) {
                index >= scrub
                    && index < save
                    && step.condition.as_deref() == Some(TOOLS_PRODUCER_INSTALL_CONDITION)
                    && installed_verifier_only(step)
            } else {
                true
            }
        })
}

fn is_install_role(role: Option<StepRole>) -> bool {
    matches!(
        role,
        Some(
            StepRole::PreparePinnedTools
                | StepRole::PrepareRustComponents
                | StepRole::VerifyRustManifest
                | StepRole::VerifyJactionlint
        )
    )
}

fn installed_verifier_only(step: &Step) -> bool {
    match (&step.role, &step.kind) {
        (Some(StepRole::VerifyRustManifest), StepKind::Action { uses, .. }) => {
            uses == RUST_VERIFIER_USES[1]
        }
        _ => true,
    }
}
