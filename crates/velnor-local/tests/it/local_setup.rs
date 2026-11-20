// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_local::io::local_setup::variant_action;

use crate::support::{Stub, temp_dir};

const PINNED: &str = "/repos/tailrocks/velnor-new/tarball/1793cf84b5924e2542a03b0fb2580e7395b2f4cb";
const UPSTREAM: &str = "https://api.github.com/repos/tailrocks/velnor-new/tarball/1793cf84b5924e2542a03b0fb2580e7395b2f4cb";
const MOUNTED: &str = "/opt/velnor/local/velnor-ci";
const LOCAL_MARKERS: &[&str] = &[
    "/opt/velnor/local",
    "VELNOR_LOCAL",
    "local-mode",
    "mountinfo",
];

fn production_action() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../actions/setup-velnor/action.yml");
    std::fs::read_to_string(&path).unwrap_or_else(|e| unreachable!("{}: {e}", path.display()))
}

fn stub(name: &str, local: bool) -> Stub {
    let repo = temp_dir(&format!("{name}-repo"));
    let state = temp_dir(&format!("{name}-state"));
    let cache = temp_dir(&format!("{name}-cache"));
    if local {
        Stub::local_setup(&repo, &state, &cache)
    } else {
        Stub::new(&repo, &state, &cache)
    }
}

fn unpacked(name: &str, tarball: &[u8]) -> PathBuf {
    let dir = temp_dir(name);
    let file = dir.join("action.tar.gz");
    std::fs::write(&file, tarball).unwrap_or_else(|e| unreachable!("{e}"));
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap_or_else(|e| unreachable!("{e}"));
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(&file)
        .arg("-C")
        .arg(&out)
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(status.success(), "tar rejected the tarball");
    out
}

fn single_top(out: &Path) -> PathBuf {
    let tops: Vec<_> = std::fs::read_dir(out)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .map(|entry| entry.unwrap_or_else(|e| unreachable!("{e}")).path())
        .collect();
    assert_eq!(
        tops.len(),
        1,
        "the runner needs exactly one top directory: {tops:?}"
    );
    tops.into_iter().next().unwrap_or_default()
}

fn input_names(action: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut inside = false;
    for line in action.lines() {
        if line == "inputs:" {
            inside = true;
        } else if inside && !line.starts_with(' ') {
            break;
        } else if inside && line.starts_with("  ") && !line.starts_with("   ") {
            names.push(line.trim().trim_end_matches(':').to_owned());
        }
    }
    names
}

#[test]
fn the_production_action_has_no_local_binary_path() {
    let action = production_action();
    for marker in LOCAL_MARKERS.iter().copied() {
        assert!(
            !action.contains(marker),
            "production action.yml mentions {marker}"
        );
    }
}

