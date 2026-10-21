use std::{collections::HashMap, future::Future, path::Path};

use bollard::{Docker, models::SystemInfo};
use serde_json::json;

use crate::{
    config::{HostConfig, RepositoryConnection},
    journal::StorageCleanupIdentity,
    launch::{
        SelectedEngineContext,
        inspect_tests::{BackgroundDockerStub, DockerStub, http},
    },
    worker::{WorkerContainerRole, WorkerIdentityNames},
};

pub(super) fn on_large_test_stack<F, Fut>(test: F) -> Result<(), String>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), String>> + Send + 'static,
{
    std::thread::Builder::new()
        .stack_size(8_388_608)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| {
                    format!(
                        "The readiness test could not create its Tokio runtime. Error: {error}."
                    )
                })?;
            runtime.block_on(test())
        })
        .map_err(|error| format!("The readiness test could not start its thread. Error: {error}."))?
        .join()
        .map_err(|_| "The readiness test thread failed.".to_owned())?
}

pub(super) async fn config_and_engine(
    scope: &crate::journal::ConnectionJournal,
    binding: &RepositoryConnection,
    database_path: &Path,
) -> Result<(HostConfig, SelectedEngineContext, std::path::PathBuf), String> {
    let context = scope
        .engine_context()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The fixture has no selected engine context.".to_owned())?;
    let mut config =
        HostConfig::test_with_job_authority_policy(binding).map_err(|error| error.to_string())?;
    config.docker.endpoint = context.endpoint.clone();
    config.docker.platform = context.platform.clone();
    let selected = SelectedEngineContext::capture(
        &context,
        &SystemInfo {
            id: Some(context.engine_id.clone()),
            docker_root_dir: Some("/var/lib/docker".to_owned()),
            os_type: Some("linux".to_owned()),
            architecture: Some("x86_64".to_owned()),
            ..SystemInfo::default()
        },
    )
    .map_err(|error| error.to_string())?;
    let state = database_path
        .parent()
        .ok_or_else(|| "The fixture has no state directory.".to_owned())?
        .to_path_buf();
    std::fs::write(
        state.join("host.toml"),
        config.to_toml().map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    Ok((config, selected, state))
}

pub(super) fn inventory_docker(
    binding: &RepositoryConnection,
    identity: &StorageCleanupIdentity,
    engine_id: &str,
) -> Result<InventoryDocker, String> {
    inventory_docker_with_server(binding, identity, engine_id, false)
}

pub(super) fn inventory_docker_on_thread(
    binding: &RepositoryConnection,
    identity: &StorageCleanupIdentity,
    engine_id: &str,
) -> Result<InventoryDocker, String> {
    inventory_docker_with_server(binding, identity, engine_id, true)
}

fn inventory_docker_with_server(
    binding: &RepositoryConnection,
    identity: &StorageCleanupIdentity,
    engine_id: &str,
    on_thread: bool,
) -> Result<InventoryDocker, String> {
    let names =
        WorkerIdentityNames::from_private_volume(identity.private_volume()).map_err(|error| {
            format!("The stored worker volume identity is invalid. Error: {error}.")
        })?;
    let mut responses = vec![
        http(
            200,
            &container(
                identity,
                binding,
                &names,
                WorkerContainerRole::Runner,
                "runner",
            )?,
        ),
        http(
            200,
            &container(identity, binding, &names, WorkerContainerRole::Dind, "dind")?,
        ),
    ];
    for (role, name) in identity.volumes() {
        let labels = HashMap::from([
            (
                "velnor.connection".to_owned(),
                binding.connection_id.to_string(),
            ),
            (
                "velnor.reservation".to_owned(),
                identity.reservation_id().to_string(),
            ),
            (
                "velnor.operation".to_owned(),
                identity.operation_id().to_string(),
            ),
            ("velnor.role".to_owned(), role.as_str().to_owned()),
        ]);
        let volume = bollard::models::Volume {
            name: name.clone(),
            driver: "local".to_owned(),
            mountpoint: format!("/var/lib/docker/volumes/{name}/_data"),
            created_at: None,
            status: None,
            labels,
            scope: Some(bollard::models::VolumeScopeEnum::LOCAL),
            cluster_volume: None,
            options: HashMap::new(),
            usage_data: None,
        };
        let body = serde_json::to_string(&volume).map_err(|error| error.to_string())?;
        let _: bollard::models::Volume =
            serde_json::from_str(&body).map_err(|error| error.to_string())?;
        responses.push(http(200, &body));
    }
    let info = json!({
        "ID": engine_id,
        "DockerRootDir": "/var/lib/docker",
        "OSType": "linux",
        "Architecture": "x86_64",
    })
    .to_string();
    let _: SystemInfo = serde_json::from_str(&info).map_err(|error| error.to_string())?;
    responses.push(http(200, &info));
    if on_thread {
        Ok(InventoryDocker::Threaded(BackgroundDockerStub::open(
            responses,
        )?))
    } else {
        Ok(InventoryDocker::Runtime(DockerStub::open(responses)?))
    }
}

pub(super) enum InventoryDocker {
    Runtime(DockerStub),
    Threaded(BackgroundDockerStub),
}

impl InventoryDocker {
    pub(super) fn client(&self) -> &Docker {
        match self {
            Self::Runtime(server) => &server.docker,
            Self::Threaded(server) => &server.docker,
        }
    }

    pub(super) async fn finish_observed(self) -> Result<Vec<String>, String> {
        match self {
            Self::Runtime(server) => server.finish_observed().await,
            Self::Threaded(server) => server.finish_observed().await,
        }
    }

    pub(super) fn observed_request_summary(&self) -> Result<String, String> {
        match self {
            Self::Runtime(server) => server.observed_request_summary(),
            Self::Threaded(server) => server.observed_request_summary(),
        }
    }
}

fn container(
    identity: &StorageCleanupIdentity,
    binding: &RepositoryConnection,
    names: &WorkerIdentityNames,
    role: WorkerContainerRole,
    role_label: &str,
) -> Result<String, String> {
    let (id, name) = match role {
        WorkerContainerRole::Runner => (identity.runner_container_id(), names.container_name(role)),
        WorkerContainerRole::Dind => (identity.dind_container_id(), names.container_name(role)),
    };
    let labels = json!({
        "io.velnor.managed": "true",
        "io.velnor.reservation-id": identity.reservation_id().to_string(),
        "io.velnor.operation-id": identity.operation_id().to_string(),
        "io.velnor.connection-id": binding.connection_id.as_str(),
        "io.velnor.role": role_label,
    });
    let body = json!({
        "Id": id,
        "Name": format!("/{name}"),
        "Config": {"Labels": labels},
        "State": {"Running": true}
    })
    .to_string();
    let _: bollard::models::ContainerInspectResponse =
        serde_json::from_str(&body).map_err(|error| error.to_string())?;
    Ok(body)
}
