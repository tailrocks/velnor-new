//! Exact native selection and nested authority mutation proofs.

use super::*;

#[test]
fn every_actual_native_source_record_has_exact_selection() -> Result<(), crate::MiseError> {
    let tools = [
        (DistributionTool::ReleasePlz, "0.3.169"),
        (DistributionTool::Node, "24.21.0"),
        (DistributionTool::Bun, "1.4.2"),
        (DistributionTool::OpenTofu, "1.13.1"),
        (DistributionTool::Python, "3.14.8"),
        (DistributionTool::Uv, "0.12.22"),
        (DistributionTool::Java, "25.0.4.1.1"),
        (DistributionTool::Gradle, "9.8.0"),
    ];
    for (tool, version) in tools {
        for host in [
            DistributionHost::LinuxAmd64,
            DistributionHost::LinuxArm64,
            DistributionHost::MacosArm64,
        ] {
            let record = QualifiedDistribution::qualify_native(tool, host, version)?;
            assert_eq!(record.selection_version(), version);
            assert!(!record.launch_entries().is_empty());
            assert!(QualifiedDistribution::qualify_native(tool, host, "0.0.0").is_err());
            if host != DistributionHost::MacosArm64 {
                assert!(QualifiedDistribution::require_native(tool, host, version).is_err());
            }
        }
    }
    Ok(())
}

#[test]
fn java_runtime_banner_is_distinct_from_catalog_selection() -> Result<(), crate::MiseError> {
    let record = QualifiedDistribution::qualify_native(
        DistributionTool::Java,
        DistributionHost::MacosArm64,
        "25.0.4.1.1",
    )?;
    assert_eq!(record.version(), "25.0.4.1.1+1-jvmci-25.4-b23");
    assert_eq!(record.selection_version(), "25.0.4.1.1");
    assert!(
        QualifiedDistribution::require_native(
            DistributionTool::Java,
            DistributionHost::MacosArm64,
            "25.0.4.1",
        )
        .is_err()
    );
    let consumer = QualifiedDistribution::require_native(
        DistributionTool::Gradle,
        DistributionHost::MacosArm64,
        "9.5.1",
    )?;
    assert_eq!(consumer.abi(), "gradle-consumer-distribution-9.5.1");
    let bootstrap = QualifiedDistribution::require_native(
        DistributionTool::Gradle,
        DistributionHost::MacosArm64,
        "9.4.1",
    )?;
    assert_eq!(
        bootstrap.abi(),
        "gradle-wrapper-bootstrap-distribution-9.4.1"
    );
    assert_ne!(
        consumer.qualification_digest(),
        bootstrap.qualification_digest()
    );
    Ok(())
}

#[test]
fn nested_authority_changes_identity() -> Result<(), crate::MiseError> {
    let original = QualifiedDistribution::require_native(
        DistributionTool::OpenTofu,
        DistributionHost::MacosArm64,
        "1.13.1",
    )?;
    let baseline = original.qualification_digest();
    let mut record = original.clone();
    let mut entry = record.launch_entries[0];
    let launch_mutations: [fn(&mut QualifiedLaunchEntry); 4] = [
        |entry| entry.archive_member = "other",
        |entry| entry.installed_relative_path = None,
        |entry| entry.sha256 = "changed",
        |entry| entry.kind = QualifiedLaunchKind::Script,
    ];
    for mutate in launch_mutations {
        mutate(&mut entry);
        record.launch_entries = Box::leak(vec![entry].into_boxed_slice());
        assert_ne!(baseline, record.qualification_digest());
        entry = original.launch_entries[0];
    }
    let plan = *original.required_install_plan()?;
    let mutations: [fn(&mut QualifiedInstallPlan); 6] = [
        |plan| plan.backend = QualifiedInstallBackend::SourceBoundBootstrap,
        |plan| plan.strip_components = 1,
        |plan| plan.bin_path = "changed",
        |plan| plan.root_relative_path = "changed",
        |plan| plan.transform_abi = "changed",
        |plan| {
            plan.environment = &[QualifiedInstallEnvironment {
                name: "PATH",
                relative_path: "bin",
            }]
        },
    ];
    for mutate in mutations {
        record = original.clone();
        let mut changed = plan;
        mutate(&mut changed);
        record.install_plan = Some(changed);
        assert_ne!(baseline, record.qualification_digest());
    }
    record.install_plan = None;
    assert_ne!(baseline, record.qualification_digest());
    Ok(())
}

