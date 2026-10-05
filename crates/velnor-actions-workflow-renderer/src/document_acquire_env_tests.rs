use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, ScaleSetSelector, Step, StepKind, StepRole, Trigger,
    VELNOR_LABEL, WorkflowIr,
};

use crate::render::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP, RenderContext};
use crate::{RenderError, steps};

fn context(checkout_uses: &str) -> RenderContext {
    RenderContext {
        generator_version: "0.1.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: checkout_uses.to_owned(),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        verification_tasks: Vec::new(),
        plan_consumer_env: BTreeMap::new(),
    }
}

fn provenance(url: &str, sha: &str, commit: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (steps::ASSET_URL_ENV.to_owned(), url.to_owned()),
        (steps::ASSET_SHA_ENV.to_owned(), sha.to_owned()),
        (steps::RELEASE_COMMIT_ENV.to_owned(), commit.to_owned()),
    ])
}

fn acquire(url: &str, sha: &str, commit: &str) -> Result<Step, RenderError> {
    let staged = format!("{}/velnor-actions", steps::STAGED_BINARY_PREFIX);
    steps::acquire_velnor_step(
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!(
                "curl \"$VELNOR_ASSET_URL\" -o {staged} && echo \"$VELNOR_ASSET_SHA256\" | sha256sum -c -"
            ),
        ],
        &provenance(url, sha, commit),
    )
}

fn mbx_steps() -> Result<[Step; 3], RenderError> {
    crate::cache_steps::mbx_steps_for_driver(
        "jdx/mr-boxington-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        crate::cache_steps::CompileDriver::Mbx,
        "0.9.146",
        "1.98.1",
        BTreeMap::from([
            (
                "MISE_RUSTUP_HOME".to_owned(),
                "${{ runner.temp }}/mise/rustup".to_owned(),
            ),
            (
                "MISE_CARGO_HOME".to_owned(),
                "${{ runner.temp }}/mise/cargo".to_owned(),
            ),
            ("RUSTUP_TOOLCHAIN".to_owned(), "1.98.1".to_owned()),
        ]),
    )?
    .ok_or_else(|| RenderError::InvalidWorkflow("missing_mbx_steps".to_owned()))
}

fn hostile_environment_writer() -> Step {
    Step {
        name: "Set repository environment".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Shell {
            run: vec![
                "sh".to_owned(),
                "-c".to_owned(),
                format!(
                    "printf '%s\\n' 'VELNOR_ASSET_SHA256={}' 'VELNOR_ASSET_URL={}' 'VELNOR_RELEASE_COMMIT={}' 'VELNOR_ACQUIRE_ASSET_SHA256={}' 'VELNOR_ACQUIRE_ASSET_URL={}' 'VELNOR_ACQUIRE_RELEASE_COMMIT={}' >> \"$GITHUB_ENV\"",
                    "f".repeat(64),
                    "https://attacker.invalid/override",
                    "e".repeat(40),
                    "d".repeat(64),
                    "https://attacker.invalid/alias",
                    "c".repeat(40),
                ),
            ],
            env: BTreeMap::new(),
        },
    }
}

fn job(id: &str, runs_on: &str, steps: Vec<Step>) -> Job {
    Job {
        display_name: id.to_owned(),
        runs_on: runs_on.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::VALIDATOR,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps,
    }
}

fn workflow(jobs: BTreeMap<String, Job>) -> WorkflowIr {
    WorkflowIr {
        name: "Acquire env regression".to_owned(),
        triggers: Trigger {
            pull_request_types: Vec::new(),
            push_branches: Vec::new(),
            merge_group: false,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
        },
        jobs,
    }
}

fn field<'a>(value: &'a crate::yaml::Yaml, key: &str) -> Option<&'a crate::yaml::Yaml> {
    let crate::yaml::Yaml::Map(entries) = value else {
        return None;
    };
    entries
        .iter()
        .find(|(entry_key, _)| entry_key == key)
        .map(|(_, entry_value)| entry_value)
}

fn named_step<'a>(steps: &'a crate::yaml::Yaml, name: &str) -> Option<&'a crate::yaml::Yaml> {
    let crate::yaml::Yaml::Seq(steps) = steps else {
        return None;
    };
    steps
        .iter()
        .find(|step| field(step, "name") == Some(&crate::yaml::Yaml::str(name)))
}

fn scalar(value: &crate::yaml::Yaml) -> Option<&str> {
    match value {
        crate::yaml::Yaml::Str(value)
        | crate::yaml::Yaml::Quoted(value)
        | crate::yaml::Yaml::Annotated { value, .. } => Some(value),
        _ => None,
    }
}

fn acquire_action_file<'a>(
    shared: &'a crate::lane_share::LaneShare,
    caller: &crate::yaml::Yaml,
) -> &'a str {
    let uses = scalar(field(caller, "uses").expect("fixed acquire action ref"))
        .expect("uses is a scalar")
        .strip_prefix("./.github/actions/")
        .expect("local generated action");
    let path = format!(".github/actions/{uses}/action.yml");
    shared
        .files
        .iter()
        .find(|file| file.path == path)
        .expect("registered immutable acquire action")
        .bytes
        .as_str()
}

fn assert_rendered_acquire_action(
    shared: &crate::lane_share::LaneShare,
    rendered: &crate::yaml::Yaml,
    job_id: &str,
    url: &str,
    sha: &str,
    commit: &str,
) {
    let workflow_yaml = crate::yaml::render_yaml(rendered);
    assert!(!workflow_yaml.contains("${{ env.VELNOR_ACQUIRE_"));
    for key in [
        "VELNOR_ACQUIRE_ASSET_SHA256:",
        "VELNOR_ACQUIRE_ASSET_URL:",
        "VELNOR_ACQUIRE_RELEASE_COMMIT:",
    ] {
        assert!(!workflow_yaml.contains(key), "caller exported {key}");
    }
    let job = field(field(rendered, "jobs").expect("jobs"), job_id).expect("target job");
    assert!(
        crate::yaml::render_yaml(job).contains("GITHUB_TOKEN: \"\""),
        "acquisition still inherits the credential scrub"
    );
    let call = named_step(field(job, "steps").expect("job steps"), steps::ACQUIRE_NAME)
        .expect("acquire action call");
    assert!(
        field(call, "env").is_none(),
        "caller has no mutable env inputs"
    );
    assert!(
        field(call, "with").is_none(),
        "caller has no mutable with inputs"
    );
    let action = acquire_action_file(shared, call);
    assert!(action.contains(url), "{action}");
    assert!(action.contains(sha), "{action}");
    assert!(action.contains(commit), "{action}");
}

#[path = "document_acquire_env_unpaired_tests.rs"]
mod unpaired;

#[path = "document_acquire_env_paired_tests.rs"]
mod paired;
