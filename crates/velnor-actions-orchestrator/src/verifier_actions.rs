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

/// One immutable set of the three verifier definitions emitted for a catalog.
#[derive(Debug, Clone)]
pub(crate) struct VerifierActionSet {
    pub(crate) restored_rust: VerifierAction,
    pub(crate) installed_rust: VerifierAction,
    pub(crate) jactionlint: VerifierAction,
    pub(crate) inline_rust_run_scalars: BTreeMap<&'static str, Vec<String>>,
    pub(crate) inline_jactionlint_run_scalars: Vec<String>,
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
        let default_home_step = velnor_actions_workflow_renderer::steps::shell_step(
            crate::workflow::VERIFY_RUST_MANIFEST_STEP,
            crate::workflow::verify_rust_argv(catalog),
            BTreeMap::new(),
        )?;
        let mut inline_rust_run_scalars = BTreeMap::new();
        inline_rust_run_scalars.insert(
            crate::workflow::VERIFY_RESTORED_RUST_STEP,
            vec![run_scalar(&restored_step)?],
        );
        inline_rust_run_scalars.insert(
            crate::workflow::VERIFY_RUST_MANIFEST_STEP,
            vec![
                run_scalar(&installed_step)?,
                run_scalar(&default_home_step)?,
                // Schema-2 release/qualification renderers preserve their
                // canonical direct-shell body; unlike workflow Steps they
                // do not pass through the shell-step credential wrapper.
                run_scalar_from_argv(&crate::workflow::verify_rust_argv(catalog))?,
            ],
        );
        let inline_jactionlint_run_scalars = vec![
            run_scalar(&jactionlint_step)?,
            run_scalar_from_argv(&crate::workflow::jactionlint_verify_argv(catalog))?,
            run_scalar_from_script(
                &crate::workflow::workflow_policy::jactionlint_verify_step::script(catalog),
            )?,
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
            inline_rust_run_scalars,
            inline_jactionlint_run_scalars,
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

    pub(crate) fn is_canonical_inline_rust_step(
        &self,
        name: &str,
        run_scalar: Option<&str>,
    ) -> bool {
        self.inline_rust_run_scalars
            .get(name)
            .is_some_and(|expected| run_scalar.is_some_and(|run| expected.iter().any(|v| v == run)))
    }

    pub(crate) fn is_canonical_inline_jactionlint_step(&self, run_scalar: Option<&str>) -> bool {
        run_scalar.is_some_and(|run| {
            self.inline_jactionlint_run_scalars
                .iter()
                .any(|expected| expected == run)
        })
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
    let StepKind::Shell { run, .. } = &step.kind else {
        return Err(OrchestratorError::Contract {
            problem: "verifier_inline_template_not_shell".to_owned(),
        });
    };
    run_scalar_from_argv(run)
}

fn run_scalar_from_argv(argv: &[String]) -> Result<String, OrchestratorError> {
    let joined = velnor_actions_workflow_renderer::join_argv_for_run(argv).map_err(|error| {
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
