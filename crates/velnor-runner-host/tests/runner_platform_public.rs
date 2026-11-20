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
        &[
            "velnor",
            "ubuntu-26.04-arm-scale-set",
            "ubuntu-24.04-arm-scale-set",
        ],
    );
    let parsed = HostConfig::parse(&text)?;
    assert_eq!(parsed.docker.platform, "linux/arm64");
    assert_eq!(HostConfig::parse(&parsed.to_toml()?)?, parsed);
    Ok(())
}

#[test]
fn arm64_and_amd64_never_share_a_selector() {
    let arm = "ubuntu-26.04-arm-scale-set";
    let amd = "ubuntu-26.04-scale-set";
    for (platform, name, extra) in [
        ("linux/arm64", amd, None),
        ("linux/arm64", arm, Some(amd)),
        ("linux/arm64", arm, Some("x64")),
        ("linux/arm64", arm, Some("linux-amd64")),
        ("linux/arm64", arm, Some("ubuntu-26.04-scale-set-x")),
        ("linux/arm64", arm, Some("x-arm-scale-set")),
        ("linux/arm64", "x-arm-scale-set", None),
        ("linux/arm64", "arm-scale-set", None),
        ("linux/amd64", arm, None),
        ("linux/amd64", amd, Some(arm)),
        ("linux/amd64", amd, Some("arm-scale-set")),
        ("linux/amd64", amd, Some("x-arm-scale-set")),
    ] {
        let mut labels = vec!["velnor", name];
        labels.extend(extra);
        assert_eq!(
            HostConfig::parse(&config(platform, name, &labels)),
            Err(HostError::PlatformSelector),
            "{platform} with {labels:?}"
        );
    }
}

#[test]
fn an_unknown_platform_is_an_invalid_config() {
    assert_eq!(
        HostConfig::parse(&config(
            "linux/riscv64",
            "ubuntu-26.04-scale-set",
            &["velnor", "ubuntu-26.04-scale-set"],
        )),
        Err(HostError::Config)
    );
}

fn arm_config(extra: &str) -> String {
    let base = config(
        "linux/arm64",
        "ubuntu-26.04-arm-scale-set",
        &["velnor", "ubuntu-26.04-arm-scale-set"],
    );
    let (head, host) = base.split_once("[host]").unwrap_or((&base, ""));
    format!("{head}{extra}[host]{host}")
}

fn developer_images(runner: char, dind: char, probe: char) -> String {
    format!(
        "[developer_images]\n\
         runner = \"sha256:{}\"\n\
         dind = \"sha256:{}\"\n\
         resource_probe = \"sha256:{}\"\n",
        runner.to_string().repeat(64),
        dind.to_string().repeat(64),
        probe.to_string().repeat(64)
    )
}

#[test]
fn a_development_build_accepts_local_developer_images() -> Result<(), HostError> {
    let parsed = HostConfig::parse(&arm_config(&developer_images('a', 'b', 'c')))?;
    let images = parsed
        .developer_images()
        .ok_or(HostError::DeveloperImages)?;
    assert_eq!(images.runner(), format!("sha256:{}", "a".repeat(64)));
    assert_eq!(
        images.resource_probe(),
        format!("sha256:{}", "c".repeat(64))
    );
    assert_eq!(HostConfig::parse(&parsed.to_toml()?)?, parsed);
    Ok(())
}

#[test]
fn developer_images_are_distinct_local_ids_without_a_release_source() {
    let release_read =
        "[release_read]\ncredential_ref = \"keychain:com.tailrocks.velnor.host/release\"\n";
    for (case, text) in [
        ("shared image", arm_config(&developer_images('a', 'a', 'c'))),
        (
            "release source",
            arm_config(&format!(
                "{release_read}{}",
                developer_images('a', 'b', 'c')
            )),
        ),
        (
            "tag instead of id",
            arm_config(
                "[developer_images]\nrunner = \"velnor-runner:linux-arm64\"\ndind = \"velnor-dind:linux-arm64\"\nresource_probe = \"velnor-resource-probe:linux-arm64\"\n",
            ),
        ),
    ] {
        assert_eq!(HostConfig::parse(&text), Err(HostError::Config), "{case}");
    }
}
