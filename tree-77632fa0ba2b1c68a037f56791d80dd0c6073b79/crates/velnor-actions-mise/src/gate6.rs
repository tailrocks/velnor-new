//! Gate-6-gated task-cache enablement: qualified runs and fixture tokens.
//!
//! Only qualified tasks may use the experimental artifact cache, and the
//! task TOML cache field is a schema change enabled only with Gate-6
//! qualification fixtures. Both gates live here so callers cannot reach
//! the invocation shapes or the renderer without passing them.

use velnor_actions_contract::is_valid_mise_task_name;

use crate::cache::{
    QualifiedTaskDef, TaskCacheMode, qualify_reuse, render_task_toml, task_run_argv,
};
use crate::command::EnvPolicy;
use crate::error::MiseError;

/// Opt-in key enabling project custom-task execution (P07-7).
///
/// Lives in `.velnor/config.toml` as `[tasks.custom] enabled = true`;
/// absent or false disables every custom task. Discovery of a
/// same-named project task is never execution authority.
pub const CUSTOM_TASK_OPT_IN_KEY: &str = "tasks.custom.enabled";

/// Declared effects of one custom task: construction is declaration.
///
/// There is no unknown state: every grant names all three effects, and
/// cache reuse of custom-task results additionally requires the reuse
/// qualification owned outside this gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomTaskEffects {
    /// Whether the task touches the network.
    pub network: bool,
    /// Whether the task reads the clock.
    pub clock: bool,
    /// Whether the task consumes randomness.
    pub random: bool,
}

/// Capability grant authorizing one project custom task (P07-7).
///
/// Carries the explicit opt-in, the declared effects and inputs, and the
/// trust boundary: grants execute under [`EnvPolicy::RepoTask`] only, a
/// cleared environment with no credentials and no privileged keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomTaskGrant {
    /// Granted task name (contract allowlist, same as the run argv).
    task: String,
    /// Declared effects.
    effects: CustomTaskEffects,
    /// Declared inputs.
    inputs: Vec<String>,
}

impl CustomTaskGrant {
    /// Grant `task` under an explicit opt-in with declared effects/inputs.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::CacheNotEligible`] without the opt-in, for a
    /// task name outside the contract allowlist, or for a blank input.
    pub fn new(
        task: &str,
        effects: CustomTaskEffects,
        inputs: &[String],
        opt_in: bool,
    ) -> Result<Self, MiseError> {
        if !opt_in {
            return Err(Self::refused(task, "custom_task_opt_in_required"));
        }
        if !is_valid_mise_task_name(task) {
            return Err(Self::refused(task, "custom_task_bad_name"));
        }
        if inputs.iter().any(|input| input.trim().is_empty()) {
            return Err(Self::refused(task, "custom_task_bad_input"));
        }
        Ok(Self {
            task: task.to_owned(),
            effects,
            inputs: inputs.to_vec(),
        })
    }

    /// Granted task name.
    #[must_use]
    pub fn task(&self) -> &str {
        &self.task
    }

    /// Declared effects.
    #[must_use]
    pub fn effects(&self) -> CustomTaskEffects {
        self.effects
    }

    /// Declared inputs.
    #[must_use]
    pub fn inputs(&self) -> &[String] {
        &self.inputs
    }

    /// Opt-in key that authorized this grant.
    #[must_use]
    pub fn opt_in_key() -> &'static str {
        CUSTOM_TASK_OPT_IN_KEY
    }

    /// Trust boundary: grants execute as unprivileged repo tasks only.
    #[must_use]
    pub fn execution_policy(&self) -> EnvPolicy {
        EnvPolicy::RepoTask
    }

    /// Shared refusal for a grant that fails the capability boundary.
    fn refused(task: &str, reason: &str) -> MiseError {
        MiseError::CacheNotEligible {
            task: task.to_owned(),
            reason: reason.to_owned(),
        }
    }
}

/// Fixed `mise run --task-cache <mode> <task> --file <path>` argv for a
/// qualified task only.
///
/// Reuse qualification runs first, so an unqualified task kind or a
/// nondeterministic task can never reach the invocation shape.
///
/// # Errors
///
/// Returns [`MiseError::CacheNotEligible`] for unqualified tasks and
/// [`MiseError::ArtifactEscapesRoot`] for task files outside runner temp.
pub fn qualified_task_run_argv(
    kind: &str,
    network: bool,
    clock: bool,
    random: bool,
    mode: TaskCacheMode,
    task: &str,
    file: &str,
) -> Result<Vec<String>, MiseError> {
    qualify_reuse(kind, network, clock, random)?;
    task_run_argv(mode, task, file)
}

/// Gate-6 qualification fixture token.
///
/// Names the fixture evidence enabling task-cache TOML rendering; only the
/// `gate6/<name>` shape builds. Qualification suites own the fixture ids;
/// the token keeps every render call site auditable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gate6Fixture {
    /// Fixture id in `gate6/<name>` shape.
    id: String,
}

impl Gate6Fixture {
    /// Build a token for one fixture id.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::CacheNotEligible`] unless the id has the
    /// `gate6/<name>` shape with a non-empty slash-free name.
    pub fn new(id: &str) -> Result<Self, MiseError> {
        let name = id.strip_prefix("gate6/").unwrap_or("");
        let valid = !name.is_empty()
            && !name.contains('/')
            && !name.contains("..")
            && !name.bytes().any(|byte| byte.is_ascii_whitespace());
        if valid {
            Ok(Self { id: id.to_owned() })
        } else {
            Err(MiseError::CacheNotEligible {
                task: id.to_owned(),
                reason: "bad_gate6_fixture".to_owned(),
            })
        }
    }

    /// Fixture id in `gate6/<name>` shape.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Render a versioned task TOML only with Gate-6 fixture evidence.
///
/// The sole public rendering path: callers name the qualifying fixture,
/// and the marker plus fixed fields render exactly as specified.
///
/// # Errors
///
/// Returns [`MiseError::CacheNotEligible`] for incomplete definitions.
pub fn render_gated_task_toml(
    version: &str,
    def: &QualifiedTaskDef,
    fixture: &Gate6Fixture,
) -> Result<String, MiseError> {
    debug_assert!(Gate6Fixture::new(fixture.id()).is_ok());
    render_task_toml(version, def)
}
