//! Canonical definitions and typed call sites for pure verifier composites.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::{RUST_VERIFIER_USES, Step, StepKind, StepRole};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::render::RenderedFile;
use velnor_actions_workflow_renderer::verification_jobs::WorkflowTaskPolicy;

use crate::OrchestratorError;

#[path = "verifier_actions_lower.rs"]
mod lower;
#[path = "verifier_actions_rendered.rs"]
mod rendered;

#[derive(Debug, Clone)]
pub(crate) struct VerifierAction {
    pub(crate) uses: &'static str,
    pub(crate) name: &'static str,
    pub(crate) role: StepRole,
    source_step: Step,
    pub(crate) file: RenderedFile,
}

/// One supported direct-shell verifier shape retained for release workflows.
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
    inline_source_steps: Vec<Step>,
    pub(crate) inline_rust_templates: BTreeMap<&'static str, Vec<InlineVerifierTemplate>>,
    pub(crate) inline_installed_rust_templates: Vec<InlineVerifierTemplate>,
    pub(crate) inline_jactionlint_templates: Vec<InlineVerifierTemplate>,
}

pub(crate) struct VerifierSteps {
    restored_rust: Step,
    installed_rust: Step,
    jactionlint: Step,
    default_home_rust: Step,
    producer_home_rust: Step,
    inline_jactionlint: Step,
}

impl VerifierSteps {
    pub(crate) fn is_installed_rust_inline(&self, step: &Step) -> bool {
        [
            &self.installed_rust,
            &self.default_home_rust,
            &self.producer_home_rust,
        ]
        .into_iter()
        .any(|expected| matches_canonical_shell_step(step, expected))
    }

    pub(crate) fn is_jactionlint_inline(&self, step: &Step) -> bool {
        [&self.jactionlint, &self.inline_jactionlint]
            .into_iter()
            .any(|expected| matches_canonical_shell_step(step, expected))
    }
}

struct InlineVerifierTemplates {
    rust: BTreeMap<&'static str, Vec<InlineVerifierTemplate>>,
    installed_rust: Vec<InlineVerifierTemplate>,
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
                crate::workflow::workflow_policy::rust_manifest_step::VERIFY_RESTORED_RUST_STEP,
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
            inline_source_steps: vec![
                steps.default_home_rust,
                steps.producer_home_rust,
                steps.inline_jactionlint,
            ],
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

    /// Replace only the canonical verifier shell steps in the main workflow
    /// IR with their closed composite-action references.
    ///
    /// The complete typed payload selects the action, never the display name.
    /// Release, freshness, and other separately rendered workflows keep their
    /// supported inline verifier steps.
    pub(crate) fn lower_workflow_calls(
        &self,
        workflow: &mut velnor_actions_contract::WorkflowIr,
        workflow_tasks: &[WorkflowTaskPolicy],
    ) -> Result<(), OrchestratorError> {
        lower::lower(self, workflow, workflow_tasks)
    }

    pub(crate) fn by_uses(&self, uses: &str) -> Option<&VerifierAction> {
        [&self.restored_rust, &self.installed_rust, &self.jactionlint]
            .into_iter()
            .find(|action| action.uses == uses)
    }

    #[cfg(test)]
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

impl VerifierAction {
    fn matches_shell_step(&self, step: &Step) -> bool {
        matches_canonical_shell_step(step, &self.source_step)
    }
}

fn matches_canonical_shell_step(step: &Step, expected: &Step) -> bool {
    if step.role != expected.role
        || step.id.is_some()
        || expected.id.is_some()
        || step.condition.is_some()
        || expected.condition.is_some()
        || step.parallel
        || expected.parallel
    {
        return false;
    }
    matches!(
        (&step.kind, &expected.kind),
        (
            StepKind::Shell { run, env },
            StepKind::Shell {
                run: expected_run,
                env: expected_env
            }
        ) if run == expected_run && env == expected_env
    )
}

pub(crate) fn verifier_steps(catalog: &ToolCatalog) -> Result<VerifierSteps, OrchestratorError> {
    let restored_rust = crate::workflow::verify_restored_rust_step(catalog)?;
    let installed_rust = crate::workflow::verify_rust_manifest_step(catalog)?;
    let jactionlint = crate::workflow::verify_jactionlint_step(catalog)?;
    let default_home_rust = rust_verifier_shell_step(
        crate::workflow::VERIFY_RUST_MANIFEST_STEP,
        crate::workflow::verify_rust_argv(catalog),
        BTreeMap::new(),
    )?;
    let producer_home_rust = rust_verifier_shell_step(
        crate::workflow::VERIFY_RUST_MANIFEST_STEP,
        crate::workflow::verify_rust_argv(catalog),
        producer_cargo_env(catalog)?,
    )?;
    let inline_jactionlint = jactionlint.clone();
    Ok(VerifierSteps {
        restored_rust,
        installed_rust,
        jactionlint,
        default_home_rust,
        producer_home_rust,
        inline_jactionlint,
    })
}

fn rust_verifier_shell_step(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, OrchestratorError> {
    let mut step = velnor_actions_workflow_renderer::steps::shell_step(name, argv, env)?;
    step.role = Some(StepRole::VerifyRustManifest);
    Ok(step)
}

fn inline_templates(steps: &VerifierSteps) -> Result<InlineVerifierTemplates, OrchestratorError> {
    let producer_home_template = InlineVerifierTemplate::wrapped_project_env(
        &steps.producer_home_rust,
        &["RUSTUP_HOME", "CARGO_HOME"],
    )?;
    // The workflow renderer binds project tool references in step env to
    // `${{ env.* }}` after constructing the canonical Step. The audit
    // template models that final emitted YAML mapping at the same boundary;
    // keep the Step input itself in shell form for normal validation and
    // source-preparation reuse.
    let mut inline_rust_templates = BTreeMap::new();
    inline_rust_templates.insert(
        crate::workflow::workflow_policy::rust_manifest_step::VERIFY_RESTORED_RUST_STEP,
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
        source_step: step.clone(),
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

#[cfg(test)]
#[path = "verifier_actions_lowering_tests.rs"]
mod lowering_tests;