#[test]
fn each_source_lineage_and_environment_field_changes_identity() -> Result<(), crate::MiseError> {
    let original = QualifiedDistribution::require_native(
        DistributionTool::Python,
        DistributionHost::MacosArm64,
        "3.14.8",
    )?;
    let baseline = original.qualification_digest();
    let source_mutations: [fn(&mut QualifiedSourceLineage); 5] = [
        |source| source.name = "changed",
        |source| source.repository = "changed",
        |source| source.commit = "changed",
        |source| source.tree = "changed",
        |source| source.version = "changed",
    ];
    for mutate in source_mutations {
        let mut record = original.clone();
        let mut entries = record.source_lineage.to_vec();
        mutate(&mut entries[0]);
        record.source_lineage = Box::leak(entries.into_boxed_slice());
        assert_ne!(baseline, record.qualification_digest());
    }
    let plan = *original.required_install_plan()?;
    for entry in [
        QualifiedInstallEnvironment {
            name: "changed",
            relative_path: "bin",
        },
        QualifiedInstallEnvironment {
            name: "PATH",
            relative_path: "changed",
        },
    ] {
        let mut record = original.clone();
        let mut changed = plan;
        changed.environment = Box::leak(vec![entry].into_boxed_slice());
        record.install_plan = Some(changed);
        assert_ne!(baseline, record.qualification_digest());
    }
    let mut reordered = original.clone();
    let mut launch = reordered.launch_entries.to_vec();
    launch.reverse();
    reordered.launch_entries = Box::leak(launch.into_boxed_slice());
    let mut sources = reordered.source_lineage.to_vec();
    sources.reverse();
    reordered.source_lineage = Box::leak(sources.into_boxed_slice());
    let mut changed = plan;
    let mut environment = changed.environment.to_vec();
    environment.reverse();
    changed.environment = Box::leak(environment.into_boxed_slice());
    reordered.install_plan = Some(changed);
    assert_eq!(baseline, reordered.qualification_digest());
    Ok(())
}

#[test]
fn qualified_plan_rejects_disconnected_artifact_and_layout() -> Result<(), crate::MiseError> {
    let original = QualifiedDistribution::require_native(
        DistributionTool::OpenTofu,
        DistributionHost::MacosArm64,
        "1.13.1",
    )?;
    for mutate in [
        (|record: &mut QualifiedDistribution| record.selector = "http:opentofu@1.13.1")
            as fn(&mut QualifiedDistribution),
        |record| {
            record.archive_sha256 =
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        },
        |record| record.binary_member = "other",
        |record| record.selection_version = "1.13.2",
        |record| record.launch_entries = &[],
    ] {
        let mut changed = original.clone();
        mutate(&mut changed);
        assert!(changed.validate().is_err());
    }
    let plan = *original.required_install_plan()?;
    for mutate in [
        (|plan: &mut QualifiedInstallPlan| plan.strip_components = 1)
            as fn(&mut QualifiedInstallPlan),
        |plan| plan.root_relative_path = "installs/other/1.13.1",
        |plan| {
            plan.environment = &[QualifiedInstallEnvironment {
                name: "UNQUALIFIED_HOME",
                relative_path: "",
            }]
        },
    ] {
        let mut changed = original.clone();
        let mut invalid = plan;
        mutate(&mut invalid);
        changed.install_plan = Some(invalid);
        assert!(changed.validate().is_err());
    }
    Ok(())
}

impl QualifiedDistribution {
    /// Synthetic Linux fixture; never admitted by a production lookup.
    pub(crate) fn java_materialization_test_fixture() -> Result<Self, crate::MiseError> {
        const ROOT: &str = "installs/http-graalvm-community-jdk/0.0.1-fixture";
        const HASH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        const ENV: &[QualifiedInstallEnvironment] = &[QualifiedInstallEnvironment {
            name: "JAVA_HOME",
            relative_path: "",
        }];
        const LAUNCH: &[QualifiedLaunchEntry] = &[
            QualifiedLaunchEntry {
                archive_member: "jdk/bin/java",
                installed_relative_path: Some(
                    "installs/http-graalvm-community-jdk/0.0.1-fixture/bin/java",
                ),
                sha256: HASH,
                kind: QualifiedLaunchKind::Executable,
            },
            QualifiedLaunchEntry {
                archive_member: "jdk/bin/javac",
                installed_relative_path: Some(
                    "installs/http-graalvm-community-jdk/0.0.1-fixture/bin/javac",
                ),
                sha256: HASH,
                kind: QualifiedLaunchKind::Executable,
            },
            QualifiedLaunchEntry {
                archive_member: "jdk/bin/native-image",
                installed_relative_path: Some(
                    "installs/http-graalvm-community-jdk/0.0.1-fixture/bin/native-image",
                ),
                sha256: HASH,
                kind: QualifiedLaunchKind::Executable,
            },
        ];
        Self {
            tool: DistributionTool::Java, host: DistributionHost::LinuxAmd64,
            selector: "http:graalvm-community-jdk[url=\"https://github.com/velnor-test-fixtures/java/releases/download/v0.0.1-fixture/java.tar.gz\",checksum=\"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",strip_components=1,bin_path=\"bin\"]@0.0.1-fixture",
            asset_url: "https://github.com/velnor-test-fixtures/java/releases/download/v0.0.1-fixture/java.tar.gz",
            archive_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            binary_sha256: HASH, asset_format: DistributionAssetFormat::TarGzip,
            binary_member: "jdk/bin/java",
            source_repository: "https://github.com/velnor-test-fixtures/java",
            source_commit: "cccccccccccccccccccccccccccccccccccccccc",
            source_tree: "dddddddddddddddddddddddddddddddddddddddd",
            owner: "velnor-test-fixtures/java", version: "0.0.1-fixture",
            selection_version: "0.0.1-fixture", abi: "test-java-materialization-v1",
            provisioning_mode: ProvisioningMode::Official,
            installed_binary_relative_path: Some("installs/http-graalvm-community-jdk/0.0.1-fixture/bin/java"),
            launch_entries: LAUNCH, source_lineage: &[],
            install_plan: Some(QualifiedInstallPlan { backend: QualifiedInstallBackend::MiseHttp,
                strip_components: 1, bin_path: "bin", root_relative_path: ROOT,
                transform_abi: "test-java-http-v1", environment: ENV }),
        }.validate()
    }
}

