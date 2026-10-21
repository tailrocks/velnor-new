//! Cache and lane step templates over validated action refs.
//!
//! Covers MBX objects-mode restore, `actions/cache` restore/save, and
//! per-lane target directories; pins arrive validated.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

#[path = "cache_steps_mbx_bundle.rs"]
mod mbx_bundle;
#[path = "cache_steps_mbx_command.rs"]
mod mbx_command;
#[path = "cache_steps_mbx_gate.rs"]
mod mbx_gate;
#[path = "cache_steps_mbx_preflight.rs"]
mod mbx_preflight;
pub use mbx_bundle::{
    MBX_BUNDLE_EXPORT_NAME, MBX_BUNDLE_IMPORT_NAME, MBX_BUNDLE_KEY_NAME, MBX_BUNDLE_RESTORE_NAME,
    MBX_BUNDLE_SAVE_NAME, MBX_EXPORT_GROUP_ENV, MBX_EXPORT_GROUP_VALUE, MBX_PRODUCER_MARK_NAME,
    mbx_producer_mark_step,
};
pub(crate) use mbx_bundle::{
    append_bundle_postludes, bundle_helper_file as mbx_bundle_helper_file,
    export_step as mbx_bundle_export_step, import_step as mbx_bundle_import_step,
};
pub(crate) use mbx_gate::append_workspace_cleanups;
pub use mbx_gate::check_mbx_gating;
pub(crate) use mbx_gate::is_mbx_action;
pub(crate) use mbx_preflight::{
    ACTION_MBX_BIN, MBX_BUILD_SCRIPT_EXECUTION_ENV, MBX_BUILD_SCRIPT_EXECUTION_VALUE, MBX_CC_ENV,
    MBX_CC_VALUE, MBX_GC_AUTO_ENV, MBX_GC_AUTO_VALUE, MBX_SHARE_OUT_DIR_ENV,
    MBX_SHARE_OUT_DIR_VALUE, RESOLVE_ACTION_MBX_BIN, mbx_version_check_step,
    mbx_workspace_clean_step_for_action,
};
pub use mbx_preflight::{
    MBX_PREFLIGHT_NAME, MBX_RESTORE_NAME, MBX_VERSION_CHECK_NAME, MbxTransport,
    mbx_steps_for_driver,
};

use crate::{
    RenderError,
    steps::{action_step, validate_uses},
};

#[path = "cache_steps_tools.rs"]
mod tools;

pub use tools::{
    TOOLS_CACHE_PATH, TOOLS_CACHE_PATHS, TOOLS_RESTORE_ACTION_USES, TOOLS_RESTORE_NAME,
    TOOLS_RESTORE_USES, TOOLS_SAVE_NAME, TOOLS_SAVE_USES,
};
pub(crate) use tools::{TOOLS_SEED_ADMITTED_EXPRESSION, TOOLS_SEED_ADMITTED_INPUT};

#[cfg(test)]
pub(crate) use tools::assert_rendered_admission_parses;
pub(crate) use tools::restore_action_file as tools_restore_action_file;

/// Build the fixed V2 tools restore composite call.
/// # Errors
pub(crate) fn tools_cache_restore_step(
    key: &str,
    mise_sha256: &str,
    condition: Option<String>,
) -> Result<Step, RenderError> {
    tools::restore_step(key, mise_sha256, condition)
}

/// Build the V2 tools save over the fixed path set.
/// # Errors
pub(crate) fn tools_cache_save_step(
    key: &str,
    condition: Option<String>,
) -> Result<Step, RenderError> {
    tools::save_step(key, condition)
}

pub(crate) fn validate_tools_restore_call(step: &Step) -> Result<&str, RenderError> {
    tools::validate_restore_call(step)
}

/// Pinned mr-boxington action name (objects mode).
pub const MBX_ACTION_NAME: &str = "jdx/mr-boxington-action";
/// Cache restore/save action names.
pub const CACHE_RESTORE_NAME: &str = "actions/cache/restore";
/// Cache save action name.
pub const CACHE_SAVE_NAME: &str = "actions/cache/save";
/// Task-artifacts dir archived for task-result reuse.
pub const TASK_ARTIFACTS_DIR: &str = "$MISE_TASK_CACHE_DIR/task-artifacts/v2";

