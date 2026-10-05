//! MBX cache ownership and version compatibility regressions.

use std::collections::BTreeMap;
use std::error::Error;

use crate::cache_steps::{MBX_ACTION_CACHE_MODE, MBX_ACTION_NAME, MBX_CACHE_MODE_ENV};
use velnor_actions_contract::workflow::timeout::JobTimeout;
use velnor_actions_contract::{Job, Step, StepKind};

use super::store::{SCALE_SET_ONLY_IF, STORE_INIT_SCRIPT};
use super::{
    MBX_BUNDLE_EXPORT_NAME, MBX_BUNDLE_IMPORT_NAME, MBX_BUNDLE_KEY_NAME, MBX_BUNDLE_PATH,
    MBX_BUNDLE_RESTORE_NAME, MBX_BUNDLE_SAVE_NAME, MBX_RESTORE_NAME, MBX_STORE_INIT_NAME,
    append_single_bundle_saves, lane,
};

const ACTION_V1_6: &str = "1687e54eb349cadf61fa38b5813a77875489e8e6";
const HOSTED_BACKEND: &str = "${{ runner.environment == 'github-hosted' && 'github' || 'local' }}";
const HOSTED_CACHE_MODE: &str = "${{ runner.environment == 'github-hosted' && github.event_name == 'push' && 'write' || 'read' }}";

fn mbx_job() -> Job {
    Job {
        display_name: "MBX job".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![Step {
            name: MBX_RESTORE_NAME.to_owned(),
            condition: None,
            kind: StepKind::Action {
                uses: format!("{MBX_ACTION_NAME}@{ACTION_V1_6}"),
                with: BTreeMap::from([
                    ("github-cache-mode".to_owned(), "objects".to_owned()),
                    ("toolchain".to_owned(), "1.98.1".to_owned()),
                    (
                        "cache-generation".to_owned(),
                        "velnor-mbx-1.21.1".to_owned(),
                    ),
                ]),
                env: BTreeMap::from([(
                    MBX_CACHE_MODE_ENV.to_owned(),
                    MBX_ACTION_CACHE_MODE.to_owned(),
                )]),
            },
        }],
    }
}

fn job_with_bundle_route() -> Result<Job, Box<dyn Error>> {
    let mut jobs = BTreeMap::from([("demo".to_owned(), mbx_job())]);
    append_single_bundle_saves(&mut jobs)?;
    jobs.remove("demo")
        .ok_or_else(|| std::io::Error::other("rendered MBX job missing").into())
}

fn matrix_job_with_bundle_route() -> Result<Job, Box<dyn Error>> {
    let mut job = mbx_job();
    job.steps.push(Step {
        name: "Matrix marker".to_owned(),
        condition: None,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::from([(
                crate::matrix::MATRIX_NEEDS_JOB_ENV.to_owned(),
                "1".to_owned(),
            )]),
        },
    });
    let mut jobs = BTreeMap::from([(crate::render::TASK_JOB_ID.to_owned(), job)]);
    append_single_bundle_saves(&mut jobs)?;
    jobs.remove(crate::render::TASK_JOB_ID)
        .ok_or_else(|| std::io::Error::other("matrix MBX job missing").into())
}

