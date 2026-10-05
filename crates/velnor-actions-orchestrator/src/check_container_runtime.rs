//! Own a declaration-bound local container socket and Docker context.
use crate::OrchestratorError;
use crate::cover_identity::generator::sha256_hex;
use crate::exclusive_write;
use crate::internal::internal;
use std::fs;
use std::path::{Component, Path, PathBuf};
use velnor_actions_contract::config::{HostContainerProfile, MAX_CHECK_CONTAINER_PATH_BYTES};

#[path = "check_container_runtime_context.rs"]
mod context;
#[path = "check_container_runtime_inventory.rs"]
mod inventory;
#[path = "check_container_runtime_socket_paths.rs"]
mod socket_paths;
use context::{
    context_config_bytes, context_metadata_bytes, ensure_children, owned_directory,
    owned_regular_file, read_limited,
};
pub(crate) use inventory::{RuntimeObservation, RuntimeRootEvidence, SocketEvidence};
use inventory::{inspect_runtime, owner, validate_socket};

/// Safe, declaration-bound runtime evidence retained for the capability receipt.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct RuntimeProjection {
    /// Canonical local Unix endpoint passed to the Docker CLI.
    pub endpoint: String,
    /// Declared Docker context name.
    pub context: String,
    /// Owned credential-free Docker configuration directory.
    pub docker_config: PathBuf,
    /// Owned Docker context metadata file.
    pub context_metadata: PathBuf,
    /// SHA-256 directory name used by Docker for this context.
    pub context_hash: String,
    /// Validated external `OrbStack` runtime authority, when declared.
    pub runtime_dir: Option<PathBuf>,
    /// Owned delegation link for the `OrbStack` runtime authority.
    pub runtime_link: Option<PathBuf>,
    /// Bounded type/path/owner inventory of the runtime authority.
    pub runtime_entries: Vec<RuntimeEntryEvidence>,
    #[serde(skip)]
    socket_path: PathBuf,
    /// Initial socket identity, revalidated before every probe.
    pub socket: SocketEvidence,
    #[serde(skip)]
    runtime_uid: Option<u32>,
    #[serde(skip)]
    home_owner: u32,
    /// Initial `OrbStack` runtime directory identity.
    pub runtime_root: Option<RuntimeRootEvidence>,
}

/// One runtime entry, without reading its contents.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct RuntimeEntryEvidence {
    /// POSIX path relative to the declared runtime directory, or the absolute
    /// Docker socket path for a generic Docker profile.
    pub path: String,
    /// Admitted filesystem type.
    pub kind: RuntimeEntryKind,
    /// Owning Unix user ID.
    pub owner: u32,
}

/// Runtime entry kinds permitted by the narrow `OrbStack` authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeEntryKind {
    Directory,
    RegularFile,
    UnixSocket,
}

/// Validate and project one typed host container runtime without process I/O.
pub(super) fn prepare_runtime(
    home: &Path,
    profile: &HostContainerProfile,
) -> Result<RuntimeProjection, OrchestratorError> {
    #[cfg(not(unix))]
    {
        let _ = (home, profile);
        return Err(OrchestratorError::unsupported(
            "container_runtime",
            "unix_platform_required",
        ));
    }
    let home = validate_home(home)?;
    let home_owner = owner(&fs::symlink_metadata(&home).map_err(|e| io(&home, e))?);
    let context = validate_context(profile.context())?;
    let socket_path = canonical_input(profile.socket_path(), "container_socket")?;
    let (runtime_dir, runtime_uid, runtime_entries, runtime_link, runtime_root) = match profile {
        HostContainerProfile::Docker { .. } => (None, None, Vec::new(), None, None),
        HostContainerProfile::OrbStack { sdk, .. } => {
            let uid = sdk.runtime_uid;
            if uid == 0 {
                return Err(internal("orbstack_runtime_owner"));
            }
            let directory = canonical_directory(Path::new(&sdk.runtime_dir), "runtime_dir")?;
            let (entries, root) = inspect_runtime(&directory, uid)?;
            if socket_path.parent() != Some(directory.as_path()) {
                return Err(internal("orbstack_socket_parent"));
            }
            let link = home.join(".orbstack").join("run");
            socket_paths::validate(&home, &entries)?;
            project_runtime_link(&home, &link, &directory)?;
            (Some(directory), Some(uid), entries, Some(link), Some(root))
        }
    };
    let expected_owner = profile.socket_uid();
    let before = validate_socket(&socket_path, expected_owner)?;
    let endpoint = format!("unix://{}", before.path);
    let (docker_config, context_metadata, context_hash) =
        write_context(&home, &context, &endpoint)?;
    let after = validate_socket(&socket_path, expected_owner)?;
    if before != after || endpoint != format!("unix://{}", after.path) {
        return Err(internal("container_socket_changed_during_projection"));
    }
    let runtime_entries = if runtime_dir.is_some() {
        runtime_entries
    } else {
        vec![RuntimeEntryEvidence {
            path: before.path.clone(),
            kind: RuntimeEntryKind::UnixSocket,
            owner: before.owner,
        }]
    };
    Ok(RuntimeProjection {
        endpoint,
        context,
        docker_config,
        context_metadata,
        context_hash,
        runtime_dir,
        runtime_link,
        runtime_entries,
        socket_path,
        socket: before,
        runtime_uid,
        home_owner,
        runtime_root,
    })
}

