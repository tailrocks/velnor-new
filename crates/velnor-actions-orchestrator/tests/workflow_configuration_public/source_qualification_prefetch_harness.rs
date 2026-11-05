// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

const PHASE: &str = "prefetch-locked-sources-for-qualification-workspaces";
const CARGO_STUB: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$STUB_LOG"
offline=0
manifest=
previous=
for argument in "$@"; do
  [ "$argument" = --offline ] && offline=1
  [ "$previous" = --manifest-path ] && manifest=$argument
  previous=$argument
done
relative=${manifest#"$GITHUB_WORKSPACE"/}
if [ "$offline" = 1 ]; then
  case " $STUB_OFFLINE_OK " in
    *" $relative "*) exit 0 ;;
  esac
  printf 'error: failed to download from `https://index.crates.io/config.json`\n\nCaused by:\n  attempting to make an HTTP request, but --offline was specified\n' >&2
  exit 101
fi
seen=
for restored in registry/index registry/cache git/db; do
  if [ -e "$CARGO_HOME/$restored" ]; then seen="$seen $restored=present"; else seen="$seen $restored=absent"; fi
done
printf '%s%s\n' "$relative" "$seen" >> "$STUB_NETWORK_SEEN"
if [ -e "$CARGO_HOME/registry/cache/corrupt.crate" ]; then
  printf 'error: failed to unpack package `block-buffer v0.12.1`\n\nCaused by:\n  failed to iterate over archive\n' >&2
  exit 101
fi
case " $STUB_NETWORK_FAIL " in
  *" $relative "*)
    printf 'error: failed to get `block-buffer` as a dependency\n' >&2
    exit 101
    ;;
esac
mktemp "$STUB_MARKERS/network.XXXXXX" > /dev/null
printf '    Updating crates.io index\n Downloading crates ...\n  Downloaded block-buffer v0.12.1\n  Downloaded digest v0.11.3\n  Downloaded 2 crates (80.0KiB) in 0.41s\n' >&2
exit 0
"#;

pub(super) struct Scenario {
    pub(super) cache_hit: &'static str,
    pub(super) matched_key: &'static str,
    pub(super) offline_ok: &'static [&'static str],
    pub(super) network_fails: &'static [&'static str],
}

pub(super) struct Outcome {
    pub(super) status: Option<i32>,
    pub(super) stdout: String,
    pub(super) calls: Vec<String>,
    pub(super) network_calls: usize,
    pub(super) network_sightings: Vec<String>,
    pub(super) identity: String,
    pub(super) lock_names: Vec<String>,
    workspace: String,
    _scratch: TempDir,
}

impl Outcome {
    pub(super) fn sequence(&self) -> Vec<String> {
        self.calls
            .iter()
            .map(|call| {
                let mode = if has_flag(call, "--offline") {
                    "offline"
                } else {
                    "network"
                };
                format!("{mode} {}", self.manifest_of(call))
            })
            .collect()
    }

    pub(super) fn fetch_modes(&self) -> Vec<&str> {
        self.identity
            .lines()
            .filter_map(|line| line.strip_prefix("fetch_mode="))
            .collect()
    }

    pub(super) fn field(&self, name: &str) -> Option<&str> {
        self.identity
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name}=")))
    }

    pub(super) fn fetched_manifests(&self) -> Vec<String> {
        let mut manifests: Vec<String> = self
            .calls
            .iter()
            .map(|call| self.manifest_of(call))
            .collect();
        manifests.sort();
        manifests.dedup();
        manifests
    }

    fn manifest_of(&self, call: &str) -> String {
        let mut arguments = call.split(' ');
        arguments
            .by_ref()
            .find(|argument| *argument == "--manifest-path");
        let path = arguments.next().unwrap_or_default();
        path.strip_prefix(&format!("{}/", self.workspace))
            .unwrap_or(path)
            .to_owned()
    }
}

pub(super) fn has_flag(call: &str, flag: &str) -> bool {
    call.split(' ').any(|argument| argument == flag)
}

