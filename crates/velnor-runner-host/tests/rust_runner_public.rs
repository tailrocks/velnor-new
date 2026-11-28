// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "rust_runner_public/hardening.rs"]
mod hardening;
#[path = "rust_runner_public/jobs.rs"]
mod jobs;
#[path = "rust_runner_public/orb_calls.rs"]
mod orb_calls;
#[path = "rust_runner_public/removal.rs"]
mod removal;
#[path = "rust_runner_public/store.rs"]
mod store;
#[path = "rust_runner_public/support.rs"]
mod support;

use std::error::Error;

use velnor_runner_host::{HostConfig, HostError, LaunchMode, RustRunnerFailure};

use support::Fixture;

#[test]
fn rust_runner_creates_missing_machine() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(r#"[{"name":"velnor-other"},{"name":"jackin"}]"#)?;
    fixture.launcher().ensure_machine()?;
    let log = fixture.log()?;
    assert_eq!(log[0], "list -f json");
    assert!(
        log.contains(&"create -a arm64 ubuntu velnor".to_owned()),
        "{log:?}"
    );
    fixture.launcher().ensure_machine()?;
    let creates = fixture
        .log()?
        .iter()
        .filter(|line| line.starts_with("create"))
        .count();
    assert_eq!(creates, 1);
    Ok(())
}

#[test]
fn rust_runner_skips_create_when_machine_exists() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new(
        r#"[{"name":"jackin"},{"name":"velnor","image":{"distro":"ubuntu","arch":"arm64"}}]"#,
    )?;
    fixture.launcher().ensure_machine()?;
    assert!(
        fixture
            .log()?
            .iter()
            .all(|line| !line.starts_with("create")),
        "{:?}",
        fixture.log()?
    );
    for listed in [
        r#"[{"name":"velnor","image":{"distro":"debian","arch":"arm64"}}]"#,
        r#"[{"name":"velnor","image":{"distro":"ubuntu","arch":"amd64"}}]"#,
    ] {
        let incompatible = Fixture::new(listed)?;
        assert_eq!(
            incompatible.launcher().ensure_machine().err(),
            Some(HostError::RustRunner(
                RustRunnerFailure::MachineIncompatible
            )),
            "{listed}"
        );
        assert!(
            incompatible.log()?.iter().all(|line| {
                !line.starts_with("create") && !line.contains(" ln ") && !line.contains(" test ")
            }),
            "{:?}",
            incompatible.log()?
        );
    }
    Ok(())
}

#[test]
fn rust_runner_links_var_lib_velnor_to_host_store() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new("[]")?;
    fixture.launcher().ensure_machine()?;
    let expected = format!(
        "-m velnor -u root ln -sfnT /mnt/mac{} /var/lib/velnor",
        fixture.store.display()
    );
    assert!(fixture.log()?.contains(&expected), "{:?}", fixture.log()?);
    for name in ["bin", "crates", "tools", "git", "gen", "cache", "records"] {
        assert!(fixture.store.join(name).is_dir(), "{name}");
    }
    Ok(())
}

#[test]
fn rust_runner_fails_when_binary_missing() -> Result<(), Box<dyn Error>> {
    let fixture =
        Fixture::new(r#"[{"name":"velnor","image":{"distro":"ubuntu","arch":"arm64"}}]"#)?;
    std::fs::remove_file(fixture.store.join("bin/velnor-runner"))?;
    let error = fixture
        .launcher()
        .ensure_machine()
        .err()
        .ok_or("ensure_machine accepted a missing binary")?;
    assert_eq!(
        error,
        HostError::RustRunner(RustRunnerFailure::BinaryMissing)
    );
    assert_eq!(
        error.to_string(),
        "velnor-runner is missing at /var/lib/velnor/bin/velnor-runner in the velnor machine"
    );
    Ok(())
}

fn host_config(launch_mode: Option<&str>, platform: &str, name: &str, labels: &[&str]) -> String {
    let labels = labels
        .iter()
        .map(|label| format!("\"{label}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let launch_mode =
        launch_mode.map_or_else(String::new, |mode| format!("launch_mode = \"{mode}\"\n"));
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
         {launch_mode}\
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

const MAC_LABELS: [&str; 2] = ["velnor", "velnor-mac"];

#[test]
fn launch_mode_defaults_to_official_runner() -> Result<(), HostError> {
    let parsed = HostConfig::parse(&host_config(
        None,
        "linux/amd64",
        "ubuntu-26.04-scale-set",
        &["velnor", "ubuntu-26.04-scale-set"],
    ))?;
    assert_eq!(parsed.host.launch_mode, LaunchMode::OfficialRunner);
    assert!(
        parsed
            .to_toml()?
            .contains("launch_mode = \"official-runner\"")
    );
    Ok(())
}

#[test]
fn launch_mode_rust_runner_parses() -> Result<(), HostError> {
    let text = host_config(
        Some("rust-runner"),
        "linux/arm64",
        "velnor-mac",
        &MAC_LABELS,
    );
    let parsed = HostConfig::parse_draft(&text)?;
    assert_eq!(parsed.host.launch_mode, LaunchMode::RustRunner);
    assert_eq!(parsed.to_toml().err(), Some(HostError::Config));
    assert_eq!(HostConfig::parse(&text).err(), Some(HostError::Config));
    Ok(())
}

#[test]
fn launch_mode_unknown_value_is_rejected() {
    for mode in ["docker", "rust_runner", "Rust-Runner", ""] {
        assert_eq!(
            HostConfig::parse(&host_config(
                Some(mode),
                "linux/arm64",
                "velnor-mac",
                &MAC_LABELS
            ))
            .err(),
            Some(HostError::Config),
            "{mode}"
        );
    }
}

#[test]
fn rust_runner_mode_accepts_velnor_mac_selector_and_requires_arm64() {
    assert!(
        HostConfig::parse_draft(&host_config(
            Some("rust-runner"),
            "linux/arm64",
            "velnor-mac",
            &MAC_LABELS
        ))
        .is_ok()
    );
    assert_eq!(
        HostConfig::parse_draft(&host_config(
            Some("rust-runner"),
            "linux/amd64",
            "velnor-mac",
            &MAC_LABELS
        ))
        .err(),
        Some(HostError::Config)
    );
    assert_eq!(
        HostConfig::parse(&host_config(
            Some("official-runner"),
            "linux/arm64",
            "velnor-mac",
            &MAC_LABELS
        ))
        .err(),
        Some(HostError::PlatformSelector)
    );
}

#[test]
fn rust_runner_bounds_the_output_it_captures_from_orb() -> Result<(), Box<dyn Error>> {
    removal::orb_output_over_the_capture_bound()
}

#[test]
fn rust_runner_allows_one_controller_per_host_store() -> Result<(), Box<dyn Error>> {
    removal::one_controller_per_host_store()
}

#[test]
fn rust_runner_ignores_a_host_store_override_in_the_environment() -> Result<(), Box<dyn Error>> {
    removal::ignores_a_store_override_in_the_environment(
        "rust_runner_ignores_a_host_store_override_in_the_environment",
    )
}
