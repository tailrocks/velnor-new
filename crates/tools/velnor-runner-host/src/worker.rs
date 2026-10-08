//! Docker create projection for one runner and its private `DinD`.
//!
//! JIT is not a field. `start_pair` writes it on stdin and does not store it.

use crate::HostError;
use crate::stage::PairStop;
use ::bollard::Docker;
use velnor_runner_core::runner_work_path;
use velnor_runner_docker_spec::{ContainerPlan, ImageMount, Mount, audit_plan, runner_plan};

mod bollard;
mod cleanup;
mod engine;
mod inventory;
mod network;
mod profile;
mod volumes;
pub use self::bollard::{BollardCreate, bollard_create};
pub use cleanup::{
    ChildCleanupEvidence, ChildResourceInventory, ChildResourceKind, CleanupDisposition,
    CleanupLedger, CleanupStep, ContainerObservation, DiagnosticsStore, DockerCleanupEngine,
    GenerationObservation, ObservedJobIdentity, OuterContainerRole, PostActionDisposition,
    ProtectedStateDirectory, ProtectedStateDirectoryIdentity, RunnerStopEvidence, RunnerStopPolicy,
    WorkerCleanupEngine, WorkerGenerationIdentity, WorkerTerminationProof,
    cleanup_worker_generation, validate_protected_state_directory,
};
pub(crate) use engine::{create_only, deliver_jit, start_id, worker_id_for_name};
pub use inventory::{
    MAX_INVENTORY_OBJECTS_PER_KIND, MAX_INVENTORY_RESPONSE_BYTES, OwnedDockerResource,
    OwnedDockerResourceKind, list_owned_docker_resources_until,
};
pub use network::{
    OuterNetworkCleanupEngine, OuterNetworkCleanupLedger, OuterNetworkRemovalReceipt,
    cleanup_provisioning_network,
};
pub use network::{
    WorkerNetworkFailure, WorkerNetworkPlan, ensure_worker_network, remove_worker_network,
};
pub use profile::{
    dind_create_for_profile, start_pair_with_profile, start_pair_with_profile_and_sink,
    start_pair_with_sink,
};
pub use volumes::worker_volume_names;
pub(crate) use volumes::{create_named_volumes, remove_worker_volumes};

const PLATFORM: &str = "linux/amd64";
const DIND_IMAGE: &str = "velnor-dind:29.8.2";
const IDENTITY_HEX: &[u8; 16] = b"0123456789abcdef";

/// Generate a collision-resistant worker volume base for one journal row.
///
/// # Errors
///
/// Returns [`HostError::Identity`] when the random source is unavailable.
pub fn new_worker_volume() -> Result<String, HostError> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|_| HostError::Identity)?;
    let mut name = String::with_capacity(33);
    name.push('w');
    for byte in random {
        push_nibble(&mut name, byte >> 4)?;
        push_nibble(&mut name, byte & 0x0f)?;
    }
    Ok(name)
}

fn push_nibble(name: &mut String, nibble: u8) -> Result<(), HostError> {
    let index = usize::from(nibble);
    let digit = IDENTITY_HEX
        .get(index)
        .copied()
        .ok_or(HostError::Identity)?;
    name.push(char::from(digit));
    Ok(())
}

/// One Docker create. Not a bollard type. JIT is not a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateProjection {
    /// Deterministic per-worker container name used for crash recovery.
    pub name: String,
    /// Image reference.
    pub image: String,
    /// OCI platform. Always `linux/amd64`.
    pub platform: String,
    /// Mount the container root filesystem read-only.
    pub readonly_rootfs: bool,
    /// Env pairs. Empty when the image environment stands.
    pub env: Vec<String>,
    /// Command. Empty when the image entrypoint stands.
    pub cmd: Vec<String>,
    /// `key=value` labels. No JIT.
    pub labels: Vec<String>,
    /// Mounts. Volume sources use `volume:<name>`.
    pub mounts: Vec<Mount>,
    /// Immutable image subpaths mounted read-only over writable parent volumes.
    pub image_mounts: Vec<ImageMount>,
    /// Host privilege. False for the runner. True only for private `DinD`.
    pub privileged: bool,
    /// Additional numeric groups. Linux runner uses this for the private `DinD` socket.
    pub group_add: Vec<String>,
    /// Fixed host security options. JIT-bearing Linux runners require `AppArmor`.
    pub security_opts: Vec<String>,
    /// `OpenStdin`. True only for the runner channel.
    pub open_stdin: bool,
    /// `container:<id>` joins that container's network namespace. Runner only.
    pub network_mode: Option<String>,
}

/// Ids this call created. No JIT and no name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    /// Private `DinD` container id.
    pub dind_id: String,
    /// Runner container id.
    pub runner_id: String,
}

