// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::io;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use velnor_local::authority::{WARNING, WARNING_POLICY, warning};
use velnor_local::io::authority::Backend;
use velnor_local::io::pair::{MintError, Minted};
use velnor_local::io::supervisor::{self, Pairs};
use velnor_local::pairs::{PlanInput, plan};
use velnor_local::side::Side;

use crate::support::temp_dir;

struct Online;

impl Backend for Online {
    fn get(&self, _: Side, path: &str) -> impl Future<Output = io::Result<Option<Vec<u8>>>> + Send {
        let body = if path.contains("isagentonline") {
            r#"{"online":true}"#
        } else {
            r#"{"value":[{"name":"trusted-1"}],"count":1}"#
        };
        std::future::ready(Ok(Some(body.as_bytes().to_vec())))
    }

    fn delete(&self, _: Side, _: &str) -> impl Future<Output = io::Result<()>> + Send {
        std::future::ready(Ok(()))
    }

    fn mint(
        &self,
        _: Side,
        _: Option<&str>,
    ) -> impl Future<Output = Result<Option<Minted>, MintError>> + Send {
        std::future::ready(Ok(None))
    }
}

fn fake_host(dir: &Path) -> std::path::PathBuf {
    let script = dir.join("fake-authority");
    std::fs::write(
        &script,
        "#!/bin/bash\nmkdir -p \"$3/runner\"\necho 127.0.0.1:9 > \"$3/authority/address\"\necho 'pair trusted-1 listening' > \"$3/runner/velnor-host.log\"\nexec -a \"$0 $1 $2 $3 $4 $5\" /bin/sleep 600\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    script
}

async fn start(dir: &Path, policy: Option<&Path>) -> io::Result<()> {
    let script = fake_host(dir);
    let pairs = Pairs {
        host: script.clone(),
        trusted: 1,
        untrusted: 0,
        policy: policy.map(Path::to_path_buf),
    };
    let started = supervisor::start_pairs(
        dir,
        &script,
        &pairs,
        async |_: &Path| Ok(Online),
        Duration::from_secs(10),
    )
    .await
    .map(|_| ());
    let pid = std::fs::read_to_string(dir.join("authority/pid")).unwrap_or_default();
    let _ = Command::new("kill").args(["-KILL", pid.trim()]).status();
    started
}

#[tokio::test]
async fn up_copies_the_policy_into_the_runner_state_for_the_host_to_read() {
    let dir = temp_dir("policy-copy");
    let file = dir.join("approved.json");
    std::fs::write(&file, "{\"schema_version\":1}").unwrap_or_else(|e| unreachable!("{e}"));
    start(&dir, Some(&file))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let copy = dir.join("runner/policy.json");
    assert_eq!(
        std::fs::read_to_string(&copy).unwrap_or_default(),
        "{\"schema_version\":1}"
    );
    let mode = std::fs::metadata(&copy).map(|meta| meta.permissions().mode() & 0o777);
    assert_eq!(mode.unwrap_or_default(), 0o600);
}

#[tokio::test]
async fn a_start_without_a_policy_removes_the_policy_of_an_earlier_start() {
    let dir = temp_dir("policy-stale");
    std::fs::create_dir_all(dir.join("runner")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(dir.join("runner/policy.json"), "{}").unwrap_or_else(|e| unreachable!("{e}"));
    start(&dir, None)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!dir.join("runner/policy.json").exists());
}

#[tokio::test]
async fn a_policy_that_is_empty_too_large_a_link_or_missing_stops_the_start() {
    let dir = temp_dir("policy-refused");
    let empty = dir.join("empty.json");
    let large = dir.join("large.json");
    let target = dir.join("target.json");
    let link = dir.join("link.json");
    std::fs::write(&empty, "").unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(&large, vec![b' '; 256 * 1024 + 1]).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(&target, "{}").unwrap_or_else(|e| unreachable!("{e}"));
    symlink(&target, &link).unwrap_or_else(|e| unreachable!("{e}"));
    for file in [empty, large, link, dir.join("missing.json")] {
        assert!(
            start(&dir, Some(&file)).await.is_err(),
            "{}",
            file.display()
        );
        assert!(!dir.join("runner/policy.json").exists());
    }
}

#[tokio::test]
async fn restarting_with_the_copied_policy_as_the_source_keeps_it() {
    let dir = temp_dir("policy-restart");
    let copied = dir.join("runner/policy.json");
    std::fs::create_dir_all(dir.join("runner")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(&copied, "{\"schema_version\":1}").unwrap_or_else(|e| unreachable!("{e}"));
    start(&dir, Some(&copied))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        std::fs::read_to_string(&copied).unwrap_or_default(),
        "{\"schema_version\":1}"
    );
}

#[test]
fn the_plan_names_the_policy_file_only_when_one_is_given() {
    let sides = [(Side::Trusted, "n".to_owned(), 1)];
    let mut input = PlanInput {
        environment: "velnor-local-ab",
        authority: "127.0.0.1:5311",
        token: "t0k3n",
        docker: "unix:///var/run/docker.sock",
        runner_image: "sha256:aa",
        dind_image: "sha256:bb",
        sides: &sides,
        policy: None,
    };
    assert!(plan(&input)["policy"].is_null());
    input.policy = Some("policy.json");
    assert_eq!(plan(&input)["policy"], "policy.json");
}

#[test]
fn the_warning_says_whether_the_check_is_on() {
    assert_eq!(warning(false), WARNING);
    assert_eq!(warning(true), WARNING_POLICY);
    assert!(WARNING_POLICY.contains("check is on"));
    assert!(WARNING.contains("check is off"));
}

#[test]
fn plain_containers_refuse_policy_because_they_have_no_job_start_check() {
    let dir = temp_dir("policy-plain");
    let file = dir.join("approved.json");
    std::fs::write(&file, "{}").unwrap_or_else(|e| unreachable!("{e}"));
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-local"))
        .args(["up", "--runner", "plain", "--dir"])
        .arg(dir.join("state"))
        .arg("--repo")
        .arg(&dir)
        .arg("--policy")
        .arg(&file)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(text.contains("--policy needs --runner pair"), "{text}");
}

#[tokio::test]
async fn a_symlinked_policy_file_is_refused_and_nothing_is_copied() {
    let dir = temp_dir("policy-symlink");
    let target = dir.join("real.json");
    let link = dir.join("link.json");
    std::fs::write(&target, "{\"schema_version\":1}").unwrap_or_else(|e| unreachable!("{e}"));
    symlink(&target, &link).unwrap_or_else(|e| unreachable!("{e}"));
    let refused = start(&dir, Some(&link)).await;
    assert!(
        refused.is_err_and(|error| error.to_string().contains("regular file")),
        "a link was accepted"
    );
    assert!(!dir.join("runner/policy.json").exists());
}
