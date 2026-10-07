use super::super::{daemon_backend_supported, read_daemon_config, read_daemon_config_with};
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
fn linux_config_preflight_uses_the_package_path_and_systemd_credential() -> Result<(), String> {
    let config = read_daemon_config_with(
        std::path::Path::new(velnor_runner_host::LINUX_CONFIG_PATH),
        HostPlatform::Linux,
        |path, platform| {
            if path != std::path::Path::new(velnor_runner_host::LINUX_CONFIG_PATH)
                || platform != HostPlatform::Linux
            {
                return Err(());
            }
            Ok(Some(LINUX_SAMPLE.to_owned()))
        },
    )
    .map_err(|error| format!("Linux config failed: {error:?}"))?
    .ok_or("Linux config was treated as missing")?;
    if config.github.credential_ref != "systemd-credential:github-token"
        || config.github.scale_set_name != "ubuntu-24.04-scale-set"
        || config
            .runner
            .as_ref()
            .map(|runner| runner.image_profile.as_str())
            != Some("ubuntu-24.04-amd64")
        || config.docker.platform != "linux/amd64"
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
    Ok(())
}

#[test]
fn linux_daemon_config_reader_rejects_a_non_package_path_before_reading() {
    let path =
        std::env::temp_dir().join(format!("velnor-daemon-linux-{}.toml", std::process::id()));
    assert!(matches!(
        read_daemon_config(&path, HostPlatform::Linux),
        Err(super::super::ConfigReadError::Unreadable)
    ));
}