/// Revalidate the external endpoint and `OrbStack` authority before probing.
pub(super) fn revalidate_runtime(projection: &RuntimeProjection) -> Result<(), OrchestratorError> {
    let expected_owner = projection.runtime_uid.unwrap_or(projection.socket.owner);
    let current = validate_socket(&projection.socket_path, expected_owner)?;
    if current != projection.socket || projection.endpoint != format!("unix://{}", current.path) {
        return Err(internal("container_socket_changed_after_projection"));
    }
    match (
        &projection.runtime_dir,
        projection.runtime_uid,
        &projection.runtime_link,
        &projection.runtime_root,
    ) {
        (None, None, None, None) => {}
        (Some(directory), Some(uid), Some(link), Some(expected_root)) => {
            let (_, current_root) = inspect_runtime(directory, uid)?;
            if &current_root != expected_root {
                return Err(internal("orbstack_runtime_root_changed_after_projection"));
            }
            let parent = link
                .parent()
                .ok_or_else(|| internal("orbstack_runtime_link_parent"))?;
            check_ancestors(parent)?;
            require_directory(parent, "orbstack_runtime_link_parent")?;
            let target = fs::read_link(link).map_err(|e| io(link, e))?;
            if target != *directory {
                return Err(internal("orbstack_runtime_link_changed"));
            }
        }
        _ => return Err(internal("container_runtime_projection_shape")),
    }
    validate_owned_context(projection)
}

/// Revalidate all runtime state, then freshly capture identity evidence.
pub(super) fn observe_runtime(
    projection: &RuntimeProjection,
) -> Result<RuntimeObservation, OrchestratorError> {
    revalidate_runtime(projection)?;
    let expected_owner = projection.runtime_uid.unwrap_or(projection.socket.owner);
    let socket = validate_socket(&projection.socket_path, expected_owner)?;
    let runtime_root = match (
        &projection.runtime_dir,
        projection.runtime_uid,
        &projection.runtime_link,
        &projection.runtime_root,
    ) {
        (None, None, None, None) => None,
        (Some(directory), Some(uid), Some(_), Some(_)) => {
            let (_, root) = inspect_runtime(directory, uid)?;
            Some(root)
        }
        _ => return Err(internal("container_runtime_observation_shape")),
    };
    if socket != projection.socket || runtime_root != projection.runtime_root {
        return Err(internal("container_runtime_changed_after_observation"));
    }
    Ok(RuntimeObservation {
        socket,
        runtime_root,
    })
}

fn validate_owned_context(projection: &RuntimeProjection) -> Result<(), OrchestratorError> {
    let config = &projection.docker_config;
    let metadata = &projection.context_metadata;
    let config_file = config.join("config.json");
    let metadata_dir = config.join("contexts").join("meta");
    let hash_dir = metadata_dir.join(&projection.context_hash);
    if metadata.parent() != Some(hash_dir.as_path()) {
        return Err(internal("container_context_projection_path"));
    }
    for path in [
        &config_file,
        metadata,
        config,
        &config.join("contexts"),
        &metadata_dir,
        &hash_dir,
    ] {
        check_ancestors(path)?;
    }
    let expected_owner = projection.home_owner;
    for path in [config, &config.join("contexts"), &metadata_dir, &hash_dir] {
        owned_directory(path, expected_owner)?;
    }
    if !owned_regular_file(&config_file, expected_owner)?
        || !owned_regular_file(metadata, expected_owner)?
    {
        return Err(internal("container_context_projection_missing"));
    }
    ensure_children(config, &["config.json", "contexts"])?;
    ensure_children(&config.join("contexts"), &["meta"])?;
    ensure_children(&metadata_dir, &[&projection.context_hash])?;
    ensure_children(&hash_dir, &["meta.json"])?;
    let expected_config = context_config_bytes(&projection.context)?;
    let expected_metadata =
        context_metadata_bytes(&projection.context, &projection.endpoint, &hash_dir)?;
    if read_limited(&config_file)? != expected_config
        || read_limited(metadata)? != expected_metadata
    {
        return Err(internal("container_context_projection_changed"));
    }
    Ok(())
}

