use super::super::{daemon_backend_supported, read_daemon_config};
use velnor_runner_host::HostPlatform;

const LINUX_SAMPLE: &str = concat!(
    "schema = 1\n",
    "[github]\n",
    "repository = \"ChainArgos/java-monorepo\"\n",
    "scale_set_name = \"ubuntu-24.04-scale-set\"\n",
    "credential_ref = \"systemd-credential:github-token\"\n",
    "registration_scope = \"repository\"\n",
    "runner_group_id = 1\n",
    "runner_group_name = \"Default\"\n",
    "[host]\n",
    "platform = \"linux\"\n",
    "max_jobs = 1\n",
    "drain_timeout_secs = 900\n",
    "[trust]\n",
    "allowed_repositories = [\"ChainArgos/java-monorepo\"]\n",
    "allowed_events = [\"push\"]\n",
    "allowed_workflow_paths = [\".github/workflows/ci.yml\"]\n",
    "allow_forks = false\n",
    "[runner]\n",
    "image_profile = \"ubuntu-24.04-amd64\"\n",
    "[docker]\n",
    "context = \"system\"\n",
    "platform = \"linux/amd64\"\n",
    "endpoint = \"unix:///var/run/docker.sock\"\n",
);

#[test]
fn linux_config_is_parsed_but_not_routed_through_legacy_admission() -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("velnor-daemon-linux-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let selected = dir.join("host.toml");
    std::fs::write(&selected, LINUX_SAMPLE).map_err(|error| error.to_string())?;
    let config = read_daemon_config(&selected, HostPlatform::Linux)
        .map_err(|error| format!("Linux config failed: {error:?}"))?
        .ok_or("Linux config was treated as missing")?;
    if config.github.credential_ref != "systemd-credential:github-token"
        || config
            .job_trust_policy()
            .map_err(|error| error.to_string())?
            .allowed_workflow_paths
            != [".github/workflows/ci.yml".to_owned()]
        || daemon_backend_supported(HostPlatform::Linux)
    {
        return Err(
            "Linux settings were not validated or admission was not fail-closed".to_owned(),
        );
    }
    std::fs::remove_dir_all(dir).map_err(|error| error.to_string())?;
    Ok(())
}
