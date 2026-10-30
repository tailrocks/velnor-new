use bollard::models::ContainerInspectResponse;

use crate::error::HostError;
use crate::worker::runner_create_for_identity;

use super::{identity, inspect_projection};
use super::super::topology_matches;

fn host_config_mut(
    inspected: &mut ContainerInspectResponse,
) -> Result<&mut bollard::models::HostConfig, HostError> {
    inspected
        .host_config
        .as_mut()
        .ok_or(HostError::Ownership)
}

#[test]
fn runner_reconciliation_rejects_writable_root() -> Result<(), HostError> {
    let spec = runner_create_for_identity(&identity()?, None)?;
    let mut inspected = inspect_projection(&spec)?;
    host_config_mut(&mut inspected)?.readonly_rootfs = Some(false);

    assert!(!topology_matches(&spec, &inspected)?);
    Ok(())
}

#[test]
fn runner_reconciliation_requires_only_the_sudo_identity_caps() -> Result<(), HostError> {
    let spec = runner_create_for_identity(&identity()?, None)?;

    let mut missing_drop = inspect_projection(&spec)?;
    host_config_mut(&mut missing_drop)?.cap_drop = None;
    assert!(!topology_matches(&spec, &missing_drop)?);

    let mut extra_cap = inspect_projection(&spec)?;
    host_config_mut(&mut extra_cap)?.cap_add =
        Some(vec!["SETGID".to_owned(), "SETUID".to_owned(), "SYS_ADMIN".to_owned()]);
    assert!(!topology_matches(&spec, &extra_cap)?);

    let mut missing_setuid = inspect_projection(&spec)?;
    host_config_mut(&mut missing_setuid)?.cap_add = Some(vec!["SETGID".to_owned()]);
    assert!(!topology_matches(&spec, &missing_setuid)?);
    Ok(())
}

#[test]
fn runner_reconciliation_requires_the_exact_seccomp_policy() -> Result<(), HostError> {
    let spec = runner_create_for_identity(&identity()?, None)?;

    let mut unconfined = inspect_projection(&spec)?;
    host_config_mut(&mut unconfined)?.security_opt =
        Some(vec!["seccomp=unconfined".to_owned()]);
    assert!(!topology_matches(&spec, &unconfined)?);

    let mut builtin = inspect_projection(&spec)?;
    host_config_mut(&mut builtin)?.security_opt = Some(vec!["seccomp=builtin".to_owned()]);
    assert!(!topology_matches(&spec, &builtin)?);

    let mut extra_security_opt = inspect_projection(&spec)?;
    host_config_mut(&mut extra_security_opt)?.security_opt = Some(vec![
        crate::worker::seccomp::runner_security_opt(),
        "no-new-privileges".to_owned(),
    ]);
    assert!(!topology_matches(&spec, &extra_security_opt)?);
    Ok(())
}
