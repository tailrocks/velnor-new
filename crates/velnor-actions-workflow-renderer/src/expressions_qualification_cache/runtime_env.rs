use velnor_actions_contract::workflow::{QualificationCacheLayer, QualificationCacheStepStage};

pub(crate) fn is_clock_output(key: &str, inner: &str) -> bool {
    matches!(
        (key, inner),
        (
            "cache_clock_started",
            "steps.velnor-cache-clock.outputs.started"
        ) | (
            "cache_clock_deadline",
            "steps.velnor-cache-clock.outputs.deadline"
        ) | (
            "cache_clock_job_seconds",
            "steps.velnor-cache-clock.outputs.job_seconds"
        )
    )
}

pub(crate) fn is_env_output(key: &str, inner: &str) -> bool {
    match key {
        "VELNOR_QUALIFICATION_CACHE_ROOTS_FILE" => step_output(
            inner,
            QualificationCacheStepStage::Roots,
            "root_observation_path",
        ),
        "VELNOR_QUALIFICATION_RUNTIME_IDENTITY_FILE" => [
            QualificationCacheStepStage::RuntimeIdentity,
            QualificationCacheStepStage::RuntimeIdentityAfterImport,
        ]
        .into_iter()
        .any(|stage| step_output(inner, stage, "runtime_identity_path")),
        _ => false,
    }
}

pub(crate) fn is_operation_output(key: &str, inner: &str) -> bool {
    if key == "VELNOR_QUALIFICATION_CACHE_SAVE_ACTION" {
        return is_save_outcome(inner);
    }
    let (stage, outputs): (&str, &[&str]) = match key {
        "VELNOR_QUALIFICATION_CACHE_RUNTIME_AFTER_FILE" => {
            ("runtime-identity-after-import", &["runtime_identity_path"])
        }
        "VELNOR_QUALIFICATION_CACHE_RUNTIME_BEFORE_FILE" => {
            ("runtime-identity", &["runtime_identity_path"])
        }
        "VELNOR_QUALIFICATION_CACHE_HIT" => {
            return step_output_for_stage(inner, "import", "restore_action_hit")
                || step_output_for_stage(inner, "restore", "cache-hit");
        }
        "VELNOR_QUALIFICATION_CACHE_MATCHED_KEY" => {
            return step_output_for_stage(inner, "import", "restore_action_matched_key")
                || step_output_for_stage(inner, "restore", "cache-matched-key");
        }
        "VELNOR_QUALIFICATION_CACHE_KEY" => ("bind", &["restore_key", "save_key"]),
        "VELNOR_QUALIFICATION_CACHE_RESTORE_RESULT" => ("import", &["restore_result"]),
        "VELNOR_QUALIFICATION_CACHE_EXPORT_EXPORT_RESULT" => ("export", &["export_result"]),
        "VELNOR_QUALIFICATION_CACHE_EXPORT_STATE_DIGEST" => ("export", &["state_digest"]),
        "VELNOR_QUALIFICATION_CACHE_EXPORT_SAVE_REQUESTED" => ("export", &["save_requested"]),
        "VELNOR_QUALIFICATION_CACHE_ACTIVE" => ("bind", &["active"]),
        "VELNOR_QUALIFICATION_CACHE_SAVE_KEY" => ("bind", &["save_key"]),
        key if key.starts_with("VELNOR_QUALIFICATION_CACHE_LAYER_")
            && key.ends_with("_EVIDENCE") =>
        {
            if !matches!(
                key,
                "VELNOR_QUALIFICATION_CACHE_LAYER_MBX_OBJECTS_EVIDENCE"
                    | "VELNOR_QUALIFICATION_CACHE_LAYER_CARGO_SOURCES_EVIDENCE"
                    | "VELNOR_QUALIFICATION_CACHE_LAYER_MISE_TOOLS_EVIDENCE"
                    | "VELNOR_QUALIFICATION_CACHE_LAYER_TOFU_PROVIDERS_EVIDENCE"
                    | "VELNOR_QUALIFICATION_CACHE_LAYER_TASK_RESULT_EVIDENCE"
            ) {
                return false;
            }
            ("owner-layer-evidence", &["owner_layer_evidence_path"])
        }
        _ => return false,
    };
    outputs
        .iter()
        .any(|output| step_output_for_stage(inner, stage, output))
}

