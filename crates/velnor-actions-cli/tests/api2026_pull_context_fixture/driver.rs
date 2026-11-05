// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::Scenario;
use std::{error::Error, process::Command};

pub(super) fn dispatch(scenario: Scenario) -> Result<(), Box<dyn Error>> {
    let name = scenario.test_name();
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/api2026_pull_context_fixture/driver.py"
    );
    let status = Command::new("python3")
        .arg(script)
        .arg(name)
        .arg(if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        })
        .status()?;
    if !status.success() {
        return Err(format!("isolated GitHub fixture failed: {status}").into());
    }
    Ok(())
}

impl Scenario {
    fn test_name(self) -> &'static str {
        match self {
            #[cfg(not(debug_assertions))]
            Self::ReleaseSentinel => {
                "release_consumer_manifest_sentinel_never_bypasses_executable_binding"
            }
            Self::Valid => "absent_rest_merge_sha_reaches_owned_locator_gate",
            Self::ReversedParents => "reversed_api_merge_parents_are_rejected",
            Self::ChangedRef => "changed_merge_ref_is_rejected",
            Self::MergePendingReady => "pending_mergeability_refreshes_before_trusting_merge_ref",
            Self::MergePendingExhausted => "unresolved_mergeability_never_produces_a_plan",
            Self::MergePendingFalse => "conflicting_pull_request_finishes_mergeability_refresh",
            Self::MergePendingChanged => {
                "pull_request_changed_during_mergeability_refresh_is_rejected"
            }
            Self::MergePendingDeadline | Self::MergePendingApiDeadline => {
                "mergeability_polling_obeys_the_public_operation_deadline"
            }
            Self::HttpFailure => "github_api_http_failure_reports_safe_context",
            Self::StdoutLimit => "github_api_stdout_limit_reports_safe_context",
            Self::StderrLimit => "github_api_stderr_limit_reports_safe_context",
            Self::Deadline => "github_api_deadline_reports_safe_context",
        }
    }
}

pub(super) fn validate_worker(scenario: Option<Scenario>) -> Result<(), Box<dyn Error>> {
    use std::net::ToSocketAddrs as _;
    let marker = std::env::var("VELNOR_GH_FIXTURE_WORKER")?;
    let name = std::env::var("VELNOR_GH_FIXTURE_SCENARIO")?;
    if marker != "container-v1"
        || scenario.is_some_and(|value| value.test_name() != name)
        || !std::path::Path::new("/.dockerenv").is_file()
        || std::env::consts::OS != "linux"
        || rustix::process::geteuid().is_root()
        || std::env::var_os("MISE_DATA_DIR").as_deref() != Some(std::ffi::OsStr::new("/tools/mise"))
        || std::env::var_os("VELNOR_GH_TEST_TLS_ROOT").as_deref()
            != Some(std::ffi::OsStr::new("/tmp/fixture-tls"))
    {
        return Err("invalid isolated GitHub fixture worker".into());
    }
    if std::env::var("VELNOR_GH_FIXTURE_ARCH")? != std::env::consts::ARCH {
        return Err("isolated GitHub fixture architecture mismatch".into());
    }
    let interfaces = std::fs::read_dir("/sys/class/net")?
        .map(|entry| entry.map(|value| value.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    if interfaces.len() != 1 || interfaces[0] != "lo" {
        return Err("GitHub fixture network is not isolated".into());
    }
    let route = std::fs::read_to_string("/proc/net/route")?;
    let addresses = ("api.github.com", 443)
        .to_socket_addrs()?
        .collect::<Vec<_>>();
    if route
        .lines()
        .skip(1)
        .any(|line| line.split_whitespace().nth(1) == Some("00000000"))
        || addresses.is_empty()
        || addresses.iter().any(|addr| !addr.ip().is_loopback())
    {
        return Err("GitHub fixture network is not isolated".into());
    }
    let pin = match std::env::consts::ARCH {
        "x86_64" => velnor_actions_mise::catalog::QUALIFIED_GH_LINUX_AMD64,
        "aarch64" => velnor_actions_mise::catalog::QUALIFIED_GH_LINUX_ARM64,
        _ => return Err("unsupported isolated GitHub fixture architecture".into()),
    };
    let manifest: serde_json::Value = serde_json::from_str(include_str!("tools.json"))?;
    if manifest["gh_version"].as_str() != Some(pin.version)
        || manifest[std::env::consts::ARCH]["gh_sha256"].as_str() != Some(pin.sha256)
    {
        return Err("GitHub fixture manifest differs from qualified catalog".into());
    }
    let gh = format!(
        "/tools/mise/installs/gh/{0}/gh_{0}_{1}/bin/gh",
        pin.version, pin.platform
    );
    let meta = std::fs::symlink_metadata(&gh)?;
    if !meta.is_file()
        || meta.len() == 0
        || meta.len() > 128 * 1024 * 1024
        || super::sha256(&std::fs::read(gh)?) != pin.sha256
    {
        return Err("unqualified GitHub fixture executable".into());
    }
    let key = std::env::var("VELNOR_GH_FIXTURE_CACHE_KEY")?;
    if key.len() != 64
        || !key
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        || std::fs::read_to_string("/target/fixture-key")? != key
    {
        return Err("invalid isolated GitHub fixture cache identity".into());
    }
    Ok(())
}

pub(super) fn record_success(scenario: Scenario) -> Result<(), Box<dyn Error>> {
    let nonce = std::env::var("VELNOR_GH_FIXTURE_NONCE")?;
    if nonce.len() != 64
        || !nonce
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("invalid isolated GitHub fixture receipt".into());
    }
    std::fs::write(
        format!("/target/receipt-{nonce}"),
        format!("{nonce}\n{}\n", scenario.test_name()),
    )?;
    Ok(())
}