#[test]
fn audit_debug_never_exposes_installed_capabilities() -> Result<(), crate::MiseError> {
    let audit = QualifiedDistribution::qualify_native(
        DistributionTool::Java,
        DistributionHost::MacosArm64,
        "25.0.4.1.1",
    )?;
    let debug = format!("{audit:?}");
    for forbidden in [
        "http:",
        "installs/",
        "selector",
        "install_plan",
        "JAVA_HOME",
    ] {
        assert!(!debug.contains(forbidden));
    }
    assert!(!audit.launch_entries().is_empty());
    Ok(())
}

#[test]
fn source_release_tags_preserve_their_actual_prefix() -> Result<(), crate::MiseError> {
    let audit = QualifiedDistribution::qualify_native(
        DistributionTool::Java,
        DistributionHost::MacosArm64,
        "25.0.4.1.1",
    )?;
    assert!(
        audit
            .source_lineage()
            .iter()
            .any(|source| source.version() == "graal-25.4.4.1.1")
    );
    let mut record = QualifiedDistribution::require_native(
        DistributionTool::Python,
        DistributionHost::MacosArm64,
        "3.14.8",
    )?;
    let mut lineage = record.source_lineage.to_vec();
    lineage[0].version = "latest";
    record.source_lineage = Box::leak(lineage.into_boxed_slice());
    assert!(record.validate().is_err());
    Ok(())
}

#[test]
fn semver_validator_admits_only_measured_canonical_installation() -> Result<(), crate::MiseError> {
    let record = QualifiedDistribution::require_native(
        DistributionTool::CargoSemverChecks,
        DistributionHost::MacosArm64,
        "0.50.0",
    )?;
    assert_eq!(
        record.selection_version(),
        CARGO_SEMVER_CHECKS_SELECTION_VERSION
    );
    assert_eq!(record.required_install_plan()?.bin_path(), "");
    assert!(
        QualifiedDistribution::require_native(
            DistributionTool::CargoSemverChecks,
            DistributionHost::LinuxAmd64,
            "0.50.0"
        )
        .is_err()
    );
    assert!(
        QualifiedDistribution::require_native(
            DistributionTool::CargoSemverChecks,
            DistributionHost::MacosArm64,
            "0.50.1"
        )
        .is_err()
    );
    let mut changed = record.clone();
    changed.abi = "different-same-version-validator-abi";
    assert_eq!(changed.version(), record.version());
    assert_ne!(
        changed.qualification_digest(),
        record.qualification_digest()
    );
    Ok(())
}

#[test]
fn release_coordinator_is_not_an_owned_publisher_record() -> Result<(), crate::MiseError> {
    let record = QualifiedDistribution::require_native(
        DistributionTool::ReleasePlz,
        DistributionHost::MacosArm64,
        "0.3.169",
    )?;
    assert_eq!(record.selection_version(), RELEASE_PLZ_SELECTION_VERSION);
    assert_eq!(record.provisioning_mode(), ProvisioningMode::Official);
    assert_eq!(record.abi(), "release-plz-cli-v0.3.169");
    assert!(
        QualifiedDistribution::require_native(
            DistributionTool::ReleasePlz,
            DistributionHost::LinuxAmd64,
            "0.3.169"
        )
        .is_err()
    );
    assert!(
        QualifiedDistribution::require_native(
            DistributionTool::ReleasePlz,
            DistributionHost::MacosArm64,
            "0.3.170"
        )
        .is_err()
    );
    Ok(())
}
