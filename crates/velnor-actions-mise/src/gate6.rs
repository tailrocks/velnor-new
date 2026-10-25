use velnor_actions_contract::is_valid_mise_task_name;

use crate::cache::{
    QualifiedTaskDef, TaskCacheMode, qualify_reuse, render_task_toml, task_run_argv,
};
use crate::command::EnvPolicy;
use crate::error::MiseError;

pub const CUSTOM_TASK_OPT_IN_KEY: &str = "tasks.custom.enabled";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomTaskEffects {
    pub network: bool,
    pub clock: bool,
    pub random: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomTaskGrant {
    task: String,
    effects: CustomTaskEffects,
    inputs: Vec<String>,
}

impl CustomTaskGrant {
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

    #[must_use]
    pub fn task(&self) -> &str {
        &self.task
    }

    #[must_use]
    pub fn effects(&self) -> CustomTaskEffects {
        self.effects
    }

    #[must_use]
    pub fn inputs(&self) -> &[String] {
        &self.inputs
    }

    #[must_use]
    pub fn opt_in_key() -> &'static str {
        CUSTOM_TASK_OPT_IN_KEY
    }

    #[must_use]
    pub fn execution_policy(&self) -> EnvPolicy {
        EnvPolicy::RepoTask
    }

    fn refused(task: &str, reason: &str) -> MiseError {
        MiseError::CacheNotEligible {
            task: task.to_owned(),
            reason: reason.to_owned(),
        }
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gate6Fixture {
    id: String,
}

impl Gate6Fixture {
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

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

pub fn render_gated_task_toml(
    version: &str,
    def: &QualifiedTaskDef,
    fixture: &Gate6Fixture,
) -> Result<String, MiseError> {
    debug_assert!(Gate6Fixture::new(fixture.id()).is_ok());
    render_task_toml(version, def)
}
