use super::{plan_with_env, plan_with_mount};
use crate::{HostError, audit_plan, runner_plan};

#[test]
fn audit_rejects_host_and_secret_mounts() {
    let sources = [
        "/Users/me/src",
        "/users/me/src",
        "/Users",
        "/home/me/src",
        "/HOME/me/src",
        "/home",
        "/var/run/docker.sock",
        "/VAR/RUN/DOCKER.SOCK",
        "/run",
        "/var/run",
        "/private/var/run",
        "/tmp/ssh-agent.sock",
        "/tmp/SSH-AGENT.sock",
        "/Users/me/.ssh/id_ed25519",
        "/Users/me/Library/Keychains/login.keychain-db",
        "/Users/me/Library/Application Support/Velnor/host.toml",
        "/users/me/library/application support/velnor/host.toml",
        "/.orbstack/run/docker.sock",
        "/opt/orbstack/run/docker.sock",
        "/var/run/orbstack.sock",
        "/Users/me/.orbstack/run/docker.sock",
    ];
    for source in sources {
        let plan = plan_with_mount(source, "/mnt");
        assert_eq!(
            audit_plan(&plan),
            Err(HostError::ForbiddenMount),
            "{source}"
        );
    }
    assert!(audit_plan(&plan_with_mount("/opt/homebrew", "/opt/homebrew")).is_ok());
}

#[test]
fn docker_sock_is_only_a_private_volume() -> Result<(), HostError> {
    let plan = runner_plan("priv")?;
    assert_eq!(plan.mounts[0].source, "volume:priv");
    assert_eq!(plan.mounts[0].target, "/run");
    assert!(audit_plan(&plan).is_ok());
    let rejected = [
        ("/var/run/docker.sock", "/var/run/docker.sock"),
        ("/run/docker.sock", "/var/run/docker.sock"),
        (
            "/Users/me/.orbstack/run/docker.sock",
            "/var/run/docker.sock",
        ),
        ("volume:", "/var/run/docker.sock"),
        ("volume:a/b", "/var/run/docker.sock"),
        ("volume:../sock", "/var/run/docker.sock"),
        ("volume:/var/run/docker.sock", "/run"),
    ];
    for (source, target) in rejected {
        assert_eq!(
            audit_plan(&plan_with_mount(source, target)),
            Err(HostError::ForbiddenMount),
            "{source}"
        );
    }
    Ok(())
}

#[test]
fn audit_rejects_host_and_token_env() {
    let entries = [
        "HOME=/Users/me",
        "HOME=/home/me",
        "HOME=/home",
        "SSH_AUTH_SOCK=/tmp/ssh-agent.sock",
        "SSH_AGENT_PID=9",
        "KEYCHAIN_PATH=/Users/me/Library/Keychains/login.keychain-db",
        "VELNOR_HOST=/Users/me/Library/Application Support/Velnor/host.toml",
        "GITHUB_TOKEN=ghs_example",
        "GH_TOKEN=ghs_example",
        "ACTIONS_RUNTIME_TOKEN=secret",
        "GH_PAT=secret",
        "DOCKER_HOST=unix:///var/run/docker.sock",
        "DOCKER_HOST=unix:///Users/me/.orbstack/run/docker.sock",
        "X=--jitconfig secret",
    ];
    for entry in entries {
        assert_eq!(
            audit_plan(&plan_with_env(entry)),
            Err(HostError::ForbiddenMount),
            "{entry}"
        );
    }
}