#[test]
fn v16_action_routes_hosted_cache_to_github_and_scaleset_to_manual_bundle()
-> Result<(), Box<dyn Error>> {
    let job = job_with_bundle_route()?;
    let action_index = job
        .steps
        .iter()
        .position(|step| step.name == MBX_RESTORE_NAME)
        .ok_or_else(|| std::io::Error::other("MBX action missing"))?;
    let StepKind::Action { uses, with, env } = &job.steps[action_index].kind else {
        return Err(std::io::Error::other("MBX restore is not an action").into());
    };
    assert_eq!(uses, &format!("{MBX_ACTION_NAME}@{ACTION_V1_6}"));
    assert_eq!(
        with.get("backend").map(String::as_str),
        Some(HOSTED_BACKEND)
    );
    assert!(!with.contains_key("isolate-objects-cache"));
    assert_eq!(
        env.get(MBX_CACHE_MODE_ENV).map(String::as_str),
        Some(HOSTED_CACHE_MODE)
    );

    let init = job
        .steps
        .iter()
        .find(|step| step.name == MBX_STORE_INIT_NAME)
        .ok_or_else(|| std::io::Error::other("private store init missing"))?;
    assert_eq!(init.condition.as_deref(), Some(SCALE_SET_ONLY_IF));
    assert!(matches!(
        &init.kind,
        StepKind::Shell { run, env }
            if env.get("MBX_MATRIX_KEY").is_some_and(String::is_empty)
                && run.get(2).is_some_and(|script| script.contains("MBX_CACHE_EXPORT_GROUP=%s"))
                && run.get(2).is_some_and(|script| script.contains("$GITHUB_RUN_ID"))
                && run.get(2).is_some_and(|script| script.contains("$GITHUB_JOB"))
    ));
    let key = job
        .steps
        .iter()
        .find(|step| step.name == MBX_BUNDLE_KEY_NAME)
        .ok_or_else(|| std::io::Error::other("manual MBX key step missing"))?;
    let key_condition = key
        .condition
        .as_deref()
        .ok_or_else(|| std::io::Error::other("manual MBX key step must be gated"))?;
    assert!(key_condition.starts_with(SCALE_SET_ONLY_IF));
    assert!(key_condition.contains("steps.mbx-store-init.outputs.ready == 'true'"));
    for (name, required) in [
        (MBX_BUNDLE_RESTORE_NAME, "steps.mbx-bundle-key.outcome"),
        (MBX_BUNDLE_IMPORT_NAME, "steps.mbx-bundle.outcome"),
    ] {
        let step = job
            .steps
            .iter()
            .find(|step| step.name == name)
            .ok_or_else(|| std::io::Error::other("manual MBX restore step missing"))?;
        let condition = step
            .condition
            .as_deref()
            .ok_or_else(|| std::io::Error::other("manual MBX step must be gated"))?;
        assert!(condition.starts_with(SCALE_SET_ONLY_IF));
        assert!(condition.contains(required), "{condition}");
        if name == MBX_BUNDLE_IMPORT_NAME {
            assert!(condition.contains("steps.mbx-bundle-key.outputs.ready == 'true'"));
        }
    }
    for name in [MBX_BUNDLE_EXPORT_NAME, MBX_BUNDLE_SAVE_NAME] {
        let step = job
            .steps
            .iter()
            .find(|step| step.name == name)
            .ok_or_else(|| std::io::Error::other("manual MBX writer step missing"))?;
        assert!(
            step.condition
                .as_deref()
                .is_some_and(|condition| condition.starts_with(SCALE_SET_ONLY_IF))
        );
    }
    let restore_path = match &job
        .steps
        .iter()
        .find(|step| step.name == MBX_BUNDLE_RESTORE_NAME)
        .ok_or_else(|| std::io::Error::other("bundle restore missing"))?
        .kind
    {
        StepKind::Action { with, .. } => with.get("path"),
        _ => None,
    };
    let save_path = match &job
        .steps
        .iter()
        .find(|step| step.name == MBX_BUNDLE_SAVE_NAME)
        .ok_or_else(|| std::io::Error::other("bundle save missing"))?
        .kind
    {
        StepKind::Action { with, .. } => with.get("path"),
        _ => None,
    };
    assert_eq!(restore_path.map(String::as_str), Some(MBX_BUNDLE_PATH));
    assert_eq!(save_path, restore_path);
    assert!(!STORE_INIT_SCRIPT.is_empty());
    Ok(())
}

