use velnor_actions_contract::workflow::QualificationCacheLayer;

use super::composite_output::is_typed_composite_output;

pub(crate) fn is_composite_input(key: &str, inner: &str) -> bool {
    let direct_input = inner.strip_prefix("inputs.");
    let prepare_field = inner
        .strip_prefix("fromJSON(inputs.prepare_outputs).")
        .filter(|field| is_declared_prepare_field(field));
    let before_field = inner
        .strip_prefix("fromJSON(inputs.before_outputs).")
        .filter(|field| is_declared_before_field(field));
    if key == "VELNOR_QUALIFICATION_CACHE_SAVE_ACTION" {
        return [
            "cache_mbx_objects",
            "cache_cargo_sources",
            "cache_mise_tools",
            "cache_tofu_providers",
            "cache_task_result",
        ]
        .iter()
        .any(|prefix| {
            inner
                == format!(
                    "fromJSON(inputs.before_outputs).{prefix}_save_requested == 'true' && inputs.{prefix}_save_action_outcome || ''"
                )
        });
    }
    if matches!(
        (key, direct_input.unwrap_or("")),
        ("VELNOR_CACHE_STARTED", "cache_clock_started")
            | ("VELNOR_CACHE_DEADLINE", "cache_clock_deadline")
            | ("VELNOR_CACHE_JOB_SECONDS", "cache_clock_job_seconds")
    ) {
        return true;
    }
    cache_layers().into_iter().any(|layer| {
        is_layer_composite_input(key, inner, layer, direct_input, prepare_field, before_field)
    })
}

fn is_layer_composite_input(
    key: &str,
    inner: &str,
    layer: QualificationCacheLayer,
    direct_input: Option<&str>,
    prepare_field: Option<&str>,
    before_field: Option<&str>,
) -> bool {
    let prefix = format!("cache_{}", layer.as_str());
    let evidence_key = format!(
        "VELNOR_QUALIFICATION_CACHE_LAYER_{}_EVIDENCE",
        layer.as_str().to_ascii_uppercase()
    );
    if key == evidence_key
        && is_typed_composite_output(
            inner,
            "after-save",
            layer,
            &format!("{prefix}_owner_evidence_path"),
        )
    {
        return true;
    }
    if key == "VELNOR_QUALIFICATION_RUNTIME_IDENTITY_FILE"
        && ["runtime_before", "runtime_after"].iter().any(|field| {
            let expected = format!("{prefix}_{field}");
            direct_input.is_some_and(|input| input == expected)
                || prepare_field.is_some_and(|mapped| mapped == expected)
                || before_field.is_some_and(|mapped| mapped == expected)
        })
    {
        return true;
    }
    let Some(field) = composite_env_field(key) else {
        return false;
    };
    let expected = format!("{prefix}_{field}");
    let matches = direct_input.is_some_and(|input| input == expected)
        || prepare_field.is_some_and(|mapped| mapped == expected)
        || before_field.is_some_and(|mapped| mapped == expected);
    if !matches {
        return false;
    }
    let observed = prepare_field
        .or(before_field)
        .or(direct_input)
        .unwrap_or_default();
    !observed.ends_with("_runtime_after") || layer != QualificationCacheLayer::TaskResult
}

fn composite_env_field(key: &str) -> Option<String> {
    let direct = match key {
        "VELNOR_QUALIFICATION_RUNTIME_IDENTITY_FILE" => return None,
        "VELNOR_QUALIFICATION_CACHE_RUNTIME_BEFORE_FILE" => "runtime_before",
        "VELNOR_QUALIFICATION_CACHE_RUNTIME_AFTER_FILE" => "runtime_after",
        "VELNOR_QUALIFICATION_CACHE_ROOTS_FILE" => "roots",
        "VELNOR_QUALIFICATION_CACHE_ACTIVE" => "active",
        "VELNOR_QUALIFICATION_CACHE_SAVE_KEY" | "VELNOR_QUALIFICATION_CACHE_KEY" => "save_key",
        "VELNOR_QUALIFICATION_CACHE_SAVE_STAGE_PATH" => "save_stage_path",
        "VELNOR_QUALIFICATION_CACHE_RESTORE_RESULT" => "restore_result",
        "VELNOR_QUALIFICATION_CACHE_EXPORT_EXPORT_RESULT" => "export_result",
        "VELNOR_QUALIFICATION_CACHE_EXPORT_STATE_DIGEST" => "state_digest",
        "VELNOR_QUALIFICATION_CACHE_EXPORT_SAVE_REQUESTED" => "save_requested",
        _ => return census_env_field(key),
    };
    Some(direct.to_owned())
}

fn census_env_field(key: &str) -> Option<String> {
    let (group, suffix) = if let Some(suffix) =
        key.strip_prefix("VELNOR_QUALIFICATION_CACHE_RESTORE_CENSUS_")
    {
        ("restore", suffix)
    } else if let Some(suffix) = key.strip_prefix("VELNOR_QUALIFICATION_CACHE_SAVE_BEFORE_CENSUS_")
    {
        ("save_before", suffix)
    } else {
        let suffix = key.strip_prefix("VELNOR_QUALIFICATION_CACHE_SAVE_AFTER_CENSUS_")?;
        ("save_after", suffix)
    };
    let field = match suffix {
        "PHASE" => "phase",
        "KEY" => "key",
        "STATUS" => "status",
        "ADMITTED" => "admitted",
        "CACHE_ID" => "cache_id",
        "CACHE_KEY" => "cache_key",
        "CACHE_REF" => "cache_ref",
        "CACHE_VERSION" => "cache_version",
        "CACHE_SIZE" => "cache_size",
        "ACTION_HIT" if group == "restore" => "action_hit",
        "ACTION_MATCHED_KEY" if group == "restore" => "action_matched_key",
        "RESTORE_KEY" if group == "restore" => "restore_key",
        _ => return None,
    };
    Some(format!("{group}_census_{field}"))
}

fn cache_layers() -> [QualificationCacheLayer; 5] {
    [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ]
}

fn is_declared_prepare_field(field: &str) -> bool {
    velnor_actions_contract::workflow::qualification_cache_composite_output_names()
        .iter()
        .any(|name| name == field)
}

fn is_declared_before_field(field: &str) -> bool {
    [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ]
    .into_iter()
    .any(|layer| {
        velnor_actions_contract::workflow::qualification_cache_before_save_output_names(layer)
            .iter()
            .any(|name| name == field)
    })
}

pub(crate) fn is_composite_with_input(key: &str, inner: &str) -> bool {
    if !matches!(key, "key" | "path") {
        return false;
    }
    let Some(input) = inner.strip_prefix("inputs.") else {
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
        input
            == format!(
                "cache_{}_{}",
                layer.as_str(),
                if key == "key" {
                    "save_key"
                } else {
                    "save_stage_path"
                }
            )
    })
}
