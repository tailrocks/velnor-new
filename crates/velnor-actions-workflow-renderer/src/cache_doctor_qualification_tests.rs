use super::*;

fn qualified_cache_action(
    save: bool,
    path_layer: QualificationCacheLayer,
    condition: Option<&str>,
) -> Step {
    let layer = QualificationCacheLayer::CargoSources;
    let bind = QualificationCacheStepId::new(QualificationCacheStepStage::Bind, layer, "rust-a")
        .expect("correct owner");
    let key_output = if save { "save_key" } else { "restore_key" };
    let path_output = if save {
        "save_stage_path"
    } else {
        "restore_stage_path"
    };
    let path_bind =
        QualificationCacheStepId::new(QualificationCacheStepStage::Bind, path_layer, "rust-a")
            .expect("correct owner");
    let key = format!("${{{{ steps.{}.outputs.{key_output} }}}}", bind.as_string());
    let path = format!(
        "${{{{ steps.{}.outputs.{path_output} }}}}",
        path_bind.as_string()
    );
    let mut step = action(
        if save {
            "Save qualification cache cargo_sources"
        } else {
            "Restore qualification cache cargo_sources"
        },
        if save { SAVE } else { RESTORE },
        &[("key", &key), ("path", &path), ("restore-keys", "")],
        condition,
    );
    let StepKind::Action { with, .. } = &mut step.kind else {
        unreachable!("cache actions are actions");
    };
    if save {
        with.remove("restore-keys");
        step.role = Some(velnor_actions_contract::StepRole::QualificationCacheSave);
    } else {
        step.role = Some(velnor_actions_contract::StepRole::QualificationCacheRestore);
    }
    step
}

fn owner_cache_save_gate() -> String {
    let bind = QualificationCacheStepId::new(
        QualificationCacheStepStage::Bind,
        QualificationCacheLayer::CargoSources,
        "rust-a",
    )
    .expect("correct owner")
    .as_string();
    let export = bind.replacen(
        "qualification-cache-bind-",
        "qualification-cache-export-",
        1,
    );
    format!(
        "needs.plan.outputs.qualification_campaign != '' && steps.{bind}.outputs.active == 'true' && steps.{export}.outputs.save_requested == 'true' && steps.{bind}.outputs.save_key != ''"
    )
}

#[test]
fn doctor_accepts_only_typed_owner_cache_outputs() {
    let mut jobs = valid();
    let reader = jobs.get_mut("rust-a").expect("reader");
    reader.steps.push(qualified_cache_action(
        false,
        QualificationCacheLayer::CargoSources,
        None,
    ));
    reader.steps.push(qualified_cache_action(
        true,
        QualificationCacheLayer::CargoSources,
        Some(&owner_cache_save_gate()),
    ));
    check(&jobs).expect("The cache outputs and the save condition must be correct.");
}

#[test]
fn doctor_rejects_owner_cache_stage_path_and_save_gate_drift() {
    let mut wrong_path = valid();
    wrong_path
        .get_mut("rust-a")
        .expect("reader")
        .steps
        .push(qualified_cache_action(
            false,
            QualificationCacheLayer::MbxObjects,
            None,
        ));
    rejects(&wrong_path, "i2_path_not_from_spec");

    let mut wrong_gate = valid();
    wrong_gate
        .get_mut("rust-a")
        .expect("reader")
        .steps
        .push(qualified_cache_action(
            true,
            QualificationCacheLayer::CargoSources,
            Some("success()"),
        ));
    rejects(&wrong_gate, "i7_save_not_trust_gated");
}

#[test]
fn doctor_rejects_wrong_owner_or_non_bind_key_outputs() {
    let mut wrong_owner = valid();
    let step = qualified_cache_action(false, QualificationCacheLayer::CargoSources, None);
    let wrong_bind = QualificationCacheStepId::new(
        QualificationCacheStepStage::Bind,
        QualificationCacheLayer::CargoSources,
        "rust-other",
    )
    .expect("correct owner")
    .as_string();
    replace_action_input(
        &mut wrong_owner,
        step,
        "key",
        &format!("${{{{ steps.{wrong_bind}.outputs.restore_key }}}}"),
    );
    rejects(&wrong_owner, "i2_path_not_from_spec");

    let mut wrong_stage = valid();
    let step = qualified_cache_action(false, QualificationCacheLayer::CargoSources, None);
    let export = QualificationCacheStepId::new(
        QualificationCacheStepStage::Export,
        QualificationCacheLayer::CargoSources,
        "rust-a",
    )
    .expect("correct owner")
    .as_string();
    replace_action_input(
        &mut wrong_stage,
        step,
        "key",
        &format!("${{{{ steps.{export}.outputs.restore_key }}}}"),
    );
    rejects(&wrong_stage, "unknown_family");
}

fn replace_action_input(jobs: &mut BTreeMap<String, Job>, mut step: Step, name: &str, value: &str) {
    let StepKind::Action { with, .. } = &mut step.kind else {
        unreachable!("test cache step is an action");
    };
    with.insert(name.to_owned(), value.to_owned());
    jobs.get_mut("rust-a").expect("reader").steps.push(step);
}
