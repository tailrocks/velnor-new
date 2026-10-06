//! Closed acquisition of the owned Mise distribution; no upstream fallback.
use super::qualification::{
    DistributionHost, DistributionRequirement, DistributionTool, ProvisioningMode,
    QualifiedDistribution,
};
use crate::MiseError;
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation,
    ToolCacheDomain, compiled_source_sha256,
};

#[path = "catalog_mise_acquisition_source_intent.rs"]
mod source_intent;
pub use source_intent::{SourceIntentMiseAcquisition, source_intent_acquisition};

/// Bind an isolated fixed root to the exact owner-qualified distribution.
/// # Errors
/// Fails closed until the owned platform distribution is published and qualified.
pub fn helper_for_domain(
    domain: ToolCacheDomain,
    host: DistributionHost,
    generator_version: &str,
) -> Result<CompiledSourceHelper, MiseError> {
    let distribution = QualifiedDistribution::require_for_generator(
        DistributionTool::Mise,
        host,
        DistributionRequirement::RequiresNoMiserc,
    )?;
    compiled(domain, generator_version, &distribution)
}

fn compiled(
    domain: ToolCacheDomain,
    version: &str,
    distribution: &QualifiedDistribution,
) -> Result<CompiledSourceHelper, MiseError> {
    if distribution.tool() != DistributionTool::Mise
        || distribution.provisioning_mode() != ProvisioningMode::MiseNoMisercExclusiveConfig
    {
        return Err(invalid());
    }
    let source = source(version)?;
    let operation = SourceBoundOperation::MiseBootstrap;
    let digest = compiled_source_sha256(source.as_bytes());
    let descriptor = SourceBoundHelper::compiled(operation, operation.path(), &digest)
        .map_err(|error| contract(&error))?;
    let invocation = HelperInvocation::compiled(
        descriptor,
        vec![
            domain.name().to_owned(),
            host_name(distribution.host()).to_owned(),
            configuration(distribution)?.to_string(),
        ],
        vec![],
    )
    .map_err(|error| contract(&error))?;
    let env = BTreeMap::from([
        ("MISE_DATA_DIR".to_owned(), domain.root().to_owned()),
        (
            "VELNOR_MISE_TARGET".to_owned(),
            distribution.host().abi().to_owned(),
        ),
        (
            "VELNOR_MISE_SHA256".to_owned(),
            distribution.binary_sha256().to_owned(),
        ),
        (
            "VELNOR_MISE_VERSION".to_owned(),
            distribution.version().to_owned(),
        ),
        (
            "VELNOR_QUALIFIED_TOOL_IDENTITY".to_owned(),
            format!("qualified-tools@{}", distribution.qualification_digest()),
        ),
    ]);
    Ok(CompiledSourceHelper::compiled(invocation, source)
        .map_err(|error| contract(&error))?
        .with_environment(env))
}

/// Reconstruct the complete source, arguments and environment from compiled authority.
/// # Errors
/// Rejects foreign operations, altered bindings and unavailable qualifications.
pub fn record_for_invocation(
    invocation: &HelperInvocation,
    env: &BTreeMap<String, String>,
    version: &str,
) -> Result<CompiledSourceHelper, MiseError> {
    if invocation.descriptor().operation() != SourceBoundOperation::MiseBootstrap
        || invocation.args().len() != 3
    {
        return Err(invalid());
    }
    let domain = domains()
        .into_iter()
        .find(|domain| invocation.args()[0] == domain.name())
        .ok_or_else(invalid)?;
    let host = [
        DistributionHost::LinuxAmd64,
        DistributionHost::LinuxArm64,
        DistributionHost::MacosArm64,
    ]
    .into_iter()
    .find(|host| invocation.args()[1] == host_name(*host))
    .ok_or_else(invalid)?;
    let expected = helper_for_domain(domain, host, version)?;
    if expected.invocation() != invocation || expected.environment() != env {
        return Err(invalid());
    }
    Ok(expected)
}

fn configuration(distribution: &QualifiedDistribution) -> Result<serde_json::Value, MiseError> {
    let format = match distribution.asset_format() {
        super::qualification::DistributionAssetFormat::Binary => "binary",
        super::qualification::DistributionAssetFormat::TarGzip => "tar-gzip",
        super::qualification::DistributionAssetFormat::TarXz
        | super::qualification::DistributionAssetFormat::Zip => return Err(invalid()),
    };
    let platform = match distribution.host() {
        DistributionHost::LinuxAmd64 => ["Linux", "x86_64"],
        DistributionHost::LinuxArm64 => ["Linux", "aarch64"],
        DistributionHost::MacosArm64 => ["Darwin", "arm64"],
    };
    Ok(serde_json::json!({
        "platform": platform, "asset_url": distribution.asset_url(),
        "archive_sha256": distribution.archive_sha256(), "binary_sha256": distribution.binary_sha256(),
        "format": format,
        "member": distribution.binary_member(), "qualification": distribution.qualification_digest(),
        "policy": "velnor-mise-acquisition-v1",
    }))
}

