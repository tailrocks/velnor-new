//! Tofu roots + evidence step (T09/T10): configured candidates, plan notes.
//!
//! Configured roots qualify against the checkout (grammar, canonical
//! containment, effective config) and emit one candidate each for the
//! conversion arms. Without a table, filename plus E4 content
//! evidence classifies to a plan advisory (never a silent claim,
//! never an auto-detection); dialect conflict is a hard error.
//! `stacks.ignore = ["tofu"]` suppresses emission, advisories, and
//! conflict: ignoring is the explicit choice to skip the stack.

use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_contract::ContractError;
use velnor_actions_contract_config::VelnorConfig;
use velnor_actions_contract_planning::{FileIndex, StackCandidate};
use velnor_actions_tofu_core::{
    EvidenceLevel, TofuNote, classify_with_contents, effective_set, plan_note, qualify_roots,
};

use crate::toolcheck::{TOOL_INPUT_PATHS, ToolInputCheck};
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::config::CONFIG_REL;
use velnor_actions_orchestrator_core::safe_read::read_repo_file_cached;

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
    reads: &mut velnor_actions_tofu_core::FileCache,
) -> Result<TofuStep, OrchestratorError> {
    let ignored = config
        .stacks
        .ignore
        .iter()
        .any(|id| id == velnor_actions_tofu_core::STACK_ID);
    if let Some(tofu) = &config.stacks.tofu {
        return qualify_configured(root, tofu, index, ignored, reads);
    }
    let evidence = classify_with_contents(
        index.files(),
        &config_contents(root, index, reads),
        &mise_toml_values(tool_checks),
    );
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
///
/// Live stacks additionally prove every provider root carries a
/// committed lock: readonly init would fail the root in CI, so
/// planning fails here with the manual remediation instead.
fn qualify_configured(
    root: &Path,
    tofu: &velnor_actions_contract_config::TofuStackConfig,
    index: &FileIndex,
    ignored: bool,
    reads: &mut velnor_actions_tofu_core::FileCache,
) -> Result<TofuStep, OrchestratorError> {
    let candidates = qualify_roots(CONFIG_REL, root, tofu, index).map_err(map_roots_error)?;
    if !ignored {
        for configured in &tofu.roots {
            let unit = velnor_actions_tofu_core::display_for_root(configured.unit_prefix());
            velnor_actions_tofu_core::require_committed_provider_lock(
                CONFIG_REL, root, &unit, reads,
            )
            .map_err(map_roots_error)?;
        }
    }
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

/// Bounded text of effective config files for E4 content signals.
///
/// Capped in count and bytes; unreadable entries contribute no
/// signals (no table, no claim).
fn config_contents(
    root: &Path,
    index: &FileIndex,
    reads: &mut velnor_actions_tofu_core::FileCache,
) -> BTreeMap<String, String> {
    let mut contents = BTreeMap::new();
    for path in effective_set(index.files())
        .iter()
        .take(velnor_actions_tofu_core::MAX_FILES_PER_UNIT)
    {
        if let velnor_actions_tofu_core::PinnedOutcome::Text(text) =
            read_repo_file_cached(root, path, velnor_actions_tofu_core::MAX_FILE_BYTES, reads)
        {
            contents.insert(path.clone(), text);
        }
    }
    contents
}

/// Diagnostic recommendation lines for configured tofu roots.
///
/// Lock findings (missing/stale/corrupt) plus `required_version`
/// exclusion against the Mise `opentofu` pin surface as validated
/// finding lines; anything unvalidated stays out. Ignored stacks
/// and table-less repos contribute nothing.
pub(crate) fn tofu_diagnostic_lines(
    root: &Path,
    config: &VelnorConfig,
    tool_checks: &[ToolInputCheck],
    reads: &mut velnor_actions_tofu_core::FileCache,
) -> Vec<String> {
    let ignored = config
        .stacks
        .ignore
        .iter()
        .any(|id| id == velnor_actions_tofu_core::STACK_ID);
    let Some(tofu) = &config.stacks.tofu else {
        return Vec::new();
    };
    if ignored {
        return Vec::new();
    }
    let mut values = mise_toml_values(tool_checks);
    let pin = values.remove("tools.opentofu");
    let mut lines = Vec::new();
    for configured in &tofu.roots {
        let unit = velnor_actions_tofu_core::display_for_root(configured.unit_prefix());
        let mut findings =
            velnor_actions_tofu_core::lockfile_findings_for_root(root, &unit, &mut *reads);
        if let Some(pinned) = &pin {
            let claims =
                velnor_actions_tofu_core::required_versions_for_root(root, &unit, &mut *reads);
            findings.extend(velnor_actions_tofu_core::version_compat_findings(
                &claims, pinned,
            ));
        }
        for finding in &findings {
            if finding.validate().is_ok() {
                lines.push(crate::toolfindings::finding_line(finding));
            }
        }
    }
    lines
}

/// Flattened `mise.toml` values (empty when missing or malformed).
fn mise_toml_values(tool_checks: &[ToolInputCheck]) -> BTreeMap<String, String> {
    tool_checks
        .iter()
        .find(|check| check.path == TOOL_INPUT_PATHS[1])
        .map(|check| check.values.clone())
        .unwrap_or_default()
}
