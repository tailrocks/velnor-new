use std::io::Read;
use std::path::Path;

use velnor_runner_host::{HostConfig, RepositoryConnection};

use super::connect::{ConnectError, ConnectRequest};

const MAX_AUTHORITY_INPUT_BYTES: usize = 256 * 1024;

pub(super) fn apply(
    mut candidate: HostConfig,
    request: &ConnectRequest<'_>,
    connection: &RepositoryConnection,
) -> Result<HostConfig, ConnectError> {
    if let Some(path) = request.job_authority_policy {
        if request.job_authority_inventory.is_some()
            || request.job_authority_generator_source_sha.is_some()
            || request.job_authority_generator_artifact_sha256.is_some()
            || request.job_authority_generation.is_some()
        {
            return Err(ConnectError::Config);
        }
        let policy_json = read_authority_input(path)?;
        candidate = candidate
            .with_job_authority_policy_json(&connection.connection_id, &policy_json)
            .map_err(|_| ConnectError::Config)?;
    } else if let Some(path) = request.job_authority_inventory {
        let (Some(source_sha), Some(artifact_sha256), Some(generation)) = (
            request.job_authority_generator_source_sha,
            request.job_authority_generator_artifact_sha256,
            request.job_authority_generation,
        ) else {
            return Err(ConnectError::Config);
        };
        let inventory = read_authority_input(path)?;
        candidate = candidate
            .with_job_authority_inventory(
                &connection.connection_id,
                &inventory,
                source_sha,
                artifact_sha256,
                generation,
            )
            .map_err(|_| ConnectError::Config)?;
    } else if request.job_authority_generator_source_sha.is_some()
        || request.job_authority_generator_artifact_sha256.is_some()
        || request.job_authority_generation.is_some()
    {
        return Err(ConnectError::Config);
    }
    Ok(candidate)
}

fn read_authority_input(path: &Path) -> Result<String, ConnectError> {
    let flags = rustix::fs::OFlags::RDONLY
        | rustix::fs::OFlags::NOFOLLOW
        | rustix::fs::OFlags::NONBLOCK
        | rustix::fs::OFlags::CLOEXEC;
    let descriptor = rustix::fs::open(path, flags, rustix::fs::Mode::empty())
        .map_err(|_| ConnectError::Config)?;
    let mut file = std::fs::File::from(descriptor);
    let metadata = file.metadata().map_err(|_| ConnectError::Config)?;
    if !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_AUTHORITY_INPUT_BYTES as u64
    {
        return Err(ConnectError::Config);
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take((MAX_AUTHORITY_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ConnectError::Config)?;
    if bytes.len() > MAX_AUTHORITY_INPUT_BYTES
        || u64::try_from(bytes.len()).ok() != Some(metadata.len())
    {
        return Err(ConnectError::Config);
    }
    String::from_utf8(bytes).map_err(|_| ConnectError::Config)
}
