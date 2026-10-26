// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::{RUST_VERIFIER_USES, Step, StepKind, StepRole};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::render::RenderedFile;

use crate::OrchestratorError;

#[path = "verifier_actions_rendered.rs"]
mod rendered;

#[derive(Debug, Clone)]
pub(crate) struct VerifierAction {
    pub(crate) uses: &'static str,
    pub(crate) name: &'static str,
    pub(crate) role: StepRole,
    pub(crate) file: RenderedFile,
}

#[derive(Debug, Clone)]
pub(crate) struct InlineVerifierTemplate {
    pub(crate) top_fields: Vec<(String, String)>,
    pub(crate) run_scalar: String,
    pub(crate) shell: Option<&'static str>,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) required_env: Vec<&'static str>,
}

impl InlineVerifierTemplate {
    fn wrapped(step: &Step, required_env: &[&'static str]) -> Result<Self, OrchestratorError> {
        Self::rendered(step, required_env, false)
    }

    fn wrapped_project_env(
        step: &Step,
        required_env: &[&'static str],
    ) -> Result<Self, OrchestratorError> {
        Self::rendered(step, required_env, true)
    }

    fn rendered(
        step: &Step,
        required_env: &[&'static str],
        bind_project_env: bool,
    ) -> Result<Self, OrchestratorError> {
        let mut fields = rendered::step_fields(step)?;
        if bind_project_env {
            rendered::bind_project_environment(&mut fields)?;
        }
        let top_fields = fields
            .iter()
            .map(|(key, value)| Ok((key.clone(), rendered::field_value(key, value)?)))
            .collect::<Result<Vec<_>, OrchestratorError>>()?;
        Ok(Self {
            top_fields,
            run_scalar: rendered::scalar_field(&fields, "run")?,
            shell: rendered::shell(&fields)?,
            env: rendered::env(&fields)?,
            required_env: required_env.to_vec(),
        })
    }
}

pub(crate) fn producer_cargo_env(
    catalog: &ToolCatalog,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let mut env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    for (native, mise) in [
        ("CARGO_HOME", "MISE_CARGO_HOME"),
        ("RUSTUP_HOME", "MISE_RUSTUP_HOME"),
    ] {
        let value = env[mise].clone();
        env.insert(native.to_owned(), value);
    }
    Ok(env)
}

#[derive(Debug, Clone)]
pub(crate) struct VerifierActionSet {
    pub(crate) restored_rust: VerifierAction,
    pub(crate) installed_rust: VerifierAction,
    pub(crate) jactionlint: VerifierAction,
    pub(crate) inline_rust_templates: BTreeMap<&'static str, Vec<InlineVerifierTemplate>>,
    pub(crate) inline_installed_rust_templates: Vec<InlineVerifierTemplate>,
    pub(crate) inline_jactionlint_templates: Vec<InlineVerifierTemplate>,
}

struct VerifierSteps {
    restored_rust: Step,
    installed_rust: Step,
    jactionlint: Step,
    default_home_rust: Step,
    producer_home_rust: Step,
    inline_jactionlint: Step,
}

struct InlineVerifierTemplates {
    rust: BTreeMap<&'static str, Vec<InlineVerifierTemplate>>,
    installed_rust: Vec<InlineVerifierTemplate>,
    jactionlint: Vec<InlineVerifierTemplate>,
}

impl VerifierActionSet {
    pub(crate) fn build(
        catalog: &ToolCatalog,
        generator_version: &str,
    ) -> Result<Self, OrchestratorError> {
        let steps = verifier_steps(catalog)?;
        let inline_templates = inline_templates(&steps)?;
        Ok(Self {
            restored_rust: verifier_action(
                RUST_VERIFIER_USES[0],
                crate::workflow::VERIFY_RESTORED_RUST_STEP,
                StepRole::VerifyRustManifest,
                &steps.restored_rust,
                generator_version,
            )?,
            installed_rust: verifier_action(
                RUST_VERIFIER_USES[1],
                crate::workflow::VERIFY_RUST_MANIFEST_STEP,
                StepRole::VerifyRustManifest,
                &steps.installed_rust,
                generator_version,
            )?,
            jactionlint: verifier_action(
                velnor_actions_contract::workflow::JACTIONLINT_VERIFIER_USES,
                crate::workflow::VERIFY_JACTIONLINT_STEP,
                StepRole::VerifyJactionlint,
                &steps.jactionlint,
                generator_version,
            )?,
            inline_rust_templates: inline_templates.rust,
            inline_installed_rust_templates: inline_templates.installed_rust,
            inline_jactionlint_templates: inline_templates.jactionlint,
        })
    }

    pub(crate) fn files(&self) -> Vec<RenderedFile> {
        vec![
            self.restored_rust.file.clone(),
            self.installed_rust.file.clone(),
            self.jactionlint.file.clone(),
        ]
    }

    pub(crate) fn by_uses(&self, uses: &str) -> Option<&VerifierAction> {
        [&self.restored_rust, &self.installed_rust, &self.jactionlint]
            .into_iter()
            .find(|action| action.uses == uses)
    }

    pub(crate) fn canonical_call(step: &Step, action: &VerifierAction) -> bool {
        step.role == Some(action.role)
            && step.id.is_none()
            && step.condition.is_none()
            && !step.parallel
            && matches!(
                &step.kind,
                StepKind::Action { uses, with, env }
                    if uses == action.uses && with.is_empty() && env.is_empty()
            )
    }
}

fn verifier_steps(catalog: &ToolCatalog) -> Result<VerifierSteps, OrchestratorError> {
    let restored_rust = crate::workflow::workflow_policy::rust_manifest_step::verifier_shell_step(
        catalog,
        crate::workflow::ToolchainPhase::Restored,
    )?;
    let installed_rust = crate::workflow::workflow_policy::rust_manifest_step::verifier_shell_step(
        catalog,
        crate::workflow::ToolchainPhase::Installed,
    )?;
    let jactionlint =
        crate::workflow::workflow_policy::jactionlint_verify_step::verifier_shell_step(catalog)?;
    let default_home_rust =
        velnor_actions_workflow_renderer::verifier_composite::verifier_shell_step(
            StepRole::VerifyRustManifest,
            crate::workflow::VERIFY_RUST_MANIFEST_STEP,
            crate::workflow::verify_rust_argv(catalog),
            BTreeMap::new(),
        )?;
    let producer_home_rust =
        velnor_actions_workflow_renderer::verifier_composite::verifier_shell_step(
            StepRole::VerifyRustManifest,
            crate::workflow::VERIFY_RUST_MANIFEST_STEP,
            crate::workflow::verify_rust_argv(catalog),
            producer_cargo_env(catalog)?,
        )?;
    let inline_jactionlint =
        velnor_actions_workflow_renderer::verifier_composite::verifier_shell_step(
            StepRole::VerifyJactionlint,
            crate::workflow::VERIFY_JACTIONLINT_STEP,
            crate::workflow::jactionlint_verify_argv(catalog),
            BTreeMap::new(),
        )?;
    Ok(VerifierSteps {
        restored_rust,
        installed_rust,
        jactionlint,
        default_home_rust,
        producer_home_rust,
        inline_jactionlint,
    })
}

fn inline_templates(steps: &VerifierSteps) -> Result<InlineVerifierTemplates, OrchestratorError> {
    let producer_home_template = InlineVerifierTemplate::wrapped_project_env(
        &steps.producer_home_rust,
        &["RUSTUP_HOME", "CARGO_HOME"],
    )?;
    let mut inline_rust_templates = BTreeMap::new();
    inline_rust_templates.insert(
        crate::workflow::VERIFY_RESTORED_RUST_STEP,
        vec![InlineVerifierTemplate::wrapped(
            &steps.restored_rust,
            &["RUSTUP_HOME", "CARGO_HOME"],
        )?],
    );
    inline_rust_templates.insert(
        crate::workflow::VERIFY_RUST_MANIFEST_STEP,
        vec![
            InlineVerifierTemplate::wrapped(&steps.installed_rust, &["RUSTUP_HOME", "CARGO_HOME"])?,
            InlineVerifierTemplate::wrapped(&steps.default_home_rust, &[])?,
            producer_home_template.clone(),
        ],
    );
    let inline_jactionlint_templates = vec![
        InlineVerifierTemplate::wrapped(&steps.jactionlint, &[])?,
        InlineVerifierTemplate::wrapped(&steps.inline_jactionlint, &[])?,
    ];
    let inline_installed_rust_templates = vec![
        InlineVerifierTemplate::wrapped(&steps.installed_rust, &["RUSTUP_HOME", "CARGO_HOME"])?,
        InlineVerifierTemplate::wrapped(&steps.default_home_rust, &[])?,
        producer_home_template,
    ];
    Ok(InlineVerifierTemplates {
        rust: inline_rust_templates,
        installed_rust: inline_installed_rust_templates,
        jactionlint: inline_jactionlint_templates,
    })
}

fn verifier_action(
    uses: &'static str,
    name: &'static str,
    role: StepRole,
    step: &Step,
    version: &str,
) -> Result<VerifierAction, OrchestratorError> {
    let file = velnor_actions_workflow_renderer::verifier_composite::render_verifier_composite(
        uses, step, version,
    )?;
    Ok(VerifierAction {
        uses,
        name,
        role,
        file,
    })
}

pub(crate) fn call_step(name: &str, uses: &'static str, role: Option<StepRole>) -> Step {
    Step {
        name: name.to_owned(),
        id: None,
        role,
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    }
}
