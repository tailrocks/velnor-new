// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::validate_transition;
use crate::config::HostConfig;
use crate::error::HostError;

fn config(developer_images: &str) -> Result<HostConfig, HostError> {
    HostConfig::parse(&format!(
        "schema = 2\n\
         [[connections]]\n\
         connection_id = \"lab\"\n\
         repository_id = 42\n\
         repository = \"tailrocks/velnor-lab\"\n\
         credential_ref = \"keychain:com.tailrocks.velnor.host/lab\"\n\
         [connections.scale_set]\n\
         name = \"ubuntu-26.04-arm-scale-set\"\n\
         labels = [\"velnor\", \"ubuntu-26.04-arm-scale-set\"]\n\
         [connections.source_policy]\n\
         mode = \"branch_restricted\"\n\
         branches = [\"main\"]\n\
         tag_prefixes = []\n\
         events = [\"push\"]\n\
         pull_request_rules = []\n\
         {developer_images}\
         [host]\n\
         max_jobs = 1\n\
         [host.resources]\n\
         runner_cpu_millicores = 2000\n\
         runner_memory_bytes = 4294967296\n\
         dind_cpu_millicores = 1000\n\
         dind_memory_bytes = 2147483648\n\
         operational_memory_reserve_bytes = 1073741824\n\
         [docker]\n\
         context = \"orbstack\"\n\
         platform = \"linux/arm64\"\n\
         endpoint = \"unix:///var/run/docker.sock\"\n"
    ))
}

fn images(runner: char) -> String {
    format!(
        "[developer_images]\nrunner = \"sha256:{}\"\ndind = \"sha256:{}\"\nresource_probe = \"sha256:{}\"\n",
        runner.to_string().repeat(64),
        "b".repeat(64),
        "c".repeat(64)
    )
}

#[test]
fn developer_images_change_only_on_restart() -> Result<(), HostError> {
    let current = config(&images('a'))?;
    assert_eq!(validate_transition(&current, &current.clone()), Ok(()));
    for next in [config(&images('d'))?, config("")?] {
        assert_eq!(validate_transition(&current, &next), Err(HostError::Config));
    }
    Ok(())
}
