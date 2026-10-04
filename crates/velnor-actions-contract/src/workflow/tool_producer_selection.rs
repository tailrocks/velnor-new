//! Typed allocation for isolated executable production.
use super::{cache_trust::CACHE_TRUSTED_PUSH_EXPR, tool_producer::ToolCacheDomain};
use crate::ContractError;
use serde::{Deserialize, Serialize};

/// Plan exposes the actual early planner result under this fixed output name.
pub const PLAN_CARGO_FALLBACK_OUTPUT: &str = "cargo_fallback_required";
/// A full producer observes actual Cargo fallback after successful Plan.
pub const PLAN_CARGO_FALLBACK_CONDITION: &str =
    "needs.plan.outputs.cargo_fallback_required == 'true'";

/// The planner coverage channel owns selected task identities.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolProducerSelection {
    /// Sorted tasks served by this exact tool domain.
    pub tasks: Vec<String>,
    /// This domain supplies Plan's actual Cargo fallback.
    pub cargo_fallback: bool,
    /// A selected always-on validation or publication role needs this domain.
    pub unconditional: bool,
}

impl ToolProducerSelection {
    /// Validate canonical task authority and nonempty full-domain allocation.
    /// # Errors
    /// Rejects malformed, duplicate or unordered task identities.
    pub fn validate(&self) -> Result<(), ContractError> {
        for task in &self.tasks {
            crate::validate_task_id(task)?;
        }
        if self.tasks.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(ContractError::identity(
                "tool_producer",
                "invalid_task_selection",
            ));
        }
        Ok(())
    }

    /// Exact producer scheduling; full domains never precede Plan.
    #[must_use]
    pub fn condition(&self, domain: ToolCacheDomain) -> String {
        if domain == ToolCacheDomain::Planning {
            return super::cache_trust::CACHE_SAVE_CONDITION.to_owned();
        }
        let selected = if self.unconditional {
            "true".to_owned()
        } else {
            let mut choices: Vec<_> = self
                .tasks
                .iter()
                .map(|task| format!("!contains(needs.plan.outputs.covered_tasks, ',{task},')"))
                .collect();
            if self.cargo_fallback {
                choices.push(PLAN_CARGO_FALLBACK_CONDITION.to_owned());
            }
            if choices.is_empty() {
                "false".to_owned()
            } else {
                choices.join(" || ")
            }
        };
        format!(
            "!cancelled() && {CACHE_TRUSTED_PUSH_EXPR} && needs.plan.result == 'success' && ({selected})"
        )
    }

    /// Canonical dependency boundary: planning production is independently runnable.
    #[must_use]
    pub fn needs(&self, domain: ToolCacheDomain) -> Vec<String> {
        if domain == ToolCacheDomain::Planning {
            Vec::new()
        } else {
            vec!["plan".to_owned()]
        }
    }
}
