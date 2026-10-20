use velnor_actions_contract::workflow::QualificationCacheLayer;

pub(crate) fn is_composite_boundary_output(key: &str, inner: &str) -> bool {
    for layer in [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ] {
        if key
            == format!(
                "VELNOR_QUALIFICATION_CACHE_LAYER_{}_EVIDENCE",
                layer.as_str().to_ascii_uppercase()
            )
            && is_typed_composite_output(
                inner,
                "after-save",
                layer,
                &format!("cache_{}_owner_evidence_path", layer.as_str()),
            )
        {
            return true;
        }
    }
    if !key.starts_with("cache_") {
        return false;
    }
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
            .any(|output| {
                key == output && is_typed_composite_output(inner, "before-save", layer, output)
            })
    })
}

pub(crate) fn is_typed_save_outcome_input(key: &str, inner: &str) -> bool {
    [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ]
    .into_iter()
    .any(|layer| {
        key == format!("cache_{}_save_action_outcome", layer.as_str())
            && is_typed_step_output(inner, "save", layer, "outcome")
    })
}

fn is_typed_step_output(
    inner: &str,
    stage: &str,
    layer: QualificationCacheLayer,
    output: &str,
) -> bool {
    let Some(value) = inner.strip_prefix(&format!(
        "steps.qualification-cache-{stage}-{}-",
        layer.as_str()
    )) else {
        return false;
    };
    let Some((tag, found_output)) = value.split_once('.') else {
        return false;
    };
    found_output == output
        && tag.len() == 16
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(crate) fn is_typed_composite_output(
    inner: &str,
    phase: &str,
    layer: QualificationCacheLayer,
    output: &str,
) -> bool {
    let Some(value) = inner.strip_prefix("steps.qualification-cache-") else {
        return false;
    };
    let Some((tagged, found_output)) = value.split_once(".outputs.") else {
        return false;
    };
    let Some(tag) = tagged.strip_prefix(&format!("{phase}-{}-", layer.as_str())) else {
        return false;
    };
    found_output == output
        && tag.len() == 16
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(crate) fn is_prepare_composite_output(inner: &str) -> bool {
    let Some(value) = inner.strip_prefix("steps.qualification-cache-prepare-") else {
        return false;
    };
    let Some((tag, output)) = value.split_once(".outputs.") else {
        return false;
    };
    tag.len() == 16
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        && velnor_actions_contract::workflow::qualification_cache_composite_output_names()
            .iter()
            .any(|name| name == output)
}

pub(crate) fn is_composite_output_map(key: &str, inner: &str) -> bool {
    let Some(value) = inner.strip_prefix("toJSON(steps.") else {
        return false;
    };
    let Some((step, suffix)) = value.split_once(".outputs)") else {
        return false;
    };
    if !suffix.is_empty() {
        return false;
    }
    match key {
        "prepare_outputs" => valid_composite_id(step, "qualification-cache-prepare-"),
        "before_outputs" => [
            QualificationCacheLayer::MbxObjects,
            QualificationCacheLayer::CargoSources,
            QualificationCacheLayer::MiseTools,
            QualificationCacheLayer::TofuProviders,
            QualificationCacheLayer::TaskResult,
        ]
        .into_iter()
        .any(|layer| {
            valid_composite_id(
                step,
                &format!("qualification-cache-before-save-{}-", layer.as_str()),
            )
        }),
        _ => false,
    }
}

fn valid_composite_id(step: &str, prefix: &str) -> bool {
    step.strip_prefix(prefix).is_some_and(|tag| {
        tag.len() == 16
            && tag
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

pub(crate) fn is_prepare_save_output(key: &str, inner: &str) -> bool {
    if !matches!(key, "key" | "path") || !is_prepare_composite_output(inner) {
        return false;
    }
    let Some((_, output)) = inner.split_once(".outputs.") else {
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
        output
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
