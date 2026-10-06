//! Opaque checks bind source, configuration and their own target identity.
use super::*;
use tempfile::TempDir;
use velnor_actions_contract::{
    CheckExecutor, CheckPlatform, CheckRunner, MiseCheck, PlanGenerator,
};
fn fixture() -> (TempDir, MiseCheck) {
    let dir = TempDir::new().expect("temporary repository");
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tasks.verify]\nrun = 'echo check'\n",
    )
    .expect("native task");
    std::fs::write(dir.path().join("input.txt"), "first").expect("input");
    let check = MiseCheck {
        id: "verify".to_owned(),
        task: "verify".to_owned(),
        directory: ".".to_owned(),
        runner: CheckRunner {
            label: "ubuntu-24.04".to_owned(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: vec!["input.txt".to_owned()],
        tools: Vec::new(),
        system_tools: Vec::new(),
        evidence: None,
        timeout_minutes: 10,
    };
    (dir, check)
}
fn discovered_check(root: &Path, check: &MiseCheck) -> DiscoveredCheck {
    velnor_actions_mise::discover_checks(root, std::slice::from_ref(check), &[])
        .expect("static checks")
        .remove(0)
}
fn generator() -> PlanGenerator {
    PlanGenerator {
        version: "0.1.0".to_owned(),
        target: "x86_64-unknown-linux-gnu".to_owned(),
        sha256: "a".repeat(64),
    }
}

mod named_checks_tests;
