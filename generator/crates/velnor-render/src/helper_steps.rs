// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CratesMirror, HelperCommand, RepoPath};

use crate::base_steps::base_age;
use crate::gate::{PLAN_STEP, REPLAN_STEP};
use crate::release_steps::shell_word;
use crate::rules::{MBX_ACTION, RenderedStep};
use crate::steps::PREPARE_STEP;

pub(crate) fn inputs_step(
    verb: &str,
    target: &RepoPath,
    directory: Option<&RepoPath>,
) -> RenderedStep {
    let depth = directory.map_or(0, |dir| dir.as_str().split('/').count());
    let relative = format!("{}{}", "../".repeat(depth), target.as_str());
    RenderedStep {
        name: format!("velnor-ci inputs {verb}"),
        run: Some(format!(
            "velnor-ci inputs {verb} --target-dir {}",
            shell_word(&relative)
        )),
        ..RenderedStep::default()
    }
}

pub(crate) fn helper_step(command: HelperCommand) -> RenderedStep {
    let arguments = match command {
        HelperCommand::Plan => "plan",
        HelperCommand::PlanFull => "plan --full",
        HelperCommand::Prepare => "prepare",
        HelperCommand::CheckMetadata => "check-metadata",
        HelperCommand::Restored(..) => "restored",
        HelperCommand::InputsAudit => "inputs audit",
    };
    let (id, env) = match command {
        HelperCommand::Plan => (Some(PLAN_STEP), Vec::new()),
        HelperCommand::PlanFull => (Some(REPLAN_STEP), Vec::new()),
        HelperCommand::Prepare => (Some(PREPARE_STEP), Vec::new()),
        HelperCommand::Restored(expiry) => (Some("restored"), base_age(expiry)),
        _ => (None, Vec::new()),
    };
    RenderedStep {
        name: format!("velnor-ci {arguments}"),
        id: id.map(str::to_owned),
        env,
        run: Some(match command {
            HelperCommand::Plan => format!("velnor-ci {arguments} --explain"),
            _ => format!("velnor-ci {arguments}"),
        }),
        ..RenderedStep::default()
    }
}

pub(crate) fn mirror_step(mirror: &CratesMirror) -> RenderedStep {
    RenderedStep {
    name: "Configure the crates mirror in $RUNNER_TEMP/velnor-cargo/config.toml".to_owned(),
    condition: Some("${{ runner.environment == 'self-hosted' && (github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository) }}".to_owned()),
    run: Some(format!("velnor-ci crates-mirror --registry {} --cainfo {}", crate::release_steps::shell_word(&mirror.registry), crate::release_steps::shell_word(&mirror.cainfo))),
    ..RenderedStep::default()
}
}

pub(crate) fn mbx_store_step() -> RenderedStep {
    RenderedStep {
        name: "velnor-ci mbx-store".to_owned(),
        run: Some("velnor-ci mbx-store".to_owned()),
        ..RenderedStep::default()
    }
}

pub(crate) fn mbx_seed_step() -> RenderedStep {
    RenderedStep {
        name: "velnor-ci mbx-seed".to_owned(),
        condition: Some(
            "${{ github.event_name == 'pull_request' || github.event_name == 'merge_group' }}"
                .to_owned(),
        ),
        run: Some("velnor-ci mbx-seed".to_owned()),
        ..RenderedStep::default()
    }
}

pub(crate) fn restored_step(
    expiry: velnor_model::BaseExpiry,
    targets: &[RepoPath],
) -> RenderedStep {
    let mut step = helper_step(HelperCommand::Restored(expiry));
    step.env.push((
        "VELNOR_CACHE_TARGETS".to_owned(),
        crate::rules::Value::Text(
            targets
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    ));
    step
}

const MBX_ACTION_SHA: &str = "88acf2bb50552c7c508cbac13e5f8c576b95b1de";

pub(crate) fn mbx_remote_step() -> RenderedStep {
    use crate::rules::Value;
    let text = |key: &str, value: &str| (key.to_owned(), Value::Text(value.to_owned()));
    RenderedStep {
        name: "Set up the mbx cache".to_owned(),
        uses: Some(format!("{MBX_ACTION}@{MBX_ACTION_SHA}")),
        uses_comment: Some("v1.7.3".to_owned()),
        with: vec![
            text("backend", "github"),
            text("github-cache-mode", "objects"),
            (
                "cache-key-suffix".to_owned(),
                Value::Expr("${{ github.job }}".to_owned()),
            ),
            text("save-on-pull-request", "false"),
            text("save-on-protected-branch", "false"),
            text("save-on-workflow-dispatch", "false"),
        ],
        ..RenderedStep::default()
    }
}
