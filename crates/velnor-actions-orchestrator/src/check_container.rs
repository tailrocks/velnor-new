//! Own typed container binaries and explicit runtime delegation before readonly probes.
use crate::OrchestratorError;
use crate::internal::internal;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use velnor_actions_contract::config::{CheckRunner, HostContainerProfile};
use velnor_actions_mise::CheckDeadline;
use velnor_actions_mise::checks::{CheckCapabilityProof, PreparedContainer};
#[path = "check_container_bundle.rs"]
pub(crate) mod bundle;
#[path = "check_container_runtime.rs"]
pub(crate) mod runtime;

#[derive(Debug)]
pub(crate) struct OwnedContainer {
    pub prepared: PreparedContainer,
    pub sdk: Option<bundle::SdkProjection>,
    pub runtime: runtime::RuntimeProjection,
}

pub(super) fn prepare(
    home: &Path,
    runner: &CheckRunner,
    deadline: CheckDeadline,
) -> Result<Option<OwnedContainer>, OrchestratorError> {
    checkpoint(Some(deadline))?;
    let Some(profile) = &runner.container else {
        return Ok(None);
    };
    let cli = profile.cli();
    let docker_program = home.join("bin/docker");
    let docker_sha256 =
        project_executable(Path::new(&cli.path), &docker_program, &cli.sha256, deadline)?;
    let sdk = match profile {
        HostContainerProfile::OrbStack { sdk, .. } => {
            Some(bundle::project_sdk_until(home, sdk, Some(deadline))?)
        }
        HostContainerProfile::Docker { .. } => None,
    };
    if let Some(sdk) = &sdk {
        std::os::unix::fs::symlink(&sdk.orbctl_program, home.join("bin/orbctl"))
            .map_err(|_| internal("container_orbctl_projection"))?;
    }
    let runtime = runtime::prepare_runtime_until(home, profile, Some(deadline))?;
    let prepared = PreparedContainer {
        home: home.to_path_buf(),
        docker_config: home.join("docker"),
        docker_program,
        docker_sha256,
        endpoint: runtime.endpoint.clone(),
        orbctl_program: sdk.as_ref().map(|sdk| sdk.orbctl_program.clone()),
        orbctl_sha256: sdk.as_ref().map(|_| match profile {
            HostContainerProfile::OrbStack { sdk, .. } => sdk.cli_sha256.clone(),
            HostContainerProfile::Docker { .. } => String::new(),
        }),
    };
    Ok(Some(OwnedContainer {
        prepared,
        sdk,
        runtime,
    }))
}

#[derive(Debug)]
pub(crate) struct ObservedContainer {
    pub proof: CheckCapabilityProof,
    pub runtime: Option<runtime::RuntimeObservation>,
}

pub(crate) fn probe(
    runner: &CheckRunner,
    owned: Option<&OwnedContainer>,
    deadline: CheckDeadline,
) -> Result<ObservedContainer, OrchestratorError> {
    if let Some(owned) = owned {
        runtime::revalidate_runtime_until(&owned.runtime, Some(deadline))?;
        verify_owned_cli(&owned.prepared, Some(deadline))?;
        if let (
            Some(sdk),
            Some(HostContainerProfile::OrbStack {
                sdk: declaration, ..
            }),
        ) = (&owned.sdk, &runner.container)
        {
            bundle::revalidate_sdk_until(sdk, declaration, Some(deadline))?;
            checkpoint(Some(deadline))?;
            let alias = owned.prepared.home.join("bin/orbctl");
            if std::fs::read_link(alias).map_err(|_| internal("container_orbctl_alias"))?
                != sdk.orbctl_program
            {
                return Err(internal("container_orbctl_alias_changed"));
            }
        }
    }
    let proof = velnor_actions_mise::checks::verify_check_capabilities(
        runner,
        owned.map(|o| &o.prepared),
        deadline,
    )
    .map_err(|e| internal(&e.to_string()))?;
    let runtime = owned
        .map(|o| runtime::observe_runtime_until(&o.runtime, Some(deadline)))
        .transpose()?;
    Ok(ObservedContainer { proof, runtime })
}

pub(crate) fn receipt(
    runner: &CheckRunner,
    owned: Option<&OwnedContainer>,
    before: ObservedContainer,
    after: ObservedContainer,
) -> Result<Option<crate::check_evidence::gate::container::ContainerReceipt>, OrchestratorError> {
    crate::check_evidence::gate::container::container_receipt(
        runner,
        before.proof,
        after.proof,
        owned.and_then(|o| o.sdk.as_ref()),
        owned.map(|o| &o.runtime),
        before.runtime,
        after.runtime,
    )
}

fn verify_owned_cli(
    prepared: &PreparedContainer,
    deadline: Option<CheckDeadline>,
) -> Result<(), OrchestratorError> {
    checkpoint(deadline)?;
    crate::check_evidence::reject_link_components(&prepared.home, "bin/docker")?;
    let metadata = std::fs::symlink_metadata(&prepared.docker_program)
        .map_err(|_| internal("container_owned_cli_unreadable"))?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o7_777 != 0o500 {
        return Err(internal("container_owned_cli_mode"));
    }
    let bytes = crate::retrieve_reports::staged_reads::read_staged_bytes_until(
        &prepared.docker_program,
        256 * 1024 * 1024,
        || read_deadline_checkpoint(deadline),
    )
    .map_err(|_| internal("container_owned_cli_unreadable"))?;
    if crate::cover_identity::generator::sha256_hex(&bytes) != prepared.docker_sha256 {
        return Err(internal("container_owned_cli_changed"));
    }
    Ok(())
}

fn project_executable(
    source: &Path,
    destination: &Path,
    expected: &str,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    checkpoint(Some(deadline))?;
    if !source.is_absolute()
        || source
            .canonicalize()
            .map_err(|_| internal("container_cli_path"))?
            != source
    {
        return Err(internal("container_cli_canonical_path"));
    }
    crate::check_evidence::reject_link_components(
        Path::new("/"),
        source
            .to_str()
            .ok_or_else(|| internal("container_cli_path"))?
            .trim_start_matches('/'),
    )?;
    let bytes = crate::retrieve_reports::staged_reads::read_staged_bytes_until(
        source,
        256 * 1024 * 1024,
        || read_deadline_checkpoint(Some(deadline)),
    )
    .map_err(|_| internal("container_cli_unreadable"))?;
    let observed = crate::cover_identity::generator::sha256_hex(&bytes);
    if bytes.is_empty() || observed != expected {
        return Err(internal("container_cli_sha256"));
    }
    crate::exclusive_write::write_exclusive_until(destination, &bytes, "container_cli", || {
        checkpoint(Some(deadline))
    })?;
    let fd = rustix::fs::open(
        destination,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| internal("container_cli_permissions"))?;
    rustix::fs::fchmod(&fd, rustix::fs::Mode::RUSR | rustix::fs::Mode::XUSR)
        .map_err(|_| internal("container_cli_permissions"))?;
    checkpoint(Some(deadline))?;
    Ok(observed)
}

fn checkpoint(deadline: Option<CheckDeadline>) -> Result<(), OrchestratorError> {
    if let Some(deadline) = deadline {
        deadline
            .remaining()
            .map_err(|error| internal(&error.to_string()))?;
    }
    Ok(())
}

fn read_deadline_checkpoint(deadline: Option<CheckDeadline>) -> Result<(), &'static str> {
    deadline.map_or(Ok(()), |value| {
        value
            .remaining()
            .map(|_| ())
            .map_err(|_| "deadline_exhausted")
    })
}

#[cfg(test)]
#[path = "check_container_tests.rs"]
mod tests;