pub(super) fn lock_workspace() -> Result<TempDir, Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    for (name, body) in [
        ("Cargo.toml", "[package]\nname = \"root\"\n"),
        ("Cargo.lock", "version = 4\n# root lock\n"),
        (
            "crates/velnor-runner/Cargo.toml",
            "[package]\nname = \"runner\"\n",
        ),
        (
            "crates/velnor-runner/Cargo.lock",
            "version = 4\n# runner lock\n",
        ),
    ] {
        let path = workspace.path().join(name);
        fs::create_dir_all(path.parent().ok_or("lock path has no parent")?)?;
        fs::write(path, body)?;
    }
    Ok(workspace)
}

pub(super) fn run_prefetch(
    workspace: &Path,
    scenario: &Scenario,
) -> Result<Outcome, Box<dyn Error>> {
    let scratch = tempfile::tempdir()?;
    let base = fs::canonicalize(scratch.path())?;
    let workspace = fs::canonicalize(workspace)?;
    let runner_temp = base.join("runner-temp");
    let stubs = base.join("stubs");
    let markers = base.join("markers");
    let log = base.join("calls.log");
    let evidence = runner_temp.join("source-qualification-artifacts");
    fs::create_dir_all(evidence.join("logs"))?;
    fs::create_dir_all(&stubs)?;
    fs::create_dir_all(&markers)?;
    fs::write(&log, "")?;
    let seen = base.join("network-seen.log");
    fs::write(&seen, "")?;
    let cargo_home = base.join("cargo-home");
    if !scenario.matched_key.is_empty() {
        for restored in ["registry/index", "registry/cache", "git/db"] {
            fs::create_dir_all(cargo_home.join(restored))?;
        }
        fs::write(
            cargo_home.join("registry/cache/corrupt.crate"),
            "not a crate",
        )?;
    }
    let stub = stubs.join("mise");
    fs::write(&stub, CARGO_STUB)?;
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755))?;
    let path = format!(
        "{}:{}",
        stubs.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/generator-release/source-qualification.sh");
    let output = Command::new("bash")
        .arg(script)
        .arg(PHASE)
        .current_dir(&workspace)
        .env("PATH", path)
        .env("RUNNER_TEMP", &runner_temp)
        .env("GITHUB_WORKSPACE", &workspace)
        .env("GITHUB_SHA", "0".repeat(40))
        .env("CARGO_HOME", &cargo_home)
        .env("VELNOR_RUST_VERSION", "1.0.0")
        .env("VELNOR_MBX_VERSION", "1.0.0")
        .env("SOURCES_CACHE_HIT", scenario.cache_hit)
        .env("SOURCES_CACHE_MATCHED_KEY", scenario.matched_key)
        .env("STUB_LOG", &log)
        .env("STUB_MARKERS", &markers)
        .env("STUB_NETWORK_SEEN", &seen)
        .env("STUB_OFFLINE_OK", scenario.offline_ok.join(" "))
        .env("STUB_NETWORK_FAIL", scenario.network_fails.join(" "))
        .output()?;
    let identity = fs::read_to_string(evidence.join("cargo-fetch-lock-identity.txt"))?;
    let lock_names = read_lock_names(&evidence.join("cargo-fetch-locks.before.txt"))?;
    Ok(Outcome {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        calls: fs::read_to_string(&log)?
            .lines()
            .map(str::to_owned)
            .collect(),
        network_calls: fs::read_dir(&markers)?.count(),
        network_sightings: fs::read_to_string(&seen)?
            .lines()
            .map(str::to_owned)
            .collect(),
        identity,
        lock_names,
        workspace: workspace.display().to_string(),
        _scratch: scratch,
    })
}

fn read_lock_names(path: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    Ok(fs::read_to_string(path)?
        .lines()
        .filter_map(|line| line.split_once("  ").map(|(_, name)| name.to_owned()))
        .collect())
}