#[tokio::test]
async fn in_local_mode_the_stub_serves_the_local_variant_of_the_pinned_action() {
    let stub = stub("local-setup-on", true);
    let reply = stub.get(PINNED).await;
    assert_eq!(reply.status, 200);
    let out = unpacked("local-setup-on-unpacked", &reply.body);
    let top = single_top(&out);
    let served = std::fs::read_to_string(top.join("actions/setup-velnor/action.yml"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(served, variant_action());
    assert!(served.contains(MOUNTED));
    assert_ne!(served, production_action());
}

#[tokio::test]
async fn without_local_mode_the_stub_sends_the_pinned_action_to_github_unchanged() {
    let stub = stub("local-setup-off", false);
    let reply = stub.get(PINNED).await;
    assert_eq!(reply.status, 302);
    let location = reply
        .headers
        .get("location")
        .and_then(|value| value.to_str().ok());
    assert_eq!(location, Some(UPSTREAM));
    assert!(reply.body.is_empty());
}

#[tokio::test]
async fn in_local_mode_every_other_action_still_comes_from_github() {
    let stub = stub("local-setup-other", true);
    for (path, upstream) in [
        (
            "/repos/actions/checkout/tarball/v4",
            "https://api.github.com/repos/actions/checkout/tarball/v4",
        ),
        (
            "/repos/tailrocks/other/tarball/refs/tags/v1",
            "https://api.github.com/repos/tailrocks/other/tarball/refs/tags/v1",
        ),
    ] {
        let reply = stub.get(path).await;
        assert_eq!(reply.status, 302, "{path}");
        let location = reply
            .headers
            .get("location")
            .and_then(|value| value.to_str().ok());
        assert_eq!(location, Some(upstream));
    }
}

#[test]
fn the_variant_declares_the_inputs_of_the_production_action() {
    let production = input_names(&production_action());
    assert_eq!(production.len(), 4, "{production:?}");
    assert_eq!(input_names(variant_action()), production);
}

struct Job {
    dir: PathBuf,
    mounted: PathBuf,
    script: String,
}

impl Job {
    fn new(name: &str) -> Self {
        let dir = temp_dir(name);
        let mounted = dir.join("mounted-velnor-ci");
        let action = variant_action();
        let (_, body) = action
            .split_once("      run: |\n")
            .unwrap_or_else(|| unreachable!("no run block"));
        let script = body
            .lines()
            .map(|line| line.strip_prefix("        ").unwrap_or(line))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            script.matches(MOUNTED).count(),
            1,
            "the mount path is a literal, once"
        );
        let script = script.replace(MOUNTED, &mounted.display().to_string());
        Self {
            dir,
            mounted,
            script,
        }
    }

    fn mount(&self, bytes: &[u8], mode: u32) {
        std::fs::write(&self.mounted, bytes).unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::set_permissions(&self.mounted, std::fs::Permissions::from_mode(mode))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }

    fn run(&self, sha: &str, version: &str, arch: &str) -> std::process::Output {
        let shims = self.dir.join("shims");
        std::fs::create_dir_all(&shims).unwrap_or_else(|e| unreachable!("{e}"));
        let curl = shims.join("curl");
        std::fs::write(
            &curl,
            format!(
                "#!/bin/sh\necho called > {}/curl-called\nexit 1\n",
                self.dir.display()
            ),
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::set_permissions(&curl, std::fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let temp = self.dir.join("temp");
        std::fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
        let path = format!(
            "{}:{}",
            shims.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new("bash")
            .arg("-c")
            .arg(&self.script)
            .env("PATH", path)
            .env("RUNNER_OS", "Linux")
            .env("RUNNER_ARCH", arch)
            .env("RUNNER_TEMP", &temp)
            .env("RUNNER_TOOL_CACHE", self.dir.join("tool-cache"))
            .env("GITHUB_PATH", self.dir.join("github-path"))
            .env("GITHUB_STEP_SUMMARY", self.dir.join("summary"))
            .env("VERSION", version)
            .env("SHA256_LINUX_X64", "")
            .env("SHA256_LINUX_ARM64", sha)
            .env("SHA256_MACOS_ARM64", "")
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"))
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.join(name)).unwrap_or_default()
    }
}

const PIN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const LOCAL_BYTES: &[u8] = b"#!/bin/sh\necho local velnor-ci\n";

fn sha256(bytes: &[u8]) -> String {
    let dir = temp_dir("local-setup-sha");
    let file = dir.join("bytes");
    std::fs::write(&file, bytes).unwrap_or_else(|e| unreachable!("{e}"));
    let out = Command::new("shasum")
        .args(["-a", "256"])
        .arg(&file)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn the_variant_installs_the_mounted_binary_loudly_with_no_download_and_no_tool_cache() {
    let job = Job::new("local-setup-install");
    job.mount(LOCAL_BYTES, 0o755);
    let out = job.run(PIN, "0.1.81", "ARM64");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let installed = job.dir.join("temp/velnor-ci-bin/velnor-ci");
    assert_eq!(std::fs::read(&installed).unwrap_or_default(), LOCAL_BYTES);
    let mode = std::fs::metadata(&installed)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .permissions()
        .mode();
    assert_ne!(mode & 0o111, 0);
    assert_eq!(
        job.read("github-path").trim(),
        job.dir.join("temp/velnor-ci-bin").display().to_string()
    );
    let local = sha256(LOCAL_BYTES);
    assert!(stdout.contains("::warning"), "{stdout}");
    assert!(stdout.contains(&local), "{stdout}");
    assert!(stdout.contains(PIN), "{stdout}");
    assert!(job.read("summary").contains(&local));
    assert!(
        !job.dir.join("curl-called").exists(),
        "the variant downloaded"
    );
    assert!(
        !job.dir.join("tool-cache").exists(),
        "the variant wrote the tool cache"
    );
}

#[test]
fn the_variant_never_falls_back_to_a_download_when_nothing_is_mounted() {
    let job = Job::new("local-setup-missing");
    let out = job.run(PIN, "0.1.81", "ARM64");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("::error"));
    assert!(!job.dir.join("curl-called").exists());
    assert!(!job.dir.join("temp/velnor-ci-bin").exists());
    job.mount(LOCAL_BYTES, 0o644);
    assert!(
        !job.run(PIN, "0.1.81", "ARM64").status.success(),
        "not executable"
    );
}

#[test]
fn the_variant_still_requires_the_workflows_version_and_sha256_pins() {
    let job = Job::new("local-setup-inputs");
    job.mount(LOCAL_BYTES, 0o755);
    for (sha, version, arch) in [
        ("", "0.1.81", "ARM64"),
        ("ABC", "0.1.81", "ARM64"),
        (PIN, "0.1", "ARM64"),
        (PIN, "latest", "ARM64"),
        (PIN, "0.1.81", "X64"),
    ] {
        let out = job.run(sha, version, arch);
        assert!(!out.status.success(), "{sha} {version} {arch}");
        assert!(String::from_utf8_lossy(&out.stdout).contains("::error"));
    }
    assert!(!job.dir.join("temp/velnor-ci-bin").exists());
}
