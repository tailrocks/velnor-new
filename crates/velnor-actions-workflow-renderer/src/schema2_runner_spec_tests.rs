use super::{RunnerSpec, Yaml};
use velnor_actions_contract::ReleaseTarget;

#[test]
fn qualification_hosted_target_accepts_only_pinned_x64_runner_pairs() {
    for (label, target) in [
        ("ubuntu-26.04", ReleaseTarget::LinuxX86_64),
        ("macos-15-intel", ReleaseTarget::MacosX86_64),
    ] {
        let runner = RunnerSpec::hosted_release_target(label, target)
            .expect("fixed qualification runner is valid");
        assert_eq!(runner.runs_on, Yaml::str(label));
    }

    for (label, target) in [
        ("macos-26-intel", ReleaseTarget::MacosX86_64),
        ("ubuntu-26.04", ReleaseTarget::MacosX86_64),
        ("macos-15-intel", ReleaseTarget::LinuxX86_64),
        ("macos-15", ReleaseTarget::MacosArm64),
    ] {
        assert!(
            RunnerSpec::hosted_release_target(label, target).is_err(),
            "unexpected runner pair accepted: {label} / {}",
            target.triple()
        );
    }
}

#[test]
fn service_host_matches_the_job_network() {
    let hosted = RunnerSpec::hosted("ubuntu-26.04").expect("hosted label");
    let scale = RunnerSpec::scale_set(Yaml::str("linux-amd64"));
    assert_eq!(hosted.service_host(false), "127.0.0.1");
    assert_eq!(hosted.service_host(true), "redis");
    assert_eq!(scale.service_host(false), "redis");
}