#[test]
fn generated_export_recognizer_requires_one_exact_full_step() -> Result<(), Box<dyn Error>> {
    let job = job_with_bundle_route()?;
    let export = job
        .steps
        .iter()
        .find(|step| step.name == MBX_BUNDLE_EXPORT_NAME)
        .ok_or_else(|| std::io::Error::other("generated MBX export missing"))?;
    assert!(super::is_exact_generated_export_step(&job, export));

    let mut changed_name = export.clone();
    changed_name.name.push_str(" altered");
    assert!(!super::is_exact_generated_export_step(&job, &changed_name));

    let mut changed_body = export.clone();
    let StepKind::Shell { run, .. } = &mut changed_body.kind else {
        return Err(std::io::Error::other("generated MBX export is not a shell step").into());
    };
    run.get_mut(2)
        .ok_or_else(|| std::io::Error::other("generated MBX export body missing"))?
        .push(' ');
    assert!(!super::is_exact_generated_export_step(&job, &changed_body));

    let mut changed_env = export.clone();
    let StepKind::Shell { env, .. } = &mut changed_env.kind else {
        return Err(std::io::Error::other("generated MBX export is not a shell step").into());
    };
    env.insert("UNEXPECTED".to_owned(), "value".to_owned());
    assert!(!super::is_exact_generated_export_step(&job, &changed_env));

    let mut changed_condition = export.clone();
    changed_condition.condition = Some("always()".to_owned());
    assert!(!super::is_exact_generated_export_step(
        &job,
        &changed_condition
    ));

    let mut changed_kind = export.clone();
    changed_kind.kind = StepKind::Action {
        uses: "example/action@0000000000000000000000000000000000000000".to_owned(),
        with: BTreeMap::new(),
        env: BTreeMap::new(),
    };
    assert!(!super::is_exact_generated_export_step(&job, &changed_kind));

    let mut duplicate = job.clone();
    duplicate.steps.push(export.clone());
    assert!(!super::is_exact_generated_export_step(&duplicate, export));
    Ok(())
}

#[test]
fn matrix_store_initializer_exports_matrix_scoped_producer_group() -> Result<(), Box<dyn Error>> {
    let job = matrix_job_with_bundle_route()?;
    let init = job
        .steps
        .iter()
        .find(|step| step.name == MBX_STORE_INIT_NAME)
        .ok_or_else(|| std::io::Error::other("private store init missing"))?;
    assert!(matches!(
        &init.kind,
        StepKind::Shell { run, env }
            if env.get("MBX_MATRIX_KEY").is_some_and(|value| value == "${{ matrix.matrix_key }}")
                && run.get(2).is_some_and(|script| script.contains("matrix_id=\"$matrix_key\""))
                && run.get(2).is_some_and(|script| script.contains("MBX_CACHE_EXPORT_GROUP=%s"))
    ));
    Ok(())
}