/// Never-archive markers: state, plans, and credential-bearing names
/// must never enter a cache archive (T22).
///
/// Exact mirror of the Mise list (`cache_sources`); the orchestrator
/// pins both equal by test, like the credential denylists.
pub const NEVER_ARCHIVE_MARKERS: [&str; 3] = ["credentials", ".tfstate", ".tfplan"];

/// True when `path` names state, plans, or credentials.
#[must_use]
pub fn is_never_archive_path(path: &str) -> bool {
    NEVER_ARCHIVE_MARKERS
        .iter()
        .any(|marker| path.contains(marker))
}

/// Detected Rust compile driver (task-execution contract profile).
///
/// The orchestrator maps the workspace's detected profile to this typed
/// selector; the renderer never inspects evidence itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileDriver {
    /// Cargo profile: MBX action and installation are absent.
    Cargo,
    /// MBX profile: the objects-mode action restores compiler objects.
    Mbx,
}

/// Cache restore/save step over `actions/cache`.
/// # Errors
pub fn cache_action_step(
    restore: bool,
    uses: &str,
    layer: &str,
    key: &str,
    restore_keys: &[String],
    paths: &[String],
) -> Result<Step, RenderError> {
    validate_uses(uses)?;
    let want = if restore {
        CACHE_RESTORE_NAME
    } else {
        CACHE_SAVE_NAME
    };
    if !uses.starts_with(&format!("{want}@")) {
        return Err(RenderError::BadActionRef(format!("bad_cache_uses:{uses}")));
    }
    if !matches!(
        layer,
        "mbx-objects" | "mbx-bundle" | "sources" | "task" | "tools" | "tofu-providers"
    ) {
        return Err(RenderError::BadCommand("mbx_needs_objects_mode".to_owned()));
    }
    if key.trim().is_empty()
        || (key.contains(' ') || key.contains('\n')) && !valid_owner_key_expression(layer, key)
    {
        return Err(RenderError::BadCommand("bad_cache_key".to_owned()));
    }
    if paths.is_empty() {
        return Err(RenderError::BadCommand("empty_cache_paths".to_owned()));
    }
    for path in paths {
        validate_cache_path(layer, path)?;
    }
    let mut with = BTreeMap::from([
        ("key".to_owned(), key.to_owned()),
        ("path".to_owned(), paths.join("\n")),
    ]);
    if restore {
        with.insert("restore-keys".to_owned(), restore_keys.join("\n"));
    }
    let name = if restore {
        "Restore cache"
    } else {
        "Save cache"
    };
    action_step(name, uses, with)
}

fn validate_cache_path(layer: &str, path: &str) -> Result<(), RenderError> {
    if valid_owner_stage_path(layer, path) {
        return Ok(());
    }
    if layer == "mbx-objects" {
        return Err(RenderError::BadCommand(format!("bad_cache_path:{path}")));
    }
    if layer == "sources" && sources_subset_ok(path)
        || layer == "task" && path == TASK_ARTIFACTS_DIR
        || layer == "tools" && tools::tools_cache_path_ok(path)
        || layer == "tofu-providers" && crate::tofu_cache::tofu_providers_path_ok(path)
        || layer == "mbx-bundle"
            && path == velnor_actions_contract::workflow::cache_family_mbx::MBX_BUNDLE_PATH_EXPR
    {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!("bad_cache_path:{path}")))
    }
}

