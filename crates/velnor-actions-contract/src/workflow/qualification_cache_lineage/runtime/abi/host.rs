use super::super::types::{QualificationRuntimeAbiObservation, QualificationRuntimeIdentity};
use super::shared::{normalize_arch, normalize_os};
use crate::errors::ContractError;

pub(super) fn validate_host_fact(
    evidence: &QualificationRuntimeIdentity,
) -> Result<(), ContractError> {
    use crate::cachekey::validate_semantic_text;

    let host = evidence
        .abi
        .iter()
        .find_map(|fact| match fact {
            QualificationRuntimeAbiObservation::Host {
                os,
                arch,
                kernel,
                libc,
            } => Some((os, arch, kernel, libc)),
            _ => None,
        })
        .ok_or_else(|| {
            ContractError::identity("qualification.runtime_identity.abi", "host_fact_missing")
        })?;
    if normalize_os(host.0) != normalize_os(&evidence.platform.os)
        || normalize_arch(host.1) != normalize_arch(&evidence.platform.arch)
    {
        return Err(ContractError::identity(
            "qualification.runtime_identity.abi",
            "host_fact_mismatch",
        ));
    }
    validate_semantic_text("runtime_abi.kernel", host.2)?;
    if let Some(libc) = host.3 {
        validate_semantic_text("runtime_abi.libc", libc)?;
    }
    if normalize_os(&evidence.platform.os) == Some("linux")
        && evidence.platform.target.ends_with("-gnu")
        && host.3.is_none()
    {
        return Err(ContractError::identity(
            "qualification.runtime_identity.abi",
            "linux_libc_fact_missing",
        ));
    }
    Ok(())
}
