// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;
use std::future::Future;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::Value;
use velnor_local::images::{DIND_TAG, Lock, PROBE_TAG, RUNNER_TAG, plan, refuse_dirty};
use velnor_local::io::env::{Engine, Environment};
use velnor_local::io::pair::{mint, write_private};
use velnor_local::jit::pack;
use velnor_local::side::Side;

use crate::support::temp_dir;

const REAL_LOCK: &str = include_str!("../../../../../.velnor/runner.lock");

#[derive(Debug)]
struct Registration {
    output: String,
    calls: Mutex<Vec<Vec<String>>>,
}

impl Registration {
    fn new(output: &str) -> Self {
        Self {
            output: output.to_owned(),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls
            .lock()
            .unwrap_or_else(|e| unreachable!("{e}"))
            .clone()
    }
}

impl Engine for &Registration {
    fn docker(&self, args: &[&str]) -> impl Future<Output = io::Result<String>> + Send {
        self.calls
            .lock()
            .unwrap_or_else(|e| unreachable!("{e}"))
            .push(args.iter().map(|arg| (*arg).to_owned()).collect());
        std::future::ready(Ok(self.output.clone()))
    }

    fn logged(&self, _: &[&str], _: &Path) -> impl Future<Output = io::Result<bool>> + Send {
        std::future::ready(Ok(true))
    }
}

fn output(files: &[(&str, &str)]) -> String {
    files
        .iter()
        .fold(String::new(), |mut text, (name, content)| {
            let _ = writeln!(text, "{name} {}", STANDARD.encode(content));
            text
        })
}

const ALL: [(&str, &str); 3] = [
    (".runner", r#"{"agentName":"x"}"#),
    (".credentials", r#"{"scheme":"OAuth"}"#),
    (".credentials_rsaparams", r#"{"d":"AQAB"}"#),
];

fn steps() -> Vec<Vec<String>> {
    plan(
        &Lock::parse(REAL_LOCK).unwrap_or_else(|e| unreachable!("{e}")),
        Path::new("/repo"),
        Path::new("/stages"),
        "abc123",
    )
    .unwrap_or_else(|e| unreachable!("{e}"))
}

fn lock_value(key: &str) -> String {
    Lock::parse(REAL_LOCK)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .get(key)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .to_owned()
}

fn tagged<'a>(steps: &'a [Vec<String>], tag: &str) -> &'a [String] {
    steps
        .iter()
        .find(|step| step.windows(2).any(|pair| pair == ["--tag", tag]))
        .unwrap_or_else(|| unreachable!("no build of {tag}"))
}

#[test]
fn the_image_plan_builds_three_arm64_images_from_the_arm64_pins_of_the_lock() {
    let steps = steps();
    let runner = tagged(&steps, RUNNER_TAG).join(" ");
    for (name, key) in [
        ("RUNNER_ARM64_SHA256", "runner_arm64_sha256"),
        ("DOCKER_ARM64_SHA256", "docker_arm64_sha256"),
        ("COMPOSE_ARM64_SHA256", "compose_arm64_sha256"),
        ("BUILDX_ARM64_SHA256", "buildx_arm64_sha256"),
        ("NODE_ARM64_SHA256", "node_arm64_sha256"),
    ] {
        assert!(runner.contains(&format!("{name}={}", lock_value(key))));
    }
    let base = format!(
        "docker-image://{}@{}",
        lock_value("base_image_name"),
        lock_value("base_image_arm64_digest")
    );
    for tag in [RUNNER_TAG, DIND_TAG] {
        let step = tagged(&steps, tag).join(" ");
        assert!(step.contains(&base), "{tag} does not use the arm64 base");
        assert!(step.contains("--platform linux/arm64"));
    }
    let probe = tagged(&steps, PROBE_TAG).join(" ");
    assert!(probe.contains("SOURCE_SHA=abc123"));
    assert!(probe.contains("resource_probe=/stages/resource-probe"));
    assert!(probe.ends_with("--tag velnor-local/pair-probe --load /repo/images/resource-probe"));
}

#[test]
fn the_helpers_build_is_locked_and_reads_the_checkout_read_only() {
    let steps = steps();
    let helpers = steps[0].join(" ");
    assert!(helpers.contains("-v /repo:/src:ro"));
    assert!(helpers.contains("cargo build --locked"));
    assert!(helpers.contains("--platform linux/arm64"));
}

#[test]
fn the_image_plan_names_the_lock_entry_that_is_missing() {
    let lock = Lock::parse("base_image = \"ubuntu:26.04@sha256:00\"\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    let refused = plan(&lock, Path::new("/r"), Path::new("/s"), "abc");
    assert!(
        refused
            .err()
            .is_some_and(|error| error.contains("base_image_name"))
    );
}

#[tokio::test]
async fn a_minted_jit_packs_the_three_files_registration_wrote() {
    let dir = temp_dir("pair-mint");
    let docker = Registration::new(&output(&ALL));
    let env = Environment::at(&dir, &docker)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let minted = mint(&env, Side::Untrusted)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let files: Value = serde_json::from_slice(
        &STANDARD
            .decode(&minted.jit)
            .unwrap_or_else(|e| unreachable!("{e}")),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    for (name, content) in ALL {
        let packed = files[name].as_str().unwrap_or_default();
        assert_eq!(
            STANDARD.decode(packed).unwrap_or_default(),
            content.as_bytes()
        );
    }
    let call = docker.calls().concat().join(" ");
    assert!(call.contains(&format!("--network {}-fork", env.name)));
    assert!(call.contains(&format!("--label velnor-local={}", env.name)));
    assert!(call.contains("--ephemeral"));
    assert!(call.contains("--platform linux/arm64"));
    assert!(call.contains("-e GITHUB_ACTIONS_RUNNER_FORCE_GHES=1"));
    assert!(call.contains(&format!("--name {}", minted.name)));
    assert!(minted.name.starts_with(&format!("{}-jit-", env.name)));
}

#[test]
fn registration_output_missing_a_file_is_refused() {
    let partial = output(&ALL[..2]);
    let refused = pack(&partial);
    assert!(
        refused
            .err()
            .is_some_and(|e| e.contains(".credentials_rsaparams"))
    );
    assert!(pack("garbage").is_err());
}

#[tokio::test]
async fn a_jit_file_is_private_and_replaces_the_one_before() {
    let dir = temp_dir("pair-private");
    let path = dir.join("jit");
    write_private(&path, "first")
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    write_private(&path, "second")
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(std::fs::read_to_string(&path).unwrap_or_default(), "second");
    let mode = std::fs::metadata(&path)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[tokio::test]
async fn a_jit_file_holds_its_whole_content_the_moment_the_write_returns() {
    let dir = temp_dir("pair-whole");
    let payload = "jit-config-payload-0123456789".repeat(8);
    for n in 0..300 {
        let path = dir.join(format!("jit-{n}"));
        write_private(&path, &payload)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
        let read = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(read, payload, "write {n} was visible before it finished");
    }
}

#[tokio::test]
async fn concurrent_jit_writes_to_one_path_all_succeed() {
    let dir = temp_dir("pair-concurrent");
    let path = dir.join("jit");
    let writes = (0..16).map(|n| {
        let path = path.clone();
        tokio::spawn(async move { write_private(&path, &format!("jit-{n}")).await })
    });
    for write in writes.collect::<Vec<_>>() {
        write
            .await
            .unwrap_or_else(|e| unreachable!("{e}"))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    assert!(
        std::fs::read_to_string(&path)
            .unwrap_or_default()
            .starts_with("jit-")
    );
    let names = std::fs::read_dir(&dir).map_or(0, Iterator::count);
    assert_eq!(names, 1);
}

#[test]
fn the_helpers_compile_the_source_revision_and_keep_their_download_caches() {
    let helpers = steps()[0].join(" ");
    let image = helpers.find(" rust:").unwrap_or(0);
    let revision = helpers.find("-e VELNOR_SOURCE_SHA=abc123");
    assert!(revision.is_some_and(|at| at < image), "{helpers}");
    assert!(helpers.contains("-v velnor-arm64-cargo:/cargo-home"));
    assert!(helpers.contains("-e CARGO_HOME=/cargo-home"));
    assert!(helpers.contains(":/usr/local/rustup"));
}

#[test]
fn the_rustup_volume_is_named_for_the_digest_of_the_rust_image_it_holds() {
    let helpers = steps()[0].clone();
    let image = helpers
        .iter()
        .find(|arg| arg.starts_with("rust:"))
        .unwrap_or_else(|| unreachable!("{helpers:?}"));
    let digest = image
        .rsplit_once("sha256:")
        .map_or("", |(_, digest)| digest);
    assert_eq!(digest.len(), 64, "{image}");
    let volume = format!("velnor-arm64-rustup-{}:/usr/local/rustup", &digest[..12]);
    assert!(helpers.contains(&volume), "{helpers:?}");
}

#[test]
fn a_dirty_source_tree_is_refused_with_the_files_named() {
    assert!(refuse_dirty("").is_ok());
    let refused = refuse_dirty(" M crates/velnor-runner/a.rs\n?? images/dind/new\n");
    let message = refused.err().unwrap_or_default();
    assert!(message.contains("crates/velnor-runner/a.rs"), "{message}");
    assert!(message.contains("images/dind/new"), "{message}");
}

#[tokio::test]
async fn build_refuses_a_checkout_whose_images_would_not_match_head() {
    let root = temp_dir("pair-dirty-build");
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(["-C", &root.display().to_string()])
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .status()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(status.success(), "{args:?}");
    };
    git(&["init", "-q"]);
    std::fs::create_dir_all(root.join(".velnor")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::create_dir_all(root.join("crates/velnor-runner"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(root.join(".velnor/runner.lock"), REAL_LOCK)
        .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(root.join("crates/velnor-runner/lib.rs"), "a")
        .unwrap_or_else(|e| unreachable!("{e}"));
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "x"]);
    std::fs::write(root.join("crates/velnor-runner/lib.rs"), "b")
        .unwrap_or_else(|e| unreachable!("{e}"));
    let refused =
        velnor_local::io::pair::build(velnor_local::cli::BuildArgs { root: root.clone() }).await;
    let message = refused.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(message.contains("crates/velnor-runner/lib.rs"), "{message}");
}

#[tokio::test]
async fn a_registration_that_cannot_be_packed_reports_the_runner_name_it_registered() {
    let dir = temp_dir("pair-mint-broken");
    let docker = Registration::new("garbage");
    let env = Environment::at(&dir, &docker)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let failure = mint(&env, Side::Trusted).await.err();
    let call = docker.calls().concat().join(" ");
    let name = failure.map(|failure| failure.name).unwrap_or_default();
    assert!(name.starts_with(&format!("{}-jit-", env.name)), "{name}");
    assert!(call.contains(&format!("--name {name}")), "{call}");
}

#[tokio::test]
async fn a_minted_runner_registers_the_extra_labels_after_the_built_in_ones() {
    let built_in = velnor_local::jit::labels(&[]);
    let dir = temp_dir("pair-mint-labels");
    let docker = Registration::new(&output(&ALL));
    let env = Environment::at(&dir, &docker)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"))
        .with_labels(&["ubuntu-26.04-scale-set".to_owned()]);
    mint(&env, Side::Trusted)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let call = docker.calls().concat().join(" ");
    assert!(
        call.contains(&format!("--labels {built_in},ubuntu-26.04-scale-set >&2")),
        "{call}"
    );
}

#[tokio::test]
async fn a_minted_runner_without_extra_labels_registers_the_built_in_ones_only() {
    let built_in = velnor_local::jit::labels(&[]);
    let dir = temp_dir("pair-mint-no-labels");
    let docker = Registration::new(&output(&ALL));
    let env = Environment::at(&dir, &docker)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    mint(&env, Side::Trusted)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let call = docker.calls().concat().join(" ");
    assert!(call.contains(&format!("--labels {built_in} >&2")), "{call}");
    assert!(built_in.starts_with("ubuntu-latest,"), "{built_in}");
    assert!(built_in.ends_with(",ubuntu-26.04-arm"), "{built_in}");
}
