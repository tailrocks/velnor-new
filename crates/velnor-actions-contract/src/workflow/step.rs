//! Workflow step payloads and local validation.

use super::step_identity::{StepId, StepRole};
use crate::workflow::TaskBindingDigest;
use serde::{Deserialize, Serialize};

mod qualification_cache;
mod task_execution;
mod validation;
pub(super) use task_execution::{TaskExecutionValidation, validate_task_execution};

/// Maximum arguments carried by one generated task record.
pub const MAX_TASK_EXECUTION_ARGV: usize = 512;
/// Maximum environment pairs carried by one generated task record.
pub const MAX_TASK_EXECUTION_ENV: usize = 64;

/// One workflow step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// Step name shown in workflow logs; presentation only.
    pub name: String,
    /// Stable GitHub Actions id for output-producing steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<StepId>,
    /// Typed semantic role; never inferred from the display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<StepRole>,
    /// Run condition (`if`), serialized by the workflow renderer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Member of a contiguous concurrent step group.
    ///
    /// Adjacent parallel steps form one group. The renderer emits each member
    /// as a GitHub `background: true` step and joins the group with one
    /// `wait-all` step before the next sequential step, so a failed member
    /// fails the job at that join. Only workflow jobs may carry parallel
    /// steps; composite actions reject them.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub parallel: bool,
    /// Step payload.
    #[serde(flatten)]
    pub kind: StepKind,
}

/// Step payload variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepKind {
    /// Pinned GitHub Action step.
    Action {
        /// Full-SHA `uses` reference.
        uses: String,
        /// Action inputs.
        #[serde(default)]
        with: std::collections::BTreeMap<String, String>,
        /// Step environment (applies to main and post phases alike).
        #[serde(default)]
        env: std::collections::BTreeMap<String, String>,
    },
    /// Fixed shell argv step.
    Shell {
        /// Fixed argument vector.
        run: Vec<String>,
        /// Fixed environment.
        #[serde(default)]
        env: std::collections::BTreeMap<String, String>,
    },
    /// One fixed pinned-tool task with a structured report envelope.
    ///
    /// This variant is reserved for generated obligation steps. The task
    /// argv, identity, report version, and optional matrix cap remain
    /// separate data through rendering; no shell script is parsed to
    /// rediscover those fields.
    TaskExecution {
        /// Full fixed `mise --no-config --no-env --no-hooks exec ... -- ...` argv.
        argv: Vec<String>,
        /// Task environment, excluding report identity and matrix metadata.
        #[serde(default)]
        env: std::collections::BTreeMap<String, String>,
        /// Stable task report lookup ID.
        task_id: String,
        /// Digest binding task argv and toolchain.
        task_digest: TaskBindingDigest,
        /// Canonical pinned toolchain inputs bound by `task_digest`.
        toolchain_inputs: crate::cachekey::ToolchainInputs,
        /// Stable matrix identity for this task.
        matrix_id: String,
        /// Short matrix lookup key.
        matrix_key: String,
        /// Version of the staged report helper to invoke.
        report_helper_version: String,
        /// Optional maximum parallelism for the containing crate-job matrix.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        matrix_max_parallel: Option<u32>,
    },
    /// Closed native Homebrew check through the authenticated SDK binary.
    NativeCheckExecution {
        /// Configured native check identifier.
        check_id: String,
        /// Closed operation; never caller-supplied argv.
        operation: crate::config::NativeHomebrewOperation,
        /// Fixed task lookup identity; digest and matrix key are resolved from the downloaded plan.
        task_id: String,
        /// Authenticated SDK release version selecting the staged binary.
        sdk_version: String,
    },
    /// Fixed internal planner/aggregation step.
    Internal {
        /// Internal operation name.
        operation: String,
        /// Fixed environment values attached by workflow expansion.
        #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        env: std::collections::BTreeMap<String, String>,
    },
}
