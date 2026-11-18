// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use clap::Parser;
use velnor_local::cli::{Cli, Command, EnvironmentArgs};
use velnor_local::io::env::Environment;

use crate::fake_docker::{FakeDocker, after};
use crate::support::temp_dir;

const LOCAL_DIR: &str = "/opt/velnor/local";
pub(crate) const ELF_ARM64: [u8; 20] = [
    0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0xb7, 0,
];

pub(crate) fn binary(dir: &Path, bytes: &[u8]) -> PathBuf {
    let file = dir.join("velnor-ci");
    std::fs::write(&file, bytes).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    file
}

pub(crate) fn parsed(dir: &Path, mode: &str, extra: &[&str]) -> EnvironmentArgs {
    let dir = dir.display().to_string();
    let mut argv = vec![
        "velnor-local",
        "up",
        "--dir",
        &dir,
        "--repo",
        &dir,
        "--runner",
        mode,
        "--runners",
        "1",
        "--fork-runners",
        "1",
    ];
    argv.extend_from_slice(extra);
    match Cli::try_parse_from(argv) {
        Ok(Cli {
            command: Command::Up(up),
        }) => up.environment,
        other => unreachable!("{other:?}"),
    }
}

async fn started(dir: &Path, args: &EnvironmentArgs) -> (Vec<Vec<String>>, std::io::Result<()>) {
    let docker = FakeDocker::default();
    let env = Environment::at(dir, &docker)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let result = env.up(dir, args).await.map(|_| ());
    (docker.started(), result)
}

const TARBALLS: &str =
    "Runner.Server__ActionDownloadUrls__0__TarballUrl=http://api-stub/repos/{0}/tarball/{1}";

#[tokio::test]
async fn a_local_velnor_ci_is_mounted_read_only_into_trusted_plain_runners_only() {
    let dir = temp_dir("local-ci-mounts");
    let file = binary(&dir, &ELF_ARM64);
    let flag = file.display().to_string();
    let args = parsed(&dir, "plain", &["--velnor-ci-binary", &flag]);
    let (calls, _) = started(&dir, &args).await;
    let canonical = std::fs::canonicalize(&file).unwrap_or_else(|e| unreachable!("{e}"));
    let wanted = format!("{}:{LOCAL_DIR}/velnor-ci:ro", canonical.display());
    let mut local_runners = 0;
    for call in &calls {
        let name = after(call, "--name")[0];
        let mounts = after(call, "-v");
        if name.ends_with("-runner-1") && !name.contains("-fork-") {
            local_runners += 1;
            assert_eq!(
                mounts
                    .iter()
                    .filter(|m| m.contains(LOCAL_DIR))
                    .collect::<Vec<_>>(),
                [&wanted.as_str()],
                "{name}"
            );
        } else {
            assert!(
                mounts.iter().all(|m| !m.contains(LOCAL_DIR)),
                "{name} {mounts:?}"
            );
        }
    }
    assert_eq!(local_runners, 1);
    assert!(
        !dir.join("local-mode").exists(),
        "a marker a job could forge is no longer written"
    );
    for call in &calls {
        assert!(
            call.iter().all(|arg| !arg.contains("VELNOR_LOCAL")),
            "no job-visible mode variable: {call:?}"
        );
    }
}

#[tokio::test]
async fn local_mode_points_only_the_trusted_standin_and_the_api_stub_at_the_local_action() {
    let dir = temp_dir("local-ci-standin");
    let file = binary(&dir, &ELF_ARM64);
    let flag = file.display().to_string();
    let args = parsed(&dir, "plain", &["--velnor-ci-binary", &flag]);
    let (calls, _) = started(&dir, &args).await;
    let mut standins = 0;
    let mut stubs = 0;
    for call in &calls {
        let name = after(call, "--name")[0];
        let tarballs = after(call, "-e").contains(&TARBALLS);
        if name.ends_with("-standin") && !name.contains("-fork-") {
            standins += 1;
            assert!(tarballs, "{call:?}");
        } else {
            assert!(!tarballs, "{name}");
        }
        let serves = call.iter().any(|arg| arg == "--local-setup-velnor");
        if name.ends_with("-api") {
            stubs += 1;
            assert!(serves, "{call:?}");
        } else {
            assert!(!serves, "{name}");
        }
    }
    assert_eq!((standins, stubs), (1, 1));
}

#[tokio::test]
async fn without_the_flag_no_container_gets_the_local_binary_or_serves_the_local_action() {
    let dir = temp_dir("local-ci-absent");
    let args = parsed(&dir, "plain", &[]);
    let (calls, _) = started(&dir, &args).await;
    assert!(calls.len() > 3, "{calls:?}");
    for call in &calls {
        assert!(
            call.iter().all(|arg| !arg.contains(LOCAL_DIR)
                && !arg.contains("VELNOR_LOCAL")
                && !arg.contains("ActionDownloadUrls")
                && arg != "--local-setup-velnor"),
            "{call:?}"
        );
    }
}

#[tokio::test]
async fn a_binary_that_cannot_run_in_the_runner_or_cannot_be_mounted_starts_nothing() {
    let dir = temp_dir("local-ci-refused");
    let macho = binary(
        &dir,
        &[
            0xcf, 0xfa, 0xed, 0xfe, 12, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    );
    let mut x86 = ELF_ARM64;
    x86[18] = 0x3e;
    let wrong_arch = dir.join("x86");
    std::fs::write(&wrong_arch, x86).unwrap_or_else(|e| unreachable!("{e}"));
    let colon = dir.join("a:b");
    std::fs::write(&colon, ELF_ARM64).unwrap_or_else(|e| unreachable!("{e}"));
    let oversize = dir.join("big");
    std::fs::File::create(&oversize)
        .and_then(|f| f.set_len((256 << 20) + 1))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let cases = [
        macho,
        wrong_arch,
        colon,
        oversize,
        dir.clone(),
        dir.join("missing"),
    ];
    for case in cases {
        let flag = case.display().to_string();
        let args = parsed(&dir, "plain", &["--velnor-ci-binary", &flag]);
        let (calls, result) = started(&dir, &args).await;
        let text = result.map_or_else(|e| e.to_string(), |()| String::new());
        assert!(text.contains("--velnor-ci-binary"), "{flag}: {text}");
        assert!(calls.is_empty(), "{flag}: {calls:?}");
    }
}

#[tokio::test]
async fn pair_mode_refuses_the_flag_because_velnor_host_owns_its_runner_containers() {
    let dir = temp_dir("local-ci-pair");
    let file = binary(&dir, &ELF_ARM64);
    let flag = file.display().to_string();
    let args = parsed(&dir, "pair", &["--velnor-ci-binary", &flag]);
    let docker = FakeDocker::default();
    let env = Environment::at(&dir, &docker)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let error = env.up(&dir, &args).await.err().map(|e| e.to_string());
    let text = error.unwrap_or_default();
    assert!(
        text.contains("--velnor-ci-binary needs --runner plain"),
        "{text}"
    );
    assert!(
        docker
            .calls
            .lock()
            .unwrap_or_else(|e| unreachable!("{e}"))
            .is_empty()
    );
}
