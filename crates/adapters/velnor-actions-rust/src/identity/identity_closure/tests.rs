use super::*;

/// Extension inputs with `lock`, `nextest`, and `rerun` supplied.
pub(super) fn inputs(
    lock: crate::task_identity::DigestSlot,
    nextest: crate::task_identity::DigestSlot,
    rerun: Option<&[String]>,
    build_script: bool,
) -> GroupExtensionInputs<'_> {
    GroupExtensionInputs {
        package_id: "demo",
        workspace_id: "workspace",
        profile: "default",
        manifest: "Cargo.toml",
        graph_digest: "graph",
        targets: &[],
        config_digest: "config",
        lock_digest: lock,
        nextest_digest: nextest,
        archive_source: None,
        rerun_inputs: rerun,
        has_build_script: build_script,
    }
}
/// Minimal group of `kind` with the Nextest runner.
pub(super) fn group(kind: crate::tasks::TaskKind) -> TaskGroup {
    TaskGroup {
        task_id: "stack/rust/root/nextest/default".to_owned(),
        package_id: "demo".to_owned(),
        package_name: "demo".to_owned(),
        manifest_key: "root".to_owned(),
        kind,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: crate::profile::CompileDriver::Cargo,
        test_runner: crate::profile::TestRunner::CargoNextest,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        nextest_profile: crate::profile::NextestProfile::Default,
        run_ignored: None,
    }
}
mod paths_tests;
mod unresolved_tests;
