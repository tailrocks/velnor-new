use super::{ValidatedHostConfigSnapshot, snapshot_from_bytes, snapshot_from_optional_bytes};
use crate::{HostPlatform, RegistrationScope};
use velnor_runner_host_config::HostConfig;
use velnor_runner_journal::HostError;

const LINUX_CONFIG: &str = concat!(
    "schema = 1\n",
    "[github]\n",
    "repository = \"ChainArgos/java-monorepo\"\n",
    "scale_set_name = \"ubuntu-26.04-scale-set\"\n",
    "credential_ref = \"systemd-credential:github-token\"\n",
    "registration_scope = \"repository\"\n",
    "runner_group_id = 1\n",
    "runner_group_name = \"Default\"\n",
    "[host]\n",
    "platform = \"linux\"\n",
    "max_jobs = 1\n",
    "drain_timeout_secs = 1800\n",
    "[trust]\n",
    "allowed_repositories = [\"ChainArgos/java-monorepo\"]\n",
    "allowed_events = [\"push\", \"pull_request\"]\n",
    "allowed_workflow_paths = [\".github/workflows/ci.yml\"]\n",
    "allow_forks = false\n",
    "[runner]\n",
    "image_profile = \"ubuntu-26.04-amd64\"\n",
    "[docker]\n",
    "context = \"system\"\n",
    "platform = \"linux/amd64\"\n",
    "endpoint = \"unix:///var/run/docker.sock\"\n",
);

fn snapshot(bytes: &[u8]) -> Result<ValidatedHostConfigSnapshot, HostError> {
    snapshot_from_bytes(bytes, HostPlatform::Linux)
}

#[test]
fn linux_snapshot_accepts_cleanup_config_without_runnable_image_profile() -> Result<(), HostError> {
    assert!(matches!(
        snapshot(
            &LINUX_CONFIG
                .replace("ubuntu-26.04-scale-set", "ubuntu-24.04-scale-set")
                .replace("ubuntu-26.04-amd64", "ubuntu-24.04-amd64")
                .into_bytes()
        ),
        Err(HostError::Config)
    ));
    let configured_26 = snapshot(LINUX_CONFIG.as_bytes())?;
    assert_eq!(
        configured_26
            .scale_set_binding()
            .runner_image_profile
            .as_deref(),
        Some("ubuntu-26.04-amd64")
    );
    assert!(configured_26.runner_image_profile().is_none());
    Ok(())
}

#[test]
fn snapshot_missing_file_is_an_error_only_for_linux() {
    assert!(matches!(
        snapshot_from_optional_bytes(None, HostPlatform::Linux),
        Err(HostError::Config)
    ));
    assert!(matches!(
        snapshot_from_optional_bytes(None, HostPlatform::Macos),
        Ok(None)
    ));
}

#[test]
fn snapshot_rejects_wrong_platform_invalid_utf8_and_repository_downgrade() -> Result<(), HostError>
{
    assert!(matches!(
        snapshot_from_bytes(LINUX_CONFIG.as_bytes(), HostPlatform::Macos),
        Err(HostError::Config)
    ));
    assert!(matches!(
        snapshot_from_bytes(&[0xff], HostPlatform::Linux),
        Err(HostError::Config)
    ));
    let organization = LINUX_CONFIG.replace(
        "registration_scope = \"repository\"\n",
        "registration_scope = \"organization\"\nregistration_scope_name = \"ChainArgos\"\n",
    );
    let config = HostConfig::parse(&organization)?;
    config.validate_for_host(HostPlatform::Linux)?;
    let binding = config.scale_set_binding()?;
    assert_eq!(
        binding.scope,
        RegistrationScope::Organization {
            organization: "ChainArgos".to_owned(),
        }
    );
    Ok(())
}
