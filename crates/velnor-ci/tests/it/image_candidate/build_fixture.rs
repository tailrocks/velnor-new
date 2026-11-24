// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_ci::image_candidate::hashing::sha256_hex;
use velnor_ci::image_candidate::record::INPUT_PATHS;

use super::fixture::{LAYER, build, plain_layer, spec};
use crate::support::fresh_dir;

pub(super) const PLUGIN: &[u8] = b"fake buildx plugin\n";

pub(super) struct Run {
    pub(super) ok: bool,
    pub(super) stderr: String,
    pub(super) stdout: String,
}

pub(super) struct Source {
    dir: velnor_test_support::ScratchDir,
}

fn write(path: &Path, data: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap_or(path)).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(path, data).unwrap_or_else(|e| unreachable!("{e}"));
}

fn git(checkout: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(checkout)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn fake_docker(dir: &Path, behavior: &str) -> String {
    let log = dir.join("docker.log").display().to_string();
    let env = dir.join("docker.env").display().to_string();
    let plugin = dir.join("buildx").display().to_string();
    format!(
        r#"#!/bin/sh
printf '%s\n' "$*" >> '{log}'
if [ "$1 $2" = "buildx version" ]; then
  echo "github.com/docker/buildx v0.38.0 fake"
  exit 0
fi
if [ "$1" = "info" ]; then
  echo '[{{"Name":"buildx","Path":"{plugin}","Version":"v0.38.0"}}]'
  exit 0
fi
if [ "$1 $2" = "buildx build" ]; then
  env > '{env}'
  for argument in "$@"; do
    case "$argument" in
      type=oci,dest=*) dest="${{argument#type=oci,dest=}}"; dest="${{dest%%,*}}" ;;
    esac
  done
  {behavior}
fi
exit 2
"#,
    )
}

pub(super) const COPY: &str = r#"[ -f "$DOCKER_CONFIG/cli-plugins/docker-buildx" ] || exit 9
  cp '{archive}' "$dest"
  exit 0"#;

impl Source {
    pub(super) fn new(label: &str) -> Self {
        let dir = fresh_dir("velnor-ci-image-build", label);
        let checkout = dir.join("checkout");
        for name in INPUT_PATHS {
            write(&checkout.join(name), format!("content of {name}\n"));
        }
        write(
            &checkout.join("images/job/ubuntu-26.04/build-test.sh"),
            "#!/usr/bin/env bash\nexit 0\n",
        );
        write(
            &checkout.join(".velnor/runner.lock"),
            format!(
                "buildx_version = \"0.38.0\"\nbuildx_sha256 = \"{}\"\n",
                sha256_hex(PLUGIN)
            ),
        );
        git(&checkout, &["init", "-q"]);
        git(&checkout, &["add", "-A"]);
        git(&checkout, &["commit", "-q", "-m", "fixture"]);
        write(&dir.join("buildx"), PLUGIN);
        write(
            &dir.join("archive.tar"),
            build(&spec(vec![(plain_layer(), LAYER)])).archive(),
        );
        fs::create_dir(dir.join("out")).unwrap_or_else(|e| unreachable!("{e}"));
        let source = Self { dir };
        source.docker(&COPY.replace(
            "{archive}",
            &source.dir.join("archive.tar").display().to_string(),
        ));
        source
    }

    pub(super) fn docker(&self, behavior: &str) {
        let script = self.dir.join("bin/docker");
        write(&script, fake_docker(&self.dir, behavior));
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }

    pub(super) fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    pub(super) fn script(&self, body: &str) {
        let checkout = self.path("checkout");
        write(
            &checkout.join("images/job/ubuntu-26.04/build-test.sh"),
            format!("#!/usr/bin/env bash\n{body}\n"),
        );
        self.commit_all("script");
    }

    pub(super) fn commit_all(&self, message: &str) {
        let checkout = self.path("checkout");
        git(&checkout, &["add", "-A"]);
        git(&checkout, &["commit", "-q", "-m", message]);
    }

    pub(super) fn head(&self) -> String {
        git(&self.path("checkout"), &["rev-parse", "HEAD"])
    }

    pub(super) fn text(&self, name: &str) -> String {
        fs::read_to_string(self.path(name)).unwrap_or_default()
    }

    pub(super) fn run(&self, flags: &[(&str, PathBuf)], env: &[(&str, Option<&str>)]) -> Run {
        self.invoke("build", flags, env)
    }

    pub(super) fn verify(&self) -> Run {
        self.invoke("verify-candidate", &[], &[])
    }

    fn invoke(
        &self,
        subcommand: &str,
        flags: &[(&str, PathBuf)],
        env: &[(&str, Option<&str>)],
    ) -> Run {
        let head = self.head();
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
        let path = format!(
            "{}:{}",
            self.path("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut arguments: Vec<(&str, PathBuf)> = vec![
            ("--root", self.path("checkout")),
            ("--archive", self.path("out/candidate.oci.tar")),
            ("--record", self.path("out/candidate.json")),
        ];
        if subcommand == "build" {
            arguments.push(("--source-commit", PathBuf::from(&head)));
        }
        for (name, value) in flags {
            arguments.retain(|(known, _)| known != name);
            arguments.push((name, value.clone()));
        }
        command.env_clear().args(["image", subcommand]);
        for (name, value) in arguments {
            command.arg(name).arg(value);
        }
        let mut vars = vec![
            ("PATH", path),
            ("GITHUB_SHA", head),
            ("GITHUB_REF", "refs/pull/7/merge".to_owned()),
            ("GITHUB_REPOSITORY", "tailrocks/velnor-new".to_owned()),
            ("GITHUB_REPOSITORY_ID", "1390620900".to_owned()),
            ("GITHUB_EVENT_NAME", "pull_request".to_owned()),
            (
                "GITHUB_WORKFLOW_REF",
                "tailrocks/velnor-new/.github/workflows/release.yml@refs/pull/7/merge".to_owned(),
            ),
            ("GITHUB_WORKFLOW_SHA", "c".repeat(40)),
            ("GITHUB_RUN_ID", "10".to_owned()),
            ("GITHUB_RUN_ATTEMPT", "2".to_owned()),
            ("GH_TOKEN", "secret-token".to_owned()),
            ("DOCKER_HOST", "tcp://elsewhere:2375".to_owned()),
            ("REGISTRY_PASSWORD", "secret-password".to_owned()),
        ];
        for (name, value) in env {
            vars.retain(|(known, _)| known != name);
            if let Some(value) = value {
                vars.push((name, (*value).to_owned()));
            }
        }
        command.envs(vars);
        let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
        Run {
            ok: output.status.success(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        }
    }
}
