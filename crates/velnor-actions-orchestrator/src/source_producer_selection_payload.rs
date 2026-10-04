//! Reuse the Rust adapter's argv owner to prove selected descriptor semantics.

use velnor_actions_contract::ProposedTask;
use velnor_actions_rust::{
    CompileDriver, NextestProfile, PackageRecord, TaskGroup, TaskKind, TestRunner,
};

/// Any unrecognized command shape retains complete workspace acquisition.
pub(super) fn matches(task: &ProposedTask, package: &PackageRecord) -> bool {
    let Some(group) = reconstructed(task, package) else {
        return false;
    };
    velnor_actions_rust::propose_task(&group).is_ok_and(|expected| {
        expected.payload == task.payload
            && expected.identity.environment == task.identity.environment
    })
}

fn reconstructed(task: &ProposedTask, package: &PackageRecord) -> Option<TaskGroup> {
    if velnor_actions_rust::manifest_for_key(&task.identity.unit_key) != package.manifest
        || task.identity.flags.iter().any(|flag| {
            !matches!(
                flag.as_str(),
                "--lib" | "--bins" | "--tests" | "--examples" | "--benches"
            )
        })
    {
        return None;
    }
    let kind = TaskKind::parse(&task.task_kind).ok()?;
    if kind == TaskKind::Fmt {
        return None;
    }
    Some(TaskGroup {
        task_id: task.task_id.clone(),
        package_id: package.id.clone(),
        package_name: package.name.clone(),
        manifest_key: task.identity.unit_key.clone(),
        kind,
        configuration: task.configuration.clone(),
        features: task.identity.features.clone(),
        target: task.identity.target.clone(),
        gated_by: task.gated_by.clone(),
        depends_on: task.depends_on.clone(),
        target_flags: task.identity.flags.clone(),
        no_test_targets: task.no_targets,
        package_arg: Some(package.name.clone()),
        compile_driver: CompileDriver::parse(&task.identity.compile_driver).ok()?,
        test_runner: TestRunner::parse(&task.identity.test_runner).ok()?,
        nextest_profile: NextestProfile::parse(&task.runner_profile).ok()?,
        declared_inputs: task.identity.declared_inputs.clone(),
        undeclared_reads: task.identity.undeclared_reads,
        uses_network: task.resource.needs_network,
        uses_clock: task.uses_clock,
        uses_random: task.uses_random,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn fixture(driver: CompileDriver, features: &[&str]) -> (PackageRecord, ProposedTask) {
        let package = PackageRecord {
            id: "demo@1.0.0".to_owned(),
            name: "demo".to_owned(),
            version: "1.0.0".to_owned(),
            manifest: "Cargo.toml".to_owned(),
            external: false,
            in_workspace: true,
            targets: Vec::new(),
            features: features.iter().map(|value| (*value).to_owned()).collect(),
            has_build_script: false,
        };
        let group = TaskGroup {
            task_id: "stack/rust/root/clippy/default".to_owned(),
            package_id: package.id.clone(),
            package_name: package.name.clone(),
            manifest_key: "root".to_owned(),
            kind: TaskKind::Clippy,
            configuration: "default".to_owned(),
            features: features.iter().map(|value| (*value).to_owned()).collect(),
            target: "host".to_owned(),
            gated_by: Vec::new(),
            depends_on: Vec::new(),
            target_flags: Vec::new(),
            no_test_targets: false,
            package_arg: Some(package.name.clone()),
            compile_driver: driver,
            test_runner: TestRunner::CargoTest,
            nextest_profile: NextestProfile::Default,
            declared_inputs: Vec::new(),
            undeclared_reads: false,
            uses_network: false,
            uses_clock: false,
            uses_random: false,
        };
        (
            package,
            velnor_actions_rust::propose_task(&group).expect("adapter proposal"),
        )
    }

    #[test]
    fn actual_cargo_and_mbx_shapes_qualify_default_and_named_features() {
        for driver in [CompileDriver::Cargo, CompileDriver::Mbx] {
            for features in [&["default"][..], &["alpha"][..]] {
                let (package, task) = fixture(driver, features);
                assert!(matches(&task, &package));
            }
        }
    }

    #[test]
    fn mismatched_feature_target_package_and_manifest_values_fail_closed() {
        let (package, task) = fixture(CompileDriver::Cargo, &["default"]);
        for extra in [
            vec!["--features", "alpha"],
            vec!["--target", "aarch64-apple-darwin"],
            vec!["--package", "other"],
            vec!["--manifest-path", "other/Cargo.toml"],
        ] {
            let mut changed = task.clone();
            changed
                .payload
                .splice(1..1, extra.into_iter().map(OsString::from));
            assert!(!matches(&changed, &package));
        }
        let (package, mut named) = fixture(CompileDriver::Mbx, &["alpha"]);
        let index = named
            .payload
            .iter()
            .position(|value| value == "alpha")
            .expect("features");
        named.payload[index] = OsString::from("extra");
        assert!(!matches(&named, &package));
    }
}