#[test]
fn only_exact_renderer_owned_mbx_scripts_allow_multiline_bash() -> Result<(), Box<dyn Error>> {
    let raw = vec![
        "bash".to_owned(),
        "-c".to_owned(),
        STORE_INIT_SCRIPT.to_owned(),
    ];
    let step = crate::steps::shell_step(MBX_STORE_INIT_NAME, raw, BTreeMap::new())?;
    let StepKind::Shell { run, .. } = &step.kind else {
        return Err(std::io::Error::other("MBX store setup is not a shell step").into());
    };
    assert!(crate::commands::validate_step_command_argv(MBX_STORE_INIT_NAME, run).is_ok());
    assert!(crate::commands::validate_command_argv(run).is_err());
    assert!(crate::commands::validate_step_command_argv("Other step", run).is_err());

    let wrapped = crate::toolchain_env::with_env_unset_argv(&[
        "bash".to_owned(),
        "-c".to_owned(),
        STORE_INIT_SCRIPT.to_owned(),
    ]);
    assert!(crate::commands::validate_step_command_argv(MBX_STORE_INIT_NAME, &wrapped).is_ok());

    let bare_env = vec![
        "env".to_owned(),
        "bash".to_owned(),
        "-c".to_owned(),
        STORE_INIT_SCRIPT.to_owned(),
    ];
    assert!(super::trusted_script_argument(MBX_STORE_INIT_NAME, &bare_env).is_none());
    assert!(crate::commands::validate_step_command_argv(MBX_STORE_INIT_NAME, &bare_env).is_err());

    let partial_unset = vec![
        "env".to_owned(),
        "-u".to_owned(),
        "GH_TOKEN".to_owned(),
        "bash".to_owned(),
        "-c".to_owned(),
        STORE_INIT_SCRIPT.to_owned(),
    ];
    assert!(super::trusted_script_argument(MBX_STORE_INIT_NAME, &partial_unset).is_none());
    assert!(
        crate::commands::validate_step_command_argv(MBX_STORE_INIT_NAME, &partial_unset).is_err()
    );

    for background_script in ["sleep 1&echo done", "sleep 1 &\techo done"] {
        let background = vec![
            "bash".to_owned(),
            "-c".to_owned(),
            background_script.to_owned(),
        ];
        assert!(super::trusted_script_argument(MBX_STORE_INIT_NAME, &background).is_none());
        assert!(
            crate::commands::validate_step_command_argv(MBX_STORE_INIT_NAME, &background).is_err()
        );
    }

    let mut script_with_extra_arg = vec![
        "bash".to_owned(),
        "-c".to_owned(),
        STORE_INIT_SCRIPT.to_owned(),
    ];
    script_with_extra_arg.push("$(untrusted)".to_owned());
    assert!(
        crate::commands::validate_step_command_argv(MBX_STORE_INIT_NAME, &script_with_extra_arg)
            .is_err()
    );

    let mut quoted_substitution_outside_body = vec![
        "bash".to_owned(),
        "-c".to_owned(),
        STORE_INIT_SCRIPT.to_owned(),
        "'\\''; $(outside-approved-body)".to_owned(),
    ];
    assert!(
        super::trusted_script_argument(MBX_STORE_INIT_NAME, &quoted_substitution_outside_body)
            .is_none()
    );
    assert!(
        crate::commands::validate_step_command_argv(
            MBX_STORE_INIT_NAME,
            &quoted_substitution_outside_body
        )
        .is_err()
    );
    quoted_substitution_outside_body.pop();

    let altered_bodies = [
        format!("{STORE_INIT_SCRIPT}; echo altered"),
        format!("{STORE_INIT_SCRIPT}\necho altered"),
        STORE_INIT_SCRIPT.replacen("umask 077", r"umask 077; echo 'x'\''y'", 1),
    ];
    for altered_body in altered_bodies {
        assert_ne!(altered_body, STORE_INIT_SCRIPT);
        let altered = vec!["bash".to_owned(), "-c".to_owned(), altered_body];
        assert!(super::trusted_script_argument(MBX_STORE_INIT_NAME, &altered).is_none());
        assert!(
            crate::commands::validate_step_command_argv(MBX_STORE_INIT_NAME, &altered).is_err()
        );
    }

    for bad_script in [
        format!("{STORE_INIT_SCRIPT}\r"),
        format!("{STORE_INIT_SCRIPT}\0"),
    ] {
        let bad = vec!["bash".to_owned(), "-c".to_owned(), bad_script];
        assert!(crate::commands::validate_step_command_argv(MBX_STORE_INIT_NAME, &bad).is_err());
    }
    let private = vec![
        "bash".to_owned(),
        "-c".to_owned(),
        "echo safe; velnor-actions __internal".to_owned(),
    ];
    assert!(matches!(
        crate::commands::validate_step_command_argv(MBX_STORE_INIT_NAME, &private),
        Err(crate::RenderError::PrivateSubcommand(_))
    ));
    Ok(())
}

#[test]
fn pinned_action_generation_matches_its_directory_cache_version() -> Result<(), Box<dyn Error>> {
    assert_eq!(
        lane::directory_cache_generation("velnor-mbx-1.21.1")?,
        "velnor-mbx-1.21.1-dir"
    );
    assert_eq!(
        lane::directory_cache_generation("velnor-mbx-1.11.9")?,
        "velnor-mbx-1.11.9"
    );
    assert!(lane::directory_cache_generation("latest").is_err());
    Ok(())
}
