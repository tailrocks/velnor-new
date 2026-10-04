//! Closed Docker action transport shared by identity and workflow generation.

use serde::Serialize;
use std::collections::BTreeMap;
use velnor_actions_actionlint::actions::{
    BUILD_PUSH_ACTION_SHA, BUILDKIT_IMAGE_DIGEST, BUILDX_VERSION, SETUP_BUILDX_ACTION_SHA,
};
use velnor_actions_contract::{
    ContractError, ProposedTask, StepId, digest_b3, matrix_id_for_task_group, matrix_key_for_id,
};

/// Output binding for the one logical validation action.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ActionBinding {
    pub(crate) id: StepId,
    pub(crate) uses: String,
}

/// Fixed action invocation; no caller-supplied YAML or action inputs.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ActionInvocation {
    pub(crate) uses: String,
    pub(crate) inputs: BTreeMap<String, String>,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) condition: Option<String>,
}

/// Exact preparation, validation, and cache authority entering task identity.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct DockerTransport {
    pub(crate) setup: ActionInvocation,
    pub(crate) read: ActionInvocation,
    pub(crate) write: ActionInvocation,
    pub(crate) buildx_version: String,
    pub(crate) buildkit_image: String,
    pub(crate) platform: String,
    pub(crate) cache_version: u8,
    pub(crate) run_mount_cache: &'static str,
    pub(crate) host_archive_cache: bool,
}

impl DockerTransport {
    /// Bind the exact fixed transport to one validated workload root.
    pub(crate) fn new(root: &str, configuration: &str) -> Result<Self, ContractError> {
        if configuration != "docker_build"
            || (root != "." && !velnor_actions_contract::config::is_valid_workload_path(root))
        {
            return Err(ContractError::identity(
                "docker_transport",
                "invalid_workload",
            ));
        }
        let platform = "linux/amd64";
        let image = format!("moby/buildkit@{BUILDKIT_IMAGE_DIGEST}");
        let env = action_env();
        let setup = ActionInvocation {
            uses: format!("docker/setup-buildx-action@{SETUP_BUILDX_ACTION_SHA}"),
            inputs: setup_inputs(&image),
            env: env.clone(),
            condition: None,
        };
        let identity = format!(
            "docker-layers-v1\n{root}\n{configuration}\n{platform}\n{BUILDX_VERSION}\n{BUILDKIT_IMAGE_DIGEST}"
        );
        let scope = format!("velnor-docker-layers-v1-{}", digest_b3(identity.as_bytes()));
        let inputs = build_inputs(root, platform, &scope);
        let uses = format!("docker/build-push-action@{BUILD_PUSH_ACTION_SHA}");
        let gate = velnor_actions_mise::cache_trust::authorize_trusted_save()
            .map_err(|error| ContractError::identity("docker_cache_trust", error.to_string()))?;
        let read = ActionInvocation {
            uses: uses.clone(),
            inputs: inputs.clone(),
            env: env.clone(),
            condition: Some("success()".to_owned()),
        };
        let mut write_inputs = inputs;
        write_inputs.insert("load".to_owned(), "false".to_owned());
        write_inputs.insert("outputs".to_owned(), "type=cacheonly".to_owned());
        write_inputs.retain(|key, _| key != "tags");
        write_inputs.insert(
            "cache-to".to_owned(),
            format!("type=gha,version=2,scope={scope},mode=max,ignore-error=true"),
        );
        let write = ActionInvocation {
            uses,
            inputs: write_inputs,
            env,
            condition: Some(gate.to_owned()),
        };
        Ok(Self {
            setup,
            read,
            write,
            buildx_version: BUILDX_VERSION.to_owned(),
            buildkit_image: image,
            platform: platform.to_owned(),
            cache_version: 2,
            run_mount_cache: "excluded",
            host_archive_cache: false,
        })
    }
}

fn action_env() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "DOCKER_CONFIG".to_owned(),
            "${{ runner.temp }}/velnor/native/docker/config".to_owned(),
        ),
        ("DOCKER_BUILD_RECORD_UPLOAD".to_owned(), "false".to_owned()),
        ("DOCKER_BUILD_SUMMARY".to_owned(), "false".to_owned()),
    ])
}

fn setup_inputs(image: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("version".to_owned(), format!("v{BUILDX_VERSION}")),
        ("driver".to_owned(), "docker-container".to_owned()),
        ("driver-opts".to_owned(), format!("image={image}")),
        (
            "buildkitd-flags".to_owned(),
            "--oci-worker-gc=true".to_owned(),
        ),
        ("cache-binary".to_owned(), "false".to_owned()),
    ])
}

fn build_inputs(root: &str, platform: &str, scope: &str) -> BTreeMap<String, String> {
    let file = if root == "." {
        "Dockerfile".to_owned()
    } else {
        format!("{root}/Dockerfile")
    };
    BTreeMap::from([
        ("context".to_owned(), root.to_owned()),
        ("file".to_owned(), file),
        ("platforms".to_owned(), platform.to_owned()),
        ("tags".to_owned(), "local-ci:validation".to_owned()),
        ("load".to_owned(), "true".to_owned()),
        ("push".to_owned(), "false".to_owned()),
        ("provenance".to_owned(), "false".to_owned()),
        ("sbom".to_owned(), "false".to_owned()),
        ("github-token".to_owned(), String::new()),
        (
            "cache-from".to_owned(),
            format!("type=gha,version=2,scope={scope}"),
        ),
    ])
}

/// Logical ID is the task's matrix identity; variant suffixes belong to rendering.
pub(crate) fn binding(task: &ProposedTask) -> Result<ActionBinding, ContractError> {
    let id = matrix_id_for_task_group(&task.stack_id, &task.task_id)?;
    let key = matrix_key_for_id(&id)?;
    Ok(ActionBinding {
        id: StepId::new(&format!("velnor-action-{key}"))?,
        uses: format!("docker/build-push-action@{BUILD_PUSH_ACTION_SHA}"),
    })
}
