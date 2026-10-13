//! Typed, secret-free values at the Docker create boundary.

use bollard::models::ContainerCreateBody;
use bollard::query_parameters::CreateContainerOptions;

use crate::docker_spec::Mount;

use super::ResourceBudget;

/// One Docker create. Not a bollard type. JIT is not a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateProjection {
    /// Deterministic per-worker container name used for crash recovery.
    pub name: String,
    /// Image reference.
    pub image: String,
    /// OCI platform. Always `linux/amd64`.
    pub platform: String,
    /// Env pairs. Empty when the image environment stands.
    pub env: Vec<String>,
    /// Command. Empty when the image entrypoint stands.
    pub cmd: Vec<String>,
    /// Expected image entrypoint. It is checked during reconciliation, not sent to Docker.
    pub entrypoint: Vec<String>,
    /// Expected image user. An empty image value means the image default.
    pub user: Option<String>,
    /// Expected image working directory. An empty image value means the image default.
    pub working_dir: Option<String>,
    /// `key=value` labels. No JIT.
    pub labels: Vec<String>,
    /// Mounts. Volume sources use `volume:<name>`.
    pub mounts: Vec<Mount>,
    /// Private host binds. The action archive bind is read-only.
    pub bind_mounts: Vec<BindMount>,
    /// Host privilege. False for the runner. True only for private `DinD`.
    pub privileged: bool,
    /// `OpenStdin`. True only for the runner channel.
    pub open_stdin: bool,
    /// `container:<id>` joins that container's network namespace. Runner only.
    pub network_mode: Option<String>,
    /// Validated CPU and memory budget required by every Docker create.
    pub(crate) resource_budget: ResourceBudget,
}

/// One controller-owned host bind in a runner create projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindMount {
    /// Controller-owned source path.
    pub source: String,
    /// Container path.
    pub target: String,
    /// Whether the container may write to the source.
    pub read_only: bool,
}

/// Ids this call created. No JIT and no name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    /// Private `DinD` container id.
    pub dind_id: String,
    /// Runner container id.
    pub runner_id: String,
}

/// Bollard create inputs. Platform is on `options` and on [`CreateProjection`].
///
/// Bollard 0.21.1 has no platform field on [`ContainerCreateBody`].
#[derive(Debug, Clone, PartialEq)]
pub struct BollardCreate {
    /// Query options for `create_container`, including platform.
    pub options: CreateContainerOptions,
    /// Body for `create_container`. No JIT.
    pub config: ContainerCreateBody,
}
