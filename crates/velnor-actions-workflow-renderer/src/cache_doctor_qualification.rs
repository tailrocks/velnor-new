use velnor_actions_contract::workflow::QualificationCacheLayer;
use velnor_actions_contract::{Step, StepKind};

pub(super) fn layer_for_key(key: &str) -> Option<QualificationCacheLayer> {
    cache_bind_output(key, "restore_key")
        .or_else(|| cache_bind_output(key, "save_key"))
        .map(|(layer, _)| layer)
}

fn cache_bind_output<'a>(
    expression: &'a str,
    expected_output: &str,
) -> Option<(QualificationCacheLayer, &'a str)> {
    let inner = expression.strip_prefix("${{ steps.")?.strip_suffix(" }}")?;
    let (id, output) = inner.split_once(".outputs.")?;
    if output != expected_output {
        return None;
    }
    let suffix = id.strip_prefix("qualification-cache-bind-")?;
    let (layer, tag) = suffix.rsplit_once('-')?;
    let layer = match layer {
        "mbx_objects" => QualificationCacheLayer::MbxObjects,
        "cargo_sources" => QualificationCacheLayer::CargoSources,
        "mise_tools" => QualificationCacheLayer::MiseTools,
        "tofu_providers" => QualificationCacheLayer::TofuProviders,
        "task_result" => QualificationCacheLayer::TaskResult,
        _ => return None,
    };
    (tag.len() == 16
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then_some((layer, id))
}

pub(super) fn action_paths(job: &str, step: &Step, path: &str, save: bool) -> bool {
    let StepKind::Action { with, .. } = &step.kind else {
        return false;
    };
    let key_output = if save { "save_key" } else { "restore_key" };
    let path_output = if save {
        "save_stage_path"
    } else {
        "restore_stage_path"
    };
    let Some((layer, bind_id)) = with
        .get("key")
        .and_then(|key| cache_bind_output(key, key_output))
    else {
        return false;
    };
    if !bind_id_matches_job(bind_id, layer, job) {
        return false;
    }
    let expected_path = format!("${{{{ steps.{bind_id}.outputs.{path_output} }}}}");
    let restore_keys_ok = if save {
        !with.contains_key("restore-keys")
    } else {
        with.get("restore-keys").is_some_and(String::is_empty)
    };
    path == expected_path
        && restore_keys_ok
        && step.role
            == Some(if save {
                velnor_actions_contract::StepRole::QualificationCacheSave
            } else {
                velnor_actions_contract::StepRole::QualificationCacheRestore
            })
}

pub(super) fn save_gate(job: &str, step: &Step, condition: &str) -> bool {
    let StepKind::Action { with, .. } = &step.kind else {
        return false;
    };
    let Some((layer, bind_id)) = with
        .get("key")
        .and_then(|key| cache_bind_output(key, "save_key"))
    else {
        return false;
    };
    if !bind_id_matches_job(bind_id, layer, job) {
        return false;
    }
    let Some(suffix) = bind_id.strip_prefix("qualification-cache-bind-") else {
        return false;
    };
    let expected_export = format!("qualification-cache-export-{suffix}");
    let expected = format!(
        "needs.plan.outputs.qualification_campaign != '' && steps.{bind_id}.outputs.active == 'true' && steps.{expected_export}.outputs.save_requested == 'true' && steps.{bind_id}.outputs.save_key != ''"
    );
    condition == expected
        || condition
            == format!(
                "({expected}) && {}",
                crate::dispatch_cache_boundary::DISPATCH_DENY
            )
}

fn bind_id_matches_job(bind_id: &str, layer: QualificationCacheLayer, job: &str) -> bool {
    velnor_actions_contract::workflow::QualificationCacheStepId::new(
        velnor_actions_contract::workflow::QualificationCacheStepStage::Bind,
        layer,
        job,
    )
    .is_ok_and(|expected| expected.as_string() == bind_id)
}
