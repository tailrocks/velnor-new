use super::{resolve_on, rust};
use velnor_actions_contract_config::config::CheckPlatform;

fn discovered_qualification() -> velnor_actions_mise_core::checks::DiscoveredCheck {
    use velnor_actions_contract_config::config::{CheckExecutor, CheckRunner, MiseCheck};
    let resolved = resolve_on(&[rust()], &["rust".to_owned()], CheckPlatform::LinuxX64)
        .expect("qualified Rust");
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
        inputs: Vec::new(),
        tools: vec!["rust".to_owned()],
        system_tools: Vec::new(),
        evidence: None,
        timeout_minutes: 10,
    };
    let proposal = super::super::super::propose_check(
        &check,
        "native-task-source",
        &[],
        &resolved.specs,
        &resolved.fingerprint,
    )
    .expect("opaque proposal");
    super::super::super::DiscoveredCheck {
        check,
        proposal,
        config_inputs: Vec::new(),
        task_config: String::new(),
        tool_specs: resolved.specs,
        qualified_tools: resolved.declarations,
        qualification_digest: resolved.fingerprint,
        config_source: String::new(),
    }
}
#[test]
fn altered_qualification_cannot_retain_the_discovered_proposal_binding() {
    let original = discovered_qualification();
    original
        .verify_qualification_identity()
        .expect("exact binding");
    let mut changed = original.clone();
    changed.qualified_tools[0].platforms[0].install_tree_sha256 = "e".repeat(64);
    assert!(changed.verify_qualification_identity().is_err());
    let mut changed = original.clone();
    changed.tool_specs = vec!["rust@1.98.1".to_owned()];
    assert!(changed.verify_qualification_identity().is_err());
    let mut changed = original;
    changed
        .proposal
        .identity
        .flags
        .retain(|flag| !flag.starts_with("qualified_tools:"));
    assert!(changed.verify_qualification_identity().is_err());
}
#[test]
fn every_host_container_field_changes_the_runner_identity() {
    use velnor_actions_contract_config::config::{
        ContainerPlatform, DaemonIdentityPolicy, HostContainerProfile, HostDockerCli,
        HostDockerDaemon,
    };
    let mut item = discovered_qualification();
    item.check.runner.container = Some(HostContainerProfile::Docker {
        context: "default".to_owned(),
        socket_path: "/var/run/docker.sock".to_owned(),
        socket_uid: 0,
        cli: HostDockerCli {
            path: "/usr/local/bin/docker".to_owned(),
            sha256: "a".repeat(64),
            version: "28.5.1".to_owned(),
            build: "deadbee".to_owned(),
        },
        daemon: HostDockerDaemon {
            version: "28.5.1".to_owned(),
            platform: ContainerPlatform::LinuxX64,
            operating_system: "Ubuntu 24.04".to_owned(),
            identity_policy: DaemonIdentityPolicy::ExecutionScoped,
        },
    });
    let flags = |item: &super::super::super::DiscoveredCheck| {
        super::super::super::check_identity(
            &item.check,
            "native-task-source",
            &[],
            &item.tool_specs,
            &item.qualification_digest,
        )
        .expect("complete runner identity")
        .flags
    };
    let original = flags(&item);
    for field in 0..10 {
        let mut changed = item.clone();
        let Some(HostContainerProfile::Docker {
            context,
            socket_path,
            socket_uid,
            cli,
            daemon,
        }) = &mut changed.check.runner.container
        else {
            panic!("typed Docker fixture");
        };
        match field {
            0 => *context = "isolated".to_owned(),
            1 => *socket_path = "/run/user/1000/docker.sock".to_owned(),
            2 => cli.path = "/opt/docker/bin/docker".to_owned(),
            3 => cli.sha256 = "b".repeat(64),
            4 => cli.version = "28.5.2".to_owned(),
            5 => cli.build = "feedbee".to_owned(),
            6 => daemon.version = "28.5.2".to_owned(),
            7 => daemon.platform = ContainerPlatform::LinuxArm64,
            8 => daemon.operating_system = "Debian 13".to_owned(),
            9 => *socket_uid = 1000,
            _ => unreachable!(),
        }
        assert_ne!(original, flags(&changed), "unbound container field {field}");
    }
}
