//! Rust task prerequisites stay in one package and configuration.

use std::collections::BTreeMap;

use velnor_actions_rust::{
    CompileDriver, DeriveInputs, NextestProfile, PackageRecord, ProfileSource,
    RustExecutionProfile, TargetRecord, TestRunner, VelnorV1TaskOwner, derive_task_groups,
};

#[test]
fn production_task_prerequisites_stay_with_package_and_configuration()
-> Result<(), Box<dyn std::error::Error>> {
    let profile = profile();
    let features = vec!["default".to_owned()];
    let mut groups = Vec::new();
    for manifest in [
        "Cargo.toml",
        "crates/a/Cargo.toml",
        "nested/crates/a/Cargo.toml",
        "tools/b/Cargo.toml",
    ] {
        let package = package(manifest);
        for configuration in ["default", "all-features"] {
            groups.extend(derive_task_groups(&DeriveInputs {
                package: &package,
                profile: &profile,
                configuration,
                features: &features,
                all_features: configuration == "all-features",
                target: "host",
                explicit_fmt: true,
            })?);
        }
    }
    let owners: BTreeMap<_, _> = groups
        .iter()
        .map(|group| {
            (
                group.task_id.as_str(),
                (group.package_id.as_str(), group.configuration.as_str()),
            )
        })
        .collect();
    assert_eq!(owners.len(), groups.len());
    let mut edges = 0;
    for group in &groups {
        for predecessor in group.gated_by.iter().chain(&group.depends_on) {
            assert_eq!(
                owners.get(predecessor.as_str()),
                owners.get(group.task_id.as_str())
            );
            edges += 1;
        }
    }
    assert_eq!(edges, 48);
    Ok(())
}

fn package(manifest: &str) -> PackageRecord {
    PackageRecord {
        id: manifest.to_owned(),
        name: "demo".to_owned(),
        version: "0.1.0".to_owned(),
        manifest: manifest.to_owned(),
        external: false,
        in_workspace: true,
        targets: vec![TargetRecord {
            kind: "lib".to_owned(),
            name: "demo".to_owned(),
            test: true,
            doctest: true,
            required_features: Vec::new(),
        }],
        features: vec!["default".to_owned()],
        has_build_script: false,
        v1_task_owner: VelnorV1TaskOwner::Project,
    }
}

fn profile() -> RustExecutionProfile {
    RustExecutionProfile {
        compile_driver: CompileDriver::Mbx,
        test_runner: TestRunner::CargoNextest,
        evidence: Vec::new(),
        driver_source: ProfileSource::Detected,
        runner_source: ProfileSource::Detected,
        nextest_profile: NextestProfile::Default,
        nextest_config: None,
        run_ignored: None,
    }
}
