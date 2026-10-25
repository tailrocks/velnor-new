use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_contract::{ContractError, FileIndex, StackCandidate, VelnorConfig};
use velnor_actions_tofu::{
    EvidenceLevel, TofuNote, classify_with_contents, effective_set, plan_note, qualify_roots,
};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::safe_read::read_repo_file_cached;
use crate::toolcheck::{TOOL_INPUT_PATHS, ToolInputCheck};

pub(crate) struct TofuStep {
    pub candidates: Vec<StackCandidate>,
    pub note: Option<TofuNote>,
}

pub(crate) fn qualify_tofu_step(
    root: &Path,
    config: &VelnorConfig,
    index: &FileIndex,
    tool_checks: &[ToolInputCheck],
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<TofuStep, OrchestratorError> {
    let ignored = config
        .stacks
        .ignore
        .iter()
        .any(|id| id == velnor_actions_tofu::STACK_ID);
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

fn qualify_configured(
    root: &Path,
    tofu: &velnor_actions_contract::TofuStackConfig,
    index: &FileIndex,
    ignored: bool,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<TofuStep, OrchestratorError> {
    let candidates = qualify_roots(CONFIG_REL, root, tofu, index).map_err(map_roots_error)?;
    if !ignored {
        for configured in &tofu.roots {
            let unit = velnor_actions_tofu::display_for_root(configured.unit_prefix());
            velnor_actions_tofu::require_committed_provider_lock(CONFIG_REL, root, &unit, reads)
                .map_err(map_roots_error)?;
        }
    }
    Ok(TofuStep {
        candidates: if ignored { Vec::new() } else { candidates },
        note: ignored.then_some(TofuNote::Ignored),
    })
}

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

fn config_contents(
    root: &Path,
    index: &FileIndex,
    reads: &mut velnor_actions_tofu::FileCache,
) -> BTreeMap<String, String> {
    let mut contents = BTreeMap::new();
    for path in effective_set(index.files())
        .iter()
        .take(velnor_actions_tofu::MAX_FILES_PER_UNIT)
    {
        if let velnor_actions_tofu::PinnedOutcome::Text(text) =
            read_repo_file_cached(root, path, velnor_actions_tofu::MAX_FILE_BYTES, reads)
        {
            contents.insert(path.clone(), text);
        }
    }
    contents
}

pub(crate) fn tofu_diagnostic_lines(
    root: &Path,
    config: &VelnorConfig,
    tool_checks: &[ToolInputCheck],
    reads: &mut velnor_actions_tofu::FileCache,
) -> Vec<String> {
    let ignored = config
        .stacks
        .ignore
        .iter()
        .any(|id| id == velnor_actions_tofu::STACK_ID);
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
        let unit = velnor_actions_tofu::display_for_root(configured.unit_prefix());
        let mut findings =
            velnor_actions_tofu::lockfile_findings_for_root(root, &unit, &mut *reads);
        if let Some(pinned) = &pin {
            let claims = velnor_actions_tofu::required_versions_for_root(root, &unit, &mut *reads);
            findings.extend(velnor_actions_tofu::version_compat_findings(
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

fn mise_toml_values(tool_checks: &[ToolInputCheck]) -> BTreeMap<String, String> {
    tool_checks
        .iter()
        .find(|check| check.path == TOOL_INPUT_PATHS[1])
        .map(|check| check.values.clone())
        .unwrap_or_default()
}