fn valid_owner_stage_path(action_layer: &str, path: &str) -> bool {
    let Some(step_id) = path.strip_prefix("${{ steps.").and_then(|value| {
        [
            ".outputs.restore_stage_path }}",
            ".outputs.save_stage_path }}",
        ]
        .iter()
        .find_map(|suffix| value.strip_suffix(suffix))
    }) else {
        return false;
    };
    let Some(tag) = step_id.strip_prefix("qualification-cache-bind-") else {
        return false;
    };
    let Some((id_layer, owner_hash)) = tag.rsplit_once('-') else {
        return false;
    };
    let expected = match action_layer {
        "mbx-objects" => "mbx_objects",
        "sources" => "cargo_sources",
        "tools" => "mise_tools",
        "tofu-providers" => "tofu_providers",
        "task" => "task_result",
        _ => return false,
    };
    id_layer == expected
        && owner_hash.len() == 16
        && owner_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_owner_key_expression(action_layer: &str, value: &str) -> bool {
    let Some(rest) = value.strip_prefix("${{ steps.") else {
        return false;
    };
    let Some((step, output)) = rest.split_once(".outputs.") else {
        return false;
    };
    let Some(step) = step.strip_prefix("qualification-cache-bind-") else {
        return false;
    };
    let Some((layer, hash)) = step.rsplit_once('-') else {
        return false;
    };
    let expected = match action_layer {
        "mbx-objects" => "mbx_objects",
        "sources" => "cargo_sources",
        "tools" => "mise_tools",
        "tofu-providers" => "tofu_providers",
        "task" => "task_result",
        _ => return false,
    };
    layer == expected
        && hash.len() == 16
        && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        && matches!(output.strip_suffix(" }}"), Some("restore_key" | "save_key"))
}

/// P08 sources subset under the owned Cargo home expression.
fn sources_subset_ok(path: &str) -> bool {
    const HOME: &str = "${{ runner.temp }}/velnor/cargo";
    if path.contains("..") || is_never_archive_path(path) {
        return false;
    }
    let Some(suffix) = path.strip_prefix(&format!("{HOME}/")) else {
        return false;
    };
    if suffix.starts_with("registry/src") {
        return false;
    }
    matches!(suffix, "registry/index" | "registry/cache" | "git/db")
        || suffix.starts_with("registry/index/")
        || suffix.starts_with("registry/cache/")
        || suffix.starts_with("git/db/")
}

/// Check restore-before/save-after ordering over cache action steps.
///
/// Every `actions/cache/restore` step (plus MBX objects restore) must
/// precede every `actions/cache/save` step within one job.
/// # Errors
pub fn check_cache_step_order(steps: &[Step]) -> Result<(), RenderError> {
    let mut last_restore: Option<usize> = None;
    let mut first_save: Option<usize> = None;
    for (index, step) in steps.iter().enumerate() {
        let StepKind::Action { uses, .. } = &step.kind else {
            continue;
        };
        if uses == TOOLS_RESTORE_USES
            || uses.starts_with("actions/cache/restore@")
            || uses.starts_with(&format!("{MBX_ACTION_NAME}@"))
        {
            last_restore = Some(index);
        } else if uses.starts_with("actions/cache/save@") {
            first_save.get_or_insert(index);
        }
    }
    if let (Some(restore), Some(save)) = (last_restore, first_save)
        && restore > save
    {
        return Err(RenderError::InvalidWorkflow(
            "cache_save_before_restore".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod owner_stage_path_tests {
    use super::cache_action_step;
    use velnor_actions_actionlint::PinnedActionRef;
    use velnor_actions_actionlint::actions::{CACHE_ACTION_SHA, CACHE_ACTION_VERSION};

    #[test]
    fn owner_bound_stage_path_is_accepted_only_for_matching_layer_and_key() {
        let uses = PinnedActionRef::new(
            "actions/cache",
            Some("restore"),
            CACHE_ACTION_SHA,
            CACHE_ACTION_VERSION,
        )
        .expect("pinned cache restore")
        .uses_value();
        let path = "${{ steps.qualification-cache-bind-mbx_objects-0123456789abcdef.outputs.restore_stage_path }}";
        let key = "${{ steps.qualification-cache-bind-mbx_objects-0123456789abcdef.outputs.restore_key }}";
        assert!(
            cache_action_step(true, &uses, "mbx-objects", key, &[], &[path.to_owned()]).is_ok()
        );
        assert!(cache_action_step(true, &uses, "sources", key, &[], &[path.to_owned()]).is_err());
        assert!(
            cache_action_step(
                true,
                &uses,
                "mbx-objects",
                "${{ steps.other.outputs.key }}",
                &[],
                &[path.to_owned()]
            )
            .is_err()
        );
        assert!(
            cache_action_step(
                true,
                &uses,
                "mbx-objects",
                key,
                &[],
                &["${{ steps.other.outputs.stage_path }}".to_owned()]
            )
            .is_err()
        );
    }
}
