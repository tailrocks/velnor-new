// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{NativeAuthorityError, NativeAuthorityInput};
use crate::render::ValidatorCommand;
use velnor_actions_contract::{ValidatorKind, WorkflowPolicy};

pub mod vectors;
pub mod verify;

#[derive(Debug)]
pub enum SupportCommandError {
    Contract { problem: String },
    Render(crate::RenderError),
}
impl From<crate::RenderError> for SupportCommandError {
    fn from(error: crate::RenderError) -> Self {
        Self::Render(error)
    }
}

pub(super) fn build(
    input: &NativeAuthorityInput<'_>,
) -> Result<Vec<ValidatorCommand>, NativeAuthorityError> {
    let mut commands = Vec::new();
    if input.config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1 {
        let workspaces = input
            .cargo_workspaces
            .iter()
            .map(|workspace| workspace.root.clone())
            .collect::<Vec<_>>();
        if !workspaces.is_empty() {
            commands.push(ValidatorCommand {
                validator: ValidatorKind::CargoDeny,
                name: crate::steps::DENY_STEP_NAME.to_owned(),
                argv: vectors::deny_argv(&workspaces).map_err(NativeAuthorityError::Support)?,
                prepare_argv: Vec::new(),
            });
        }
        commands.push(ValidatorCommand {
            validator: ValidatorKind::CargoMachete,
            name: crate::steps::MACHETE_STEP_NAME.to_owned(),
            argv: vectors::machete_argv().map_err(NativeAuthorityError::Support)?,
            prepare_argv: vectors::machete_install_argv().map_err(NativeAuthorityError::Support)?,
        });
    }
    let verify = verify::verify_kinds(&input.config.workflow.verify.jobs)
        .map_err(NativeAuthorityError::Support)?;
    verify::push_verify_commands(&mut commands, &verify, &input.config.workflow.verify)
        .map_err(NativeAuthorityError::Support)?;
    Ok(commands)
}

fn validate_root(root: &str) -> Result<(), SupportCommandError> {
    velnor_actions_contract::validate_fetch_root(root).map_err(|error| {
        SupportCommandError::Contract {
            problem: error.to_string(),
        }
    })
}
fn isolated_manifest_flag(root: &str) -> String {
    crate::plan_sources::cargo_fetch::isolated_manifest_flag(root)
}
fn privilege_drop_argv(install: &str, payload: &str) -> Vec<String> {
    let prelude = crate::toolchain_env::credential_unset_prelude();
    let prefix = crate::plan_sources::cargo_fetch::cargo_isolation_prefix();
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!("{{ {install} && {prelude} }} && {prefix}{payload}"),
    ]
}

const NATIVE_VALIDATORS_SCRIPT: &str = r#"fail=0; if command -v claude >/dev/null 2>&1; then claude plugin validate --strict . || fail=1; fi; if command -v muse >/dev/null 2>&1; then muse plugins validate . || fail=1; for d in skills/*/; do [ -d "$d" ] || continue; muse skills validate "$d" || fail=1; done; fi; if command -v grok >/dev/null 2>&1; then grok plugin validate . || fail=1; fi; exit $fail"#;

#[must_use]
pub fn native_validators_argv() -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        NATIVE_VALIDATORS_SCRIPT.to_owned(),
    ]
}
