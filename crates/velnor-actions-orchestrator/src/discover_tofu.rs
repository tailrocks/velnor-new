//! Tofu roots + evidence step (T09): configured candidates, plan notes.
//!
//! Configured roots qualify against the checkout (grammar, canonical
//! containment, effective config) and emit one candidate each; the
//! T10+ conversion arms stay fail-closed downstream. Without a table,
//! filename evidence classifies to a plan advisory (never a silent
//! claim, never an auto-detection); dialect conflict is a hard error.
//! `stacks.ignore = ["tofu"]` suppresses emission, advisories, and
//! conflict: ignoring is the explicit choice to skip the stack.

use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_contract::{ContractError, FileIndex, StackCandidate, VelnorConfig};
use velnor_actions_tofu::{EvidenceLevel, TofuNote, classify, plan_note, qualify_roots};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::toolcheck::{TOOL_INPUT_PATHS, ToolInputCheck};

/// Tofu step output: configured candidates plus an optional plan note.
pub(crate) struct TofuStep {
    /// One candidate per configured root (empty when ignored/absent).
    pub candidates: Vec<StackCandidate>,
    /// Plan note (ignored marker or table-less advisory).
    pub note: Option<TofuNote>,
}

/// Qualify configured roots or classify table-less evidence.
///
/// # Errors
///
/// Returns config errors for bad roots, and a detection error for
/// dialect conflict.
pub(crate) fn qualify_tofu_step(
    root: &Path,
    config: &VelnorConfig,
    index: &FileIndex,
    tool_checks: &[ToolInputCheck],
) -> Result<TofuStep, OrchestratorError> {
    let ignored = config
        .stacks
        .ignore
        .iter()
        .any(|id| id == velnor_actions_tofu::STACK_ID);
    if let Some(tofu) = &config.stacks.tofu {
        return qualify_configured(root, tofu, index, ignored);
    }
    let evidence = classify(index.files(), &mise_toml_values(tool_checks));
    if evidence.level == EvidenceLevel::Conflict && !ignored {
        return Err(OrchestratorError::Detection {
            problem: format!("tofu_dialect_conflict:{}", evidence.signals.join(",")),
        });
    }
    let note = if ignored && evidence.level != EvidenceLevel::None {
        Some(TofuNote::Ignored)
    } else {
        plan_note(&evidence)
    };
    Ok(TofuStep {
        candidates: Vec::new(),
        note,
    })
}

/// Qualify configured roots; ignored stacks validate but emit nothing.
fn qualify_configured(
    root: &Path,
    tofu: &velnor_actions_contract::TofuStackConfig,
    index: &FileIndex,
    ignored: bool,
) -> Result<TofuStep, OrchestratorError> {
    let candidates = qualify_roots(CONFIG_REL, root, tofu, index).map_err(map_roots_error)?;
    Ok(TofuStep {
        candidates: if ignored { Vec::new() } else { candidates },
        note: ignored.then_some(TofuNote::Ignored),
    })
}

/// Map root qualification failures back to key-path config errors.
fn map_roots_error(err: ContractError) -> OrchestratorError {
    match err {
        ContractError::Config {
            file,
            key_path,
            problem,
        } => OrchestratorError::Config {
            file,
            key_path,
            problem,
        },
        other => OrchestratorError::Contract {
            problem: other.to_string(),
        },
    }
}

/// Flattened `mise.toml` values (empty when missing or malformed).
fn mise_toml_values(tool_checks: &[ToolInputCheck]) -> BTreeMap<String, String> {
    tool_checks
        .iter()
        .find(|check| check.path == TOOL_INPUT_PATHS[1])
        .map(|check| check.values.clone())
        .unwrap_or_default()
}