fn source(version: &str) -> Result<String, MiseError> {
    let roots: BTreeMap<_, _> = domains()
        .into_iter()
        .map(|domain| {
            (
                domain.name(),
                domain
                    .root()
                    .trim_start_matches("${{ runner.temp }}/")
                    .to_owned(),
            )
        })
        .collect();
    source_with_roots(version, &roots, AcquisitionOutputPurpose::Workflow)
}

#[derive(Clone, Copy)]
enum AcquisitionOutputPurpose {
    Workflow,
    SourceIntent,
}

fn source_with_roots(
    version: &str,
    roots: &BTreeMap<&str, String>,
    purpose: AcquisitionOutputPurpose,
) -> Result<String, MiseError> {
    let output = match purpose {
        AcquisitionOutputPurpose::Workflow => "workflow",
        AcquisitionOutputPurpose::SourceIntent => "source-intent",
    };
    let body = format!(
        "set -euo pipefail\nexport PATH=/usr/bin:/bin:/usr/sbin:/sbin\n/usr/bin/python3 -I -S - \"$1\" \"$3\" <<'VELNOR_OWNED_MISE_ACQUISITION'\n{}\n{}\n{}\n{}\nDOMAINS = {}\nOUTPUT_PURPOSE = {output:?}\nconfiguration = json.loads(__import__('sys').argv[2])\nacquire(__import__('sys').argv[1], configuration)\nVELNOR_OWNED_MISE_ACQUISITION\n",
        include_str!("catalog_executable_bounds.py"),
        include_str!("catalog_mise_acquisition_archive.py"),
        include_str!("catalog_mise_acquisition_download.py"),
        include_str!("catalog_mise_acquisition_install.py"),
        serde_json::to_string(&roots).map_err(|error| MiseError::Contract {
            problem: error.to_string()
        })?
    );
    velnor_actions_contract::generated_source(version, &body).map_err(|error| contract(&error))
}

const fn domains() -> [ToolCacheDomain; 6] {
    [
        ToolCacheDomain::Planning,
        ToolCacheDomain::Full,
        ToolCacheDomain::NpmBootstrap,
        ToolCacheDomain::BunBootstrap,
        ToolCacheDomain::TofuBootstrap,
        ToolCacheDomain::GradleBootstrap,
    ]
}

const fn host_name(host: DistributionHost) -> &'static str {
    match host {
        DistributionHost::LinuxAmd64 => "linux-amd64",
        DistributionHost::LinuxArm64 => "linux-arm64",
        DistributionHost::MacosArm64 => "macos-arm64",
    }
}

fn contract(error: &velnor_actions_contract::ContractError) -> MiseError {
    MiseError::Contract {
        problem: error.to_string(),
    }
}
fn invalid() -> MiseError {
    MiseError::Contract {
        problem: "invalid_owned_mise_acquisition".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unpublished_owned_bootstrap_fails_closed_on_every_host_and_domain() {
        for host in [
            DistributionHost::LinuxAmd64,
            DistributionHost::LinuxArm64,
            DistributionHost::MacosArm64,
        ] {
            for domain in domains() {
                assert!(helper_for_domain(domain, host, "0.1.0").is_err());
            }
        }
    }
    #[test]
    fn owned_fixture_binds_source_root_host_and_exact_pins() {
        let mut sources = std::collections::BTreeSet::new();
        for host in [
            DistributionHost::LinuxAmd64,
            DistributionHost::LinuxArm64,
            DistributionHost::MacosArm64,
        ] {
            let distribution =
                QualifiedDistribution::owned_test_fixture(host).expect("private owned fixture");
            for domain in domains() {
                let record = compiled(domain, "0.1.0", &distribution).expect("fixture source");
                sources.insert(record.source().to_owned());
                assert!(record.invocation().installed_selectors().is_empty());
                assert_eq!(
                    &record.invocation().args()[..2],
                    [domain.name(), host_name(host)]
                );
                assert_eq!(record.environment()["MISE_DATA_DIR"], domain.root());
                assert_eq!(record.environment()["VELNOR_MISE_TARGET"], host.abi());
                assert_eq!(
                    record.environment()["VELNOR_MISE_SHA256"],
                    distribution.binary_sha256()
                );
                assert_eq!(
                    record.environment()["VELNOR_MISE_VERSION"],
                    distribution.version()
                );
                assert!(record.invocation().args()[2].contains(distribution.asset_url()));
                assert!(record.invocation().args()[2].contains(distribution.archive_sha256()));
                assert!(record.invocation().args()[2].contains(distribution.binary_member()));
                assert!(!record.source().contains("jdx/mise-action"));
                assert_eq!(
                    compiled_source_sha256(record.source().as_bytes()),
                    record.invocation().descriptor().source_sha256()
                );
            }
        }
        assert_eq!(
            sources.len(),
            1,
            "one canonical compiled source across all domains and hosts"
        );
    }

    #[test]
    fn official_distribution_never_authorizes_owned_acquisition() {
        for host in [
            DistributionHost::LinuxAmd64,
            DistributionHost::LinuxArm64,
            DistributionHost::MacosArm64,
        ] {
            let official = QualifiedDistribution::qualify_official(DistributionTool::Mise, host)
                .expect("official audited distribution");
            assert!(compiled(ToolCacheDomain::Full, "0.1.0", &official).is_err());
        }
    }
}
