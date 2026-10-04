use std::process::Command;

use super::*;

pub(super) fn assert_observer_scope_and_order(jobs: &[(String, Yaml)]) {
    let observer = job(jobs, Role::ObserverShared.id());
    let prepare_dirs = step(observer, "Prepare private MBX receipt directories");
    assert!(string(field(prepare_dirs, "run")).contains("mkdir -m 700"));
    let api_step = step(observer, "Validate MBX parallel REST timestamps");
    assert_observer_api_inputs(api_step);
    assert_eq!(
        string(field(map_fields(field(observer, "permissions")), "actions")),
        "read"
    );
    let Yaml::Seq(observer_steps) = field(observer, "steps") else {
        panic!("steps must be a sequence");
    };
    assert_observer_step_order(observer_steps);
    assert_api_step_is_single_observer(jobs);
    assert_token_env_is_scoped(jobs);
}

fn assert_observer_api_inputs(api_step: &Yaml) {
    assert_eq!(string(field(api_step, "shell")), "bash");
    let api_env = map_fields(field(api_step, "env"));
    assert_eq!(string(field(api_env, "GH_TOKEN")), "${{ github.token }}");
    for (key, value) in [
        (
            "MBX_PARALLEL_OBSERVER_PRIMARY",
            "${{ steps.mbx-bundle-key.outputs.primary }}",
        ),
        (
            "MBX_PARALLEL_OBSERVER_RESTORE_PRIMARY",
            "${{ steps.mbx-bundle.outputs.cache-primary-key }}",
        ),
        (
            "MBX_PARALLEL_OBSERVER_CACHE_HIT",
            "${{ steps.mbx-bundle.outputs.cache-hit }}",
        ),
        (
            "MBX_PARALLEL_OBSERVER_MATCHED_KEY",
            "${{ steps.mbx-bundle.outputs.cache-matched-key }}",
        ),
        (
            "MBX_PARALLEL_OBSERVER_RESTORE_CONCLUSION",
            "${{ steps.mbx-bundle.conclusion }}",
        ),
    ] {
        assert_eq!(string(field(api_env, key)), value);
    }
}

fn assert_observer_step_order(observer_steps: &[Yaml]) {
    assert_receipt_directories_precede_downloads(observer_steps);
    let api_position = observer_steps.iter().position(|candidate| {
        string(field(map_fields(candidate), "name")) == "Validate MBX parallel REST timestamps"
    });
    let reuse_position = observer_steps.iter().position(|candidate| {
        string(field(map_fields(candidate), "name")) == "Require reused compilation"
    });
    assert!(api_position.is_some_and(|api| reuse_position.is_some_and(|reuse| api == reuse + 1)));
}

fn assert_api_step_is_single_observer(jobs: &[(String, Yaml)]) {
    for role in [
        Role::Seed,
        Role::ReaderA,
        Role::ReaderB,
        Role::NewKeyWriter,
        Role::ObserverNew,
    ] {
        assert!(
            optional_step(
                job(jobs, role.id()),
                "Validate MBX parallel REST timestamps"
            )
            .is_none()
        );
    }
}

fn assert_token_env_is_scoped(jobs: &[(String, Yaml)]) {
    for (id, fields) in jobs {
        let Yaml::Seq(steps) = field(map_fields(fields), "steps") else {
            panic!("steps must be a sequence");
        };
        for candidate in steps {
            let candidate = map_fields(candidate);
            if let Some(environment) = optional_field(candidate, "env") {
                let scoped_token = optional_field(map_fields(environment), "GH_TOKEN")
                    .is_some_and(|value| string(value) == "${{ github.token }}");
                assert_eq!(
                    scoped_token,
                    id == Role::ObserverShared.id()
                        && string(field(candidate, "name"))
                            == "Validate MBX parallel REST timestamps"
                );
            }
        }
    }
}

fn assert_receipt_directories_precede_downloads(observer_steps: &[Yaml]) {
    let prepare_position = observer_steps.iter().position(|candidate| {
        string(field(map_fields(candidate), "name")) == "Prepare private MBX receipt directories"
    });
    for receipt_step in [
        "Download MBX seed receipt",
        "Download MBX reader-a receipt",
        "Download MBX reader-b receipt",
        "Download MBX new-key-writer receipt",
    ] {
        let download_position = observer_steps
            .iter()
            .position(|candidate| string(field(map_fields(candidate), "name")) == receipt_step);
        assert!(prepare_position.is_some_and(|prepare| {
            download_position.is_some_and(|download| prepare < download)
        }));
    }
}

pub(super) fn assert_observer_script_semantics(jobs: &[(String, Yaml)]) {
    let observer = job(jobs, Role::ObserverShared.id());
    let api_step = step(observer, "Validate MBX parallel REST timestamps");
    let script = string(field(api_step, "run"));
    let bash_check = Command::new("bash")
        .args(["-n", "-c", script])
        .output()
        .expect("Bash is available for syntax validation");
    assert!(
        bash_check.status.success(),
        "Bash rejected the receipt script: {}",
        String::from_utf8_lossy(&bash_check.stderr)
    );
    assert!(script.contains('\n'));
    assert!(script.contains("stock_restore_classify_receipt"));
    assert!(script.contains("mbx-stock-restore-evidence"));
    assert!(script.contains("CLEAN_MISS"));
    assert!(script.contains("/attempts/$GITHUB_RUN_ATTEMPT/jobs"));
    assert!(script.contains("Restore MBX single bundle"));
    assert!(script.contains("Compile MBX cache probe"));
    assert!(script.contains("NOT_RUN"));
    assert!(script.contains("seed_save_not_complete_before_parallel_starts"));
    assert!(script.contains("qualification.yml@refs/heads/main"));
    assert!(script.contains("$GITHUB_WORKFLOW_REF"));
    assert!(script.contains("($seed_save.completed_at | epoch_ms)"));
    assert!(
        script
            .contains("[$restore_a.started_at, $restore_b.started_at, $restore_writer.started_at]")
    );
    assert!(
        script
            .contains("[$build_a.completed_at, $build_b.completed_at, $build_writer.completed_at]")
    );
    assert!(script.contains("$latest_start < $earliest_end"));
    assert!(script.contains("overlap_duration_ms"));
    assert!(!script.contains("workflow-runs?"));
    assert!(!script.contains("actions/runs?"));
}
