//! Source, reconstruction and fail-closed distribution proofs.

use super::*;

fn fixture(domain: ToolCacheDomain, version: &str) -> Result<CompiledSourceHelper, MiseError> {
    let mise = QualifiedDistribution::qualify_official(
        DistributionTool::Mise,
        DistributionHost::LinuxAmd64,
    )?;
    let java = QualifiedDistribution::java_materialization_test_fixture()?;
    compiled(domain, version, &mise, &java)
}

#[test]
fn owned_domain_records_round_trip_without_install_authority() -> Result<(), MiseError> {
    for domain in domains() {
        let record = fixture(domain, "0.1.0")?;
        assert_eq!(record.invocation().args(), &[domain.name()]);
        assert!(record.invocation().installed_selectors().is_empty());
        assert_eq!(record.environment()["MISE_DATA_DIR"], domain.root());
        assert_eq!(record.environment()["GRADLE_USER_HOME"], GRADLE_HOME);
        assert_eq!(
            record_with(record.invocation(), record.environment(), "0.1.0", fixture)?,
            record
        );
        assert!(record_with(record.invocation(), record.environment(), "0.1.1", fixture).is_err());
        assert_eq!(
            record.invocation().descriptor().source_sha256(),
            compiled_source_sha256(record.source().as_bytes())
        );
        assert!(
            record
                .source()
                .contains(QualifiedDistribution::java_materialization_test_fixture()?.selector())
        );
        assert!(record.source().contains("'where'"));
        assert!(!record.source().contains("mise activate"));
        assert!(!record.source().contains("systemProp.org.gradle.java"));
    }
    Ok(())
}

#[test]
fn production_never_falls_back_to_official_mise() {
    for domain in domains() {
        assert!(helper_for_domain(domain, "0.1.0").is_err());
    }
    assert!(helper_for_domain(ToolCacheDomain::NpmBootstrap, "0.1.0").is_err());
}

#[test]
fn reconstruction_rejects_arguments_footprints_digests_and_environment() -> Result<(), MiseError> {
    let record = fixture(ToolCacheDomain::Full, "0.1.0")?;
    let invocation = record.invocation();
    for args in [
        vec!["full".to_owned(), "java@latest".to_owned()],
        vec!["npm-bootstrap".to_owned()],
        vec!["$(touch sentinel)".to_owned()],
    ] {
        let altered = HelperInvocation::compiled(invocation.descriptor().clone(), args, vec![])
            .map_err(|error| contract(&error))?;
        assert!(record_with(&altered, record.environment(), "0.1.0", fixture).is_err());
    }
    let descriptor = SourceBoundHelper::compiled(
        SourceBoundOperation::JavaHomeMaterialize,
        SourceBoundOperation::JavaHomeMaterialize.path(),
        &"0".repeat(64),
    )
    .map_err(|error| contract(&error))?;
    let altered = HelperInvocation::compiled(descriptor, invocation.args().to_vec(), vec![])
        .map_err(|error| contract(&error))?;
    assert!(record_with(&altered, record.environment(), "0.1.0", fixture).is_err());
    let altered = HelperInvocation::compiled(
        invocation.descriptor().clone(),
        invocation.args().to_vec(),
        vec![
            QualifiedDistribution::java_materialization_test_fixture()?
                .selector()
                .to_owned(),
        ],
    )
    .map_err(|error| contract(&error))?;
    assert!(record_with(&altered, record.environment(), "0.1.0", fixture).is_err());
    for (key, value) in [
        ("MISE_DATA_DIR", "/tmp/foreign"),
        ("GRADLE_USER_HOME", "/tmp/foreign"),
        ("JAVA_HOME", "/tmp/foreign"),
        ("BASH_ENV", "/tmp/hostile"),
    ] {
        let mut env = record.environment().clone();
        env.insert(key.to_owned(), value.to_owned());
        assert!(record_with(invocation, &env, "0.1.0", fixture).is_err());
    }
    Ok(())
}

#[test]
fn persisted_authority_has_exactly_five_plain_keys() {
    let source = include_str!("catalog_java_materialize.py");
    for property in [
        "org.gradle.java.home",
        "org.gradle.java.installations.auto-detect",
        "org.gradle.java.installations.auto-download",
        "org.gradle.java.installations.fromEnv",
        "org.gradle.java.installations.paths",
    ] {
        assert_eq!(source.matches(&format!("('{property}',")).count(), 1);
    }
    assert!(source.contains("environment = dict(configuration['isolation'])"));
    assert!(source.contains("os.O_NOFOLLOW"));
    assert!(source.contains("root + '/' + java['install_root']"));
    assert!(source.contains("java_materialize_launch_digest"));
}

#[test]
fn actual_native_java_without_measured_installation_never_becomes_a_helper() -> Result<(), MiseError>
{
    let java = QualifiedDistribution::qualify_native(
        DistributionTool::Java,
        DistributionHost::LinuxAmd64,
        super::super::qualification::JAVA_SELECTION_VERSION,
    )?;
    assert_eq!(
        java.selection_version(),
        super::super::qualification::JAVA_SELECTION_VERSION
    );
    assert_eq!(java.version(), "25.0.4.1.1+1-jvmci-25.4-b23");
    assert!(
        QualifiedDistribution::require_native(
            DistributionTool::Java,
            DistributionHost::LinuxAmd64,
            java.selection_version(),
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn exact_native_selection_and_runtime_are_bound_without_catalog_aliases() -> Result<(), MiseError> {
    let java = QualifiedDistribution::java_materialization_test_fixture()?;
    let configuration = java_configuration(&java)?;
    assert_eq!(configuration["selector"], java.selector());
    assert_eq!(configuration["selection_version"], java.selection_version());
    assert_eq!(configuration["reported_version"], java.version());
    assert_eq!(configuration["qualification"], java.qualification_digest());
    assert!(
        configuration["selector"]
            .as_str()
            .is_some_and(|value| !value.starts_with("java@"))
    );
    Ok(())
}
