//! Canonical definitions and typed call sites for pure verifier composites.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::{RUST_VERIFIER_USES, Step, StepKind, StepRole};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::render::RenderedFile;

use crate::OrchestratorError;

#[derive(Debug, Clone)]
pub(crate) struct VerifierAction {
    pub(crate) uses: &'static str,
    pub(crate) name: &'static str,
    pub(crate) role: Option<StepRole>,
    pub(crate) file: RenderedFile,
}

/// One supported direct-shell verifier shape retained for release workflows.
#[derive(Debug, Clone)]
pub(crate) struct InlineVerifierTemplate {
    pub(crate) run_scalar: String,
    pub(crate) shell: Option<&'static str>,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) required_env: Vec<&'static str>,
}

impl InlineVerifierTemplate {
    fn wrapped(step: &Step, required_env: &[&'static str]) -> Result<Self, OrchestratorError> {
        let StepKind::Shell { env, .. } = &step.kind else {
            return Err(OrchestratorError::Contract {
                problem: "verifier_inline_template_not_shell".to_owned(),
            });
        };
        Ok(Self {
            run_scalar: run_scalar(step)?,
            shell: Some(velnor_actions_workflow_renderer::VERIFY_SHELL),
            env: env.clone(),
            required_env: required_env.to_vec(),
        })
    }
}

/// Environment carried by the standalone preview producer's Rust source-prep
/// job. Keep this shared with source preparation so its inline verifier audit
/// expects exactly the resolved Mise/Cargo/Rustup context the renderer emits.
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

/// One immutable set of the three verifier definitions emitted for a catalog.
#[derive(Debug, Clone)]
pub(crate) struct VerifierActionSet {
    pub(crate) restored_rust: VerifierAction,
    pub(crate) installed_rust: VerifierAction,
    pub(crate) jactionlint: VerifierAction,
    pub(crate) inline_rust_templates: BTreeMap<&'static str, Vec<InlineVerifierTemplate>>,
    pub(crate) inline_jactionlint_templates: Vec<InlineVerifierTemplate>,
}

impl VerifierActionSet {
    /// Build action definitions from the same canonical scrubbed shell steps
    /// that the pre-extraction workflow used.
    ///
    /// # Errors
    ///
    /// Returns a contract/render error when a canonical step is invalid.
    pub(crate) fn build(
        catalog: &ToolCatalog,
        generator_version: &str,
    ) -> Result<Self, OrchestratorError> {
        let restored_step =
            crate::workflow::workflow_policy::rust_manifest_step::verifier_shell_step(
                catalog,
                crate::workflow::ToolchainPhase::Restored,
            )?;
        let installed_step =
            crate::workflow::workflow_policy::rust_manifest_step::verifier_shell_step(
                catalog,
                crate::workflow::ToolchainPhase::Installed,
            )?;
        let jactionlint_step =
            crate::workflow::workflow_policy::jactionlint_verify_step::verifier_shell_step(
                catalog,
            )?;
        let default_home_step =
            velnor_actions_workflow_renderer::verifier_composite::verifier_shell_step(
                crate::workflow::VERIFY_RUST_MANIFEST_STEP,
                crate::workflow::verify_rust_argv(catalog),
                BTreeMap::new(),
            )?;
        let producer_env = producer_cargo_env(catalog)?;
        let producer_home_step =
            velnor_actions_workflow_renderer::verifier_composite::verifier_shell_step(
                crate::workflow::VERIFY_RUST_MANIFEST_STEP,
                crate::workflow::verify_rust_argv(catalog),
                producer_env,
            )?;
        let jactionlint_inline_step =
            velnor_actions_workflow_renderer::verifier_composite::verifier_shell_step(
                crate::workflow::VERIFY_JACTIONLINT_STEP,
                crate::workflow::jactionlint_verify_argv(catalog),
                BTreeMap::new(),
            )?;
        let mut inline_rust_templates = BTreeMap::new();
        let mut producer_home_template =
            InlineVerifierTemplate::wrapped(&producer_home_step, &["RUSTUP_HOME", "CARGO_HOME"])?;
        // The workflow renderer binds project tool references in step env to
        // `${{ env.* }}` after constructing the canonical Step. The audit
        // template models that final emitted YAML mapping at the same
        // boundary; keep the Step input itself in shell form for normal
        // validation and source-preparation reuse.
        for value in producer_home_template.env.values_mut() {
            *value = velnor_actions_contract::config::project_tool_refs::
                project_tool_refs_to_expressions(value);
        }
        inline_rust_templates.insert(
            crate::workflow::VERIFY_RESTORED_RUST_STEP,
            vec![InlineVerifierTemplate::wrapped(
                &restored_step,
                &["RUSTUP_HOME", "CARGO_HOME"],
            )?],
        );
        inline_rust_templates.insert(
            crate::workflow::VERIFY_RUST_MANIFEST_STEP,
            vec![
                InlineVerifierTemplate::wrapped(&installed_step, &["RUSTUP_HOME", "CARGO_HOME"])?,
                InlineVerifierTemplate::wrapped(&default_home_step, &[])?,
                producer_home_template,
            ],
        );
        let inline_jactionlint_templates = vec![
            InlineVerifierTemplate::wrapped(&jactionlint_step, &[])?,
            InlineVerifierTemplate::wrapped(&jactionlint_inline_step, &[])?,
        ];
        Ok(Self {
            restored_rust: VerifierAction {
                uses: RUST_VERIFIER_USES[0],
                name: crate::workflow::VERIFY_RESTORED_RUST_STEP,
                role: Some(StepRole::VerifyRustManifest),
                file: velnor_actions_workflow_renderer::verifier_composite::render_verifier_composite(
                    RUST_VERIFIER_USES[0],
                    &restored_step,
                    generator_version,
                )?,
            },
            installed_rust: VerifierAction {
                uses: RUST_VERIFIER_USES[1],
                name: crate::workflow::VERIFY_RUST_MANIFEST_STEP,
                role: Some(StepRole::VerifyRustManifest),
                file: velnor_actions_workflow_renderer::verifier_composite::render_verifier_composite(
                    RUST_VERIFIER_USES[1],
                    &installed_step,
                    generator_version,
                )?,
            },
            jactionlint: VerifierAction {
                uses: velnor_actions_workflow_renderer::verifier_composite::VERIFY_JACTIONLINT_USES,
                name: crate::workflow::VERIFY_JACTIONLINT_STEP,
                role: None,
                file: velnor_actions_workflow_renderer::verifier_composite::render_verifier_composite(
                    velnor_actions_workflow_renderer::verifier_composite::VERIFY_JACTIONLINT_USES,
                    &jactionlint_step,
                    generator_version,
                )?,
            },
            inline_rust_templates,
            inline_jactionlint_templates,
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
        step.name == action.name
            && step.role == action.role
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

fn run_scalar(step: &Step) -> Result<String, OrchestratorError> {
    let argv = velnor_actions_workflow_renderer::verifier_composite::verifier_shell_argv(step)
        .map_err(|error| OrchestratorError::Contract {
            problem: error.to_string(),
        })?;
    let joined = velnor_actions_workflow_renderer::join_argv_for_run(&argv).map_err(|error| {
        OrchestratorError::Contract {
            problem: error.to_string(),
        }
    })?;
    run_scalar_from_script(&joined)
}

fn run_scalar_from_script(script: &str) -> Result<String, OrchestratorError> {
    let quoted =
        velnor_actions_workflow_renderer::quote_project_tool_run_refs(script).map_err(|error| {
            OrchestratorError::Contract {
                problem: error.to_string(),
            }
        })?;
    Ok(
        velnor_actions_workflow_renderer::render_yaml(
            &velnor_actions_workflow_renderer::Yaml::str(quoted),
        )
        .trim_end()
        .to_owned(),
    )
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
