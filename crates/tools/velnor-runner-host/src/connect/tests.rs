//! Connect is idempotent. A different binding is not a provider switch.

use crate::{HostConfig, HostError};

pub(super) fn host_toml(repo: &str, context: &str, endpoint: &str, credential: &str) -> String {
    format!(
        concat!(
            "schema = 1\n",
            "[github]\n",
            "repository = \"{repo}\"\n",
            "scale_set_name = \"ubuntu-26.04-scale-set\"\n",
            "credential_ref = \"{credential}\"\n",
            "[host]\n",
            "max_jobs = 1\n",
            "[docker]\n",
            "context = \"{context}\"\n",
            "platform = \"linux/amd64\"\n",
            "endpoint = \"{endpoint}\"\n",
        ),
        repo = repo,
        context = context,
        endpoint = endpoint,
        credential = credential,
    )
}

pub(super) fn stored() -> Result<HostConfig, HostError> {
    HostConfig::parse(&host_toml(
        "ChainArgos/java-monorepo",
        "orbstack",
        "unix:///var/run/docker.sock",
        "keychain:com.tailrocks.velnor.host/chainargos",
    ))
}

mod disconnect_tests;
mod plan_tests;