/// Project an audited runner plan. Stdin carries JIT. The plan does not.
///
/// # Errors
///
/// Returns [`HostError::PrivilegedRunner`] or [`HostError::ForbiddenMount`]
/// from [`audit_plan`], including JIT in env, cmd, or labels.
pub fn runner_create(plan: &ContainerPlan) -> Result<CreateProjection, HostError> {
    audit_plan(plan)?;
    Ok(CreateProjection {
        name: plan.name.clone(),
        image: plan.image.clone(),
        platform: plan.platform.clone(),
        readonly_rootfs: plan.readonly_rootfs,
        env: plan.env.clone(),
        cmd: plan.cmd.clone(),
        labels: plan.labels.clone(),
        mounts: plan.mounts.clone(),
        image_mounts: plan.image_mounts.clone(),
        privileged: false,
        group_add: plan.group_add.clone(),
        security_opts: plan.security_opts.clone(),
        open_stdin: true,
        network_mode: None,
    })
}

/// Join `spec` to the private `DinD` network namespace.
///
/// Published service ports and Testcontainers then bind on the runner's localhost.
/// `dind_id` must be one Docker container id. `host` and other modes are rejected.
///
/// # Errors
///
/// Returns [`HostError::ForbiddenMount`] when `dind_id` is not a hex container id.
pub fn join_dind_net(
    mut spec: CreateProjection,
    dind_id: &str,
) -> Result<CreateProjection, HostError> {
    if !dind_container_id(dind_id) {
        return Err(HostError::ForbiddenMount);
    }
    spec.network_mode = Some(format!("container:{dind_id}"));
    Ok(spec)
}

pub(crate) fn dind_container_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Private `DinD` create. Privilege is not a flag on the runner plan.
///
/// Mounts are the runner plan's socket volume, the work volume at
/// the Actions runner's `/home/runner/_work`, and a DinD-only volume at `/var/lib/docker`. The data
/// volume is not on the runner. vfs on the container layer slows later
/// Testcontainers starts.
///
/// # Errors
///
/// Returns [`HostError::ForbiddenMount`] when `private_volume` is not one private name.
pub fn dind_create(private_volume: &str) -> Result<CreateProjection, HostError> {
    let runner = runner_plan(private_volume)?;
    let mounts = dind_mounts(runner.mounts, private_volume)?;
    let mut labels = worker_labels(private_volume, "dind");
    labels.sort_unstable();
    Ok(CreateProjection {
        name: format!("{private_volume}-dind"),
        image: DIND_IMAGE.to_owned(),
        platform: runner.platform,
        readonly_rootfs: false,
        env: Vec::new(),
        cmd: Vec::new(),
        labels,
        mounts,
        image_mounts: Vec::new(),
        privileged: true,
        group_add: Vec::new(),
        security_opts: Vec::new(),
        open_stdin: false,
        network_mode: None,
    })
}

fn dind_mounts(mut mounts: Vec<Mount>, private_volume: &str) -> Result<Vec<Mount>, HostError> {
    if !private_volume
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
    {
        return Err(HostError::ForbiddenMount);
    }
    mounts.push(Mount {
        source: format!("volume:{private_volume}-docker"),
        target: "/var/lib/docker".to_owned(),
        read_only: false,
    });
    Ok(mounts)
}

fn dind_mounts_for_profile(private_volume: &str) -> Result<Vec<Mount>, HostError> {
    if !private_volume
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
    {
        return Err(HostError::ForbiddenMount);
    }
    Ok(vec![
        Mount {
            source: format!("volume:{private_volume}"),
            target: "/var/run".to_owned(),
            read_only: false,
        },
        Mount {
            source: format!("volume:{private_volume}-work"),
            target: runner_work_path(),
            read_only: false,
        },
        Mount {
            source: format!("volume:{private_volume}-externals"),
            target: "/home/runner/externals".to_owned(),
            read_only: true,
        },
        Mount {
            source: format!("volume:{private_volume}-docker"),
            target: "/var/lib/docker".to_owned(),
            read_only: false,
        },
    ])
}

fn worker_labels(private_volume: &str, role: &str) -> Vec<String> {
    let mut labels = vec![
        format!("velnor.role={role}"),
        format!("velnor.volume={private_volume}"),
        format!("velnor.worker={private_volume}"),
    ];
    labels.sort_unstable();
    labels
}

/// Create volumes, start `DinD`, start the runner, then write `jit` on stdin.
///
/// Empty `jit` returns before any container create. Failure after a create
/// removes only ids this call created.
///
/// # Errors
///
/// Returns [`HostError::EmptyJit`] when `jit` is empty.
/// Returns [`HostError::ForbiddenMount`] for a rejected volume name.
/// Returns [`HostError::Docker`] when Docker rejects a call. The error text
/// does not include `jit`.
pub async fn start_pair(
    docker: &Docker,
    private_volume: &str,
    jit: &[u8],
) -> Result<Started, HostError> {
    let partial = Box::pin(crate::stage::start_pair_until(
        docker,
        private_volume,
        jit,
        PairStop::Jit,
    ))
    .await?;
    Ok(Started {
        dind_id: partial.dind_id.ok_or(HostError::Docker)?,
        runner_id: partial.runner_id.ok_or(HostError::Docker)?,
    })
}

#[cfg(test)]
mod tests;