fn write_context(
    home: &Path,
    context: &str,
    endpoint: &str,
) -> Result<(PathBuf, PathBuf, String), OrchestratorError> {
    let config = home.join("docker");
    require_directory(&config, "docker_config")?;
    if fs::read_dir(&config)
        .map_err(|e| io(&config, e))?
        .next()
        .is_some()
    {
        return Err(internal("docker_config_must_be_empty"));
    }
    let hash = sha256_hex(context.as_bytes());
    let metadata_dir = config.join("contexts").join("meta").join(&hash);
    exclusive_write::create_dir_no_symlink(home, &metadata_dir)?;
    let metadata_path = metadata_dir.join("meta.json");
    let metadata_bytes = context_metadata_bytes(context, endpoint, &metadata_dir)?;
    exclusive_write::write_exclusive(&metadata_path, &metadata_bytes, "container_context")?;
    let config_bytes = context_config_bytes(context)?;
    exclusive_write::write_exclusive(
        &config.join("config.json"),
        &config_bytes,
        "container_config",
    )?;
    Ok((config, metadata_path, hash))
}

fn project_runtime_link(
    home: &Path,
    link: &Path,
    directory: &Path,
) -> Result<(), OrchestratorError> {
    exclusive_write::create_dir_no_symlink(home, &home.join(".orbstack"))?;
    if fs::symlink_metadata(link).is_ok() {
        return Err(internal("orbstack_runtime_link_exists"));
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(directory, link).map_err(|e| io(link, e))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (link, directory);
        Err(OrchestratorError::unsupported(
            "orbstack_runtime_link",
            "unix_platform_required",
        ))
    }
}

fn canonical_directory(path: &Path, field: &str) -> Result<PathBuf, OrchestratorError> {
    canonical_input(
        path.to_str()
            .ok_or_else(|| internal("container_path_utf8"))?,
        field,
    )
    .and_then(|path| {
        let metadata = fs::symlink_metadata(&path).map_err(|e| io(&path, e))?;
        if !metadata.is_dir() {
            return Err(internal("container_runtime_directory"));
        }
        Ok(path)
    })
}

fn canonical_input(raw: &str, field: &str) -> Result<PathBuf, OrchestratorError> {
    let path = Path::new(raw);
    if raw.len() > MAX_CHECK_CONTAINER_PATH_BYTES
        || raw.chars().any(char::is_control)
        || !path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::RootDir | Component::Normal(_)))
    {
        return Err(internal(&format!("{field}_absolute_canonical_required")));
    }
    check_ancestors(path)?;
    let canonical = path.canonicalize().map_err(|e| io(path, e))?;
    if canonical != path {
        return Err(internal(&format!("{field}_not_canonical")));
    }
    Ok(canonical)
}

fn check_ancestors(path: &Path) -> Result<(), OrchestratorError> {
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).map_err(|e| io(ancestor, e))?;
        if metadata.file_type().is_symlink() {
            return Err(internal("container_path_symlink_ancestor"));
        }
        if ancestor != path && !metadata.is_dir() {
            return Err(internal("container_path_parent_not_directory"));
        }
    }
    Ok(())
}

fn validate_home(path: &Path) -> Result<PathBuf, OrchestratorError> {
    let home = canonical_input(
        path.to_str()
            .ok_or_else(|| internal("container_home_utf8"))?,
        "container_home",
    )?;
    require_directory(&home, "container_home")?;
    Ok(home)
}

fn require_directory(path: &Path, field: &str) -> Result<(), OrchestratorError> {
    let metadata = fs::symlink_metadata(path).map_err(|e| io(path, e))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(internal(field));
    }
    Ok(())
}

fn validate_context(context: &str) -> Result<String, OrchestratorError> {
    if context.is_empty()
        || context.len() > 128
        || !context
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        || !context
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        return Err(internal("container_context_name"));
    }
    Ok(context.to_owned())
}

fn io(path: &Path, error: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}

#[cfg(test)]
#[path = "check_container_runtime_tests.rs"]
mod tests;
