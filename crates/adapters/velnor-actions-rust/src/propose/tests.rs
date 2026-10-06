use super::*;
use velnor_actions_rust_core::profile::NextestProfile;

/// Minimal group exercising every converted field.
pub(super) fn group() -> TaskGroup {
    TaskGroup {
        task_id: "stack/rust/root/clippy/default".to_owned(),
        package_id: "path+file:///repo#demo@0.1.0".to_owned(),
        package_name: "demo".to_owned(),
        manifest_key: "root".to_owned(),
        kind: TaskKind::Clippy,
        configuration: "default".to_owned(),
        features: vec!["a".to_owned()],
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: Some("demo".to_owned()),
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        nextest_profile: NextestProfile::Default,
        run_ignored: None,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
    }
}
mod conversion_tests;
mod dispatch_tests;
