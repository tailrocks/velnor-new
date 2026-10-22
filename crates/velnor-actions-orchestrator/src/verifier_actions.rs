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
    jactionlint: Vec<InlineVerifierTemplate>,
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
        let steps = verifier_steps(catalog)?;
        let inline_templates = inline_templates(&steps)?;
        Ok(Self {
            restored_rust: verifier_action(
                RUST_VERIFIER_USES[0],
                crate::workflow::VERIFY_RESTORED_RUST_STEP,
                Some(StepRole::VerifyRustManifest),
                &steps.restored_rust,
                generator_version,
            )?,
            installed_rust: verifier_action(
                RUST_VERIFIER_USES[1],
                crate::workflow::VERIFY_RUST_MANIFEST_STEP,
                Some(StepRole::VerifyRustManifest),
                &steps.installed_rust,
                generator_version,
            )?,
            jactionlint: verifier_action(
                velnor_actions_workflow_renderer::verifier_composite::VERIFY_JACTIONLINT_USES,
                crate::workflow::VERIFY_JACTIONLINT_STEP,
                None,
                &steps.jactionlint,
                generator_version,
            )?,
            inline_rust_templates: inline_templates.rust,
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
            crate::workflow::VERIFY_RUST_MANIFEST_STEP,
            crate::workflow::verify_rust_argv(catalog),
            BTreeMap::new(),
        )?;
    let producer_home_rust =
        velnor_actions_workflow_renderer::verifier_composite::verifier_shell_step(
            crate::workflow::VERIFY_RUST_MANIFEST_STEP,
            crate::workflow::verify_rust_argv(catalog),
            producer_cargo_env(catalog)?,
        )?;
    let inline_jactionlint =
        velnor_actions_workflow_renderer::verifier_composite::verifier_shell_step(
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
    let mut producer_home_template =
        InlineVerifierTemplate::wrapped(&steps.producer_home_rust, &["RUSTUP_HOME", "CARGO_HOME"])?;
    // The workflow renderer binds project tool references in step env to
    // `${{ env.* }}` after constructing the canonical Step. The audit
    // template models that final emitted YAML mapping at the same boundary;
    // keep the Step input itself in shell form for normal validation and
    // source-preparation reuse.
    for value in producer_home_template.env.values_mut() {
        *value =
            velnor_actions_contract::config::project_tool_refs::project_tool_refs_to_expressions(
                value,
            );
    }
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
            producer_home_template,
        ],
    );
    let inline_jactionlint_templates = vec![
        InlineVerifierTemplate::wrapped(&steps.jactionlint, &[])?,
        InlineVerifierTemplate::wrapped(&steps.inline_jactionlint, &[])?,
    ];
    Ok(InlineVerifierTemplates {
        rust: inline_rust_templates,
        jactionlint: inline_jactionlint_templates,
    })
}

fn verifier_action(
    uses: &'static str,
    name: &'static str,
    role: Option<StepRole>,
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
