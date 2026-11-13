// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_host::{HostConfig, HostError};

fn config(platform: &str, name: &str, labels: &[&str]) -> String {
    let labels = labels
        .iter()
        .map(|label| format!("\"{label}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "schema = 2\n\
         [[connections]]\n\
         connection_id = \"lab\"\n\
         repository_id = 42\n\
         repository = \"tailrocks/velnor-lab\"\n\
         credential_ref = \"keychain:com.tailrocks.velnor.host/lab\"\n\
         [connections.scale_set]\n\
         name = \"{name}\"\n\
         labels = [{labels}]\n\
         [connections.source_policy]\n\
         mode = \"branch_restricted\"\n\
         branches = [\"main\"]\n\
         tag_prefixes = []\n\
         events = [\"push\"]\n\
         pull_request_rules = []\n\
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
         platform = \"{platform}\"\n\
         endpoint = \"unix:///var/run/docker.sock\"\n"
    )
}

#[test]
fn amd64_host_keeps_the_amd64_selector() -> Result<(), HostError> {
    let parsed = HostConfig::parse(&config(
        "linux/amd64",
        "ubuntu-26.04-scale-set",
        &["velnor", "ubuntu-26.04-scale-set"],
    ))?;
    assert_eq!(parsed.docker.platform, "linux/amd64");
    Ok(())
}

#[test]
fn arm64_host_is_accepted_with_an_arm_selector() -> Result<(), HostError> {
    let text = config(
        "linux/arm64",
        "ubuntu-26.04-arm-scale-set",
        &["velnor", "ubuntu-26.04-arm-scale-set"],
    );
    let parsed = HostConfig::parse(&text)?;
    assert_eq!(parsed.docker.platform, "linux/arm64");
    assert_eq!(HostConfig::parse(&parsed.to_toml()?)?, parsed);
    Ok(())
}

#[test]
fn arm64_and_amd64_never_share_a_selector() {
    for (platform, name, labels) in [
        (
            "linux/arm64",
            "ubuntu-26.04-scale-set",
            &["velnor", "ubuntu-26.04-scale-set"][..],
        ),
        (
            "linux/arm64",
            "ubuntu-26.04-arm-scale-set",
            &[
                "velnor",
                "ubuntu-26.04-arm-scale-set",
                "ubuntu-26.04-scale-set",
            ][..],
        ),
        (
            "linux/amd64",
            "ubuntu-26.04-arm-scale-set",
            &["velnor", "ubuntu-26.04-arm-scale-set"][..],
        ),
        (
            "linux/amd64",
            "ubuntu-26.04-scale-set",
            &[
                "velnor",
                "ubuntu-26.04-scale-set",
                "ubuntu-26.04-arm-scale-set",
            ][..],
        ),
        (
            "linux/riscv64",
            "ubuntu-26.04-scale-set",
            &["velnor", "ubuntu-26.04-scale-set"][..],
        ),
    ] {
        assert!(
            HostConfig::parse(&config(platform, name, labels)).is_err(),
            "accepted {platform} with {name} {labels:?}"
        );
    }
}