pub(crate) fn is_restore_action_input(key: &str, inner: &str) -> bool {
    match key {
        "VELNOR_QUALIFICATION_CACHE_HIT" => step_output_for_stage(inner, "restore", "cache-hit"),
        "VELNOR_QUALIFICATION_CACHE_MATCHED_KEY" => {
            step_output_for_stage(inner, "restore", "cache-matched-key")
        }
        _ => false,
    }
}

fn is_save_outcome(inner: &str) -> bool {
    let Some(value) = inner.strip_prefix("steps.") else {
        return false;
    };
    let Some((export_id, tail)) = value.split_once(".outputs.save_requested == 'true' && steps.")
    else {
        return false;
    };
    let Some(save_id) = tail.strip_suffix(".outcome || ''") else {
        return false;
    };
    typed_step_id(export_id, "export")
        .is_some_and(|(layer, tag)| typed_step_id(save_id, "save") == Some((layer, tag)))
}

fn step_output_for_stage(inner: &str, stage: &str, output: &str) -> bool {
    let Some(value) = inner.strip_prefix("steps.") else {
        return false;
    };
    let Some((id, actual_output)) = value.split_once(".outputs.") else {
        return false;
    };
    actual_output == output && typed_step_id(id, stage).is_some()
}

fn typed_step_id<'a>(id: &'a str, stage: &str) -> Option<(&'static str, &'a str)> {
    let rest = id
        .strip_prefix("qualification-cache-")?
        .strip_prefix(&format!("{stage}-"))?;
    let (layer, tag) = rest.rsplit_once('-')?;
    let layer = match layer {
        "mbx_objects" => "mbx_objects",
        "cargo_sources" => "cargo_sources",
        "mise_tools" => "mise_tools",
        "tofu_providers" => "tofu_providers",
        "task_result" => "task_result",
        _ => return None,
    };
    (tag.len() == 16
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then_some((layer, tag))
}

fn step_output(inner: &str, stage: QualificationCacheStepStage, output: &str) -> bool {
    let Some(value) = inner.strip_prefix("steps.") else {
        return false;
    };
    let Some((id, actual_output)) = value.split_once(".outputs.") else {
        return false;
    };
    if actual_output != output {
        return false;
    }
    let Some(rest) = id.strip_prefix("qualification-cache-") else {
        return false;
    };
    let Some(rest) = rest.strip_prefix(&format!("{}-", stage.as_str())) else {
        return false;
    };
    [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ]
    .into_iter()
    .any(|layer| {
        rest.strip_prefix(&format!("{}-", layer.as_str()))
            .is_some_and(|tag| {
                tag.len() == 16
                    && tag
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
    })
}

pub(crate) fn is_action_output(key: &str, inner: &str) -> bool {
    if !matches!(key, "key" | "path") {
        return false;
    }
    let Some(value) = inner.strip_prefix("steps.") else {
        return false;
    };
    let Some((id, output)) = value.split_once(".outputs.") else {
        return false;
    };
    let Some(tag) = id.strip_prefix("qualification-cache-bind-") else {
        return false;
    };
    let Some((layer, hash)) = tag.rsplit_once('-') else {
        return false;
    };
    let output_allowed = if key == "key" {
        matches!(output, "restore_key" | "save_key")
    } else {
        matches!(output, "restore_stage_path" | "save_stage_path")
    };
    output_allowed
        && matches!(
            layer,
            "mbx_objects" | "cargo_sources" | "mise_tools" | "tofu_providers" | "task_result"
        )
        && hash.len() == 16
        && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}
