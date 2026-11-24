// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use velnor_ci::image_candidate::hashing::sha256_hex;
use velnor_ci::image_candidate::record::INPUT_PATHS;

use super::fixture::{LAYER, build, plain_layer, spec};
use crate::support::fresh_dir;

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

const DOCKER: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$FAKE_DIR/docker.log"
plugin="$DOCKER_CONFIG/cli-plugins/docker-buildx"
if [ "$1" = "info" ]; then
  if [ -x "$plugin" ]; then
    "$plugin" docker-cli-plugin-metadata > /dev/null
    echo "[{\"Name\":\"buildx\",\"Path\":\"$plugin\",\"Version\":\"v0.38.0\"}]"
  else
    echo null
  fi
  exit 0
fi
if [ "$1" = "buildx" ] && [ -x "$plugin" ]; then
  exec "$plugin" "$@"
fi
exit 2
"#;

const GENUINE: &[u8] = br#"#!/bin/sh
if [ "$1" = "docker-cli-plugin-metadata" ]; then
  echo '{"SchemaVersion":"0.1.0","Vendor":"fake","Version":"v0.38.0"}'
  exit 0
fi
echo "$2" >> "$FAKE_DIR/plugin.ran"
if [ "$2" = "version" ]; then
  echo "github.com/docker/buildx v0.38.0 fake"
  exit 0
fi
if [ "$2" = "build" ]; then
  env > "$FAKE_DIR/docker.env"
  for argument in "$@"; do
    case "$argument" in
      type=oci,dest=*) dest="${argument#type=oci,dest=}"; dest="${dest%%,*}" ;;
    esac
  done
  . "$FAKE_DIR/behavior.sh"
fi
exit 2
"#;

pub(super) const PLANTED: &[u8] = br#"#!/bin/sh
echo planted >> "$FAKE_DIR/planted.ran"
cp "$FAKE_DIR/genuine-plugin" "$0"
if [ "$1" = "docker-cli-plugin-metadata" ]; then
  echo '{"SchemaVersion":"0.1.0","Vendor":"fake","Version":"v0.38.0"}'
fi
exit 0
"#;

const COPY: &str = r#"[ -f "$DOCKER_CONFIG/cli-plugins/docker-buildx" ] || exit 9
cp "$FAKE_DIR/archive.tar" "$dest"
exit 0"#;

fn executable(path: &Path, data: &[u8]) {
    write(path, data);
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

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
                sha256_hex(GENUINE)
            ),
        );
        git(&checkout, &["init", "-q"]);
        git(&checkout, &["add", "-A"]);
        git(&checkout, &["commit", "-q", "-m", "fixture"]);
        executable(&dir.join("bin/docker"), DOCKER.as_bytes());
        executable(&dir.join("genuine-plugin"), GENUINE);
        write(
            &dir.join("archive.tar"),
            build(&spec(vec![(plain_layer(), LAYER)])).archive(),
        );
        fs::create_dir(dir.join("out")).unwrap_or_else(|e| unreachable!("{e}"));
        let source = Self { dir };
        source.plugin(GENUINE);
        source.docker(COPY);
        source
    }

    pub(super) fn docker(&self, behavior: &str) {
        write(&self.dir.join("behavior.sh"), behavior);
    }

    pub(super) fn plugin(&self, bytes: &[u8]) {
        executable(
            &self.dir.join("userconfig/cli-plugins/docker-buildx"),
            bytes,
        );
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

    pub(super) fn spawn(&self) -> std::process::Child {
        self.command("build", &[], &[])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|e| unreachable!("{e}"))
    }

    fn invoke(
        &self,
        subcommand: &str,
        flags: &[(&str, PathBuf)],
        env: &[(&str, Option<&str>)],
    ) -> Run {
        let output = self
            .command(subcommand, flags, env)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        Run {
            ok: output.status.success(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        }
    }

    fn command(
        &self,
        subcommand: &str,
        flags: &[(&str, PathBuf)],
        env: &[(&str, Option<&str>)],
    ) -> Command {
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
            if !value.as_os_str().is_empty() {
                arguments.push((name, value.clone()));
            }
        }
        command.env_clear().args(["image", subcommand]);
        for (name, value) in arguments {
            command.arg(name).arg(value);
        }
        let mut vars = vec![
            ("PATH", path),
            ("FAKE_DIR", self.dir.display().to_string()),
            (
                "DOCKER_CONFIG",
                self.path("userconfig").display().to_string(),
            ),
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
            ("DOCKER_HOST", "tcp://elsewhere:2375".to_owned()),
        ];
        for (name, value) in env {
            vars.retain(|(known, _)| known != name);
            if let Some(value) = value {
                vars.push((name, (*value).to_owned()));
            }
        }
        command.envs(vars);
        command
    }
}
