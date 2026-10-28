// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, MiseCheck, QualifiedTool,
};
use velnor_actions_mise::checks::verify_check_capabilities;
use velnor_actions_mise::{
    CheckDeadline, MISE_VERSION, QualifiedCheck, RUSTUP_TOOLCHAIN_ENV, discover_checks,
};

const TASK_CONFIG: &str =
    "[tools]\nrust = \"1.98.0\"\n\n[tasks.named-rust-selection]\nrun = \"rustc --version\"\n";

const QUALIFIED_RUST_1991: &str = r#"{
  "id": "rust",
  "backend": { "kind": "core", "tool": "rust" },
  "version": "1.99.1",
  "options": { "kind": "rust", "components": [], "targets": [] },
  "depends_on": [],
  "platforms": [{
    "platform": "linux_x64",
    "artifacts": [
      { "url": "https://static.rust-lang.org/dist/2026-10-15/cargo-1.99.1-x86_64-unknown-linux-gnu.tar.xz", "sha256": "53f3521949404d595efb06a55c12c5658d4ed1c75566f33d3d514eaebe70bd8a" },
      { "url": "https://static.rust-lang.org/dist/2026-10-15/rust-std-1.99.1-x86_64-unknown-linux-gnu.tar.xz", "sha256": "109b074225bf5b9ea3820890876bfdf4ed161adbfaf94b2b4b5984c85ee5d958" },
      { "url": "https://static.rust-lang.org/dist/2026-10-15/rustc-1.99.1-x86_64-unknown-linux-gnu.tar.xz", "sha256": "2ff05ff9c08a90861e7f6e4065b40a8b87463b4576186fd613871c3e37f2091f" }
    ],
    "dependency_artifacts": [],
    "install_tree_sha256": "baa8089f830b6c49980b5c2274a0863d963d2eae8ef7f8ece56b90dfae8a8625",
    "executables": [
      {
        "name": "cargo",
        "path": "bin/cargo",
        "sha256": "a670e02cfa39ac2e53f77a2d911ca1fc9e2c479b0603ec956625be1daf24e3af",
        "probe": { "kind": "version", "expected": "cargo 1.99.1 (5f94df478 2026-08-27)" }
      },
      {
        "name": "rustc",
        "path": "bin/rustc",
        "sha256": "b53b780f77f952062f8ca8a3b70aef2390b1e45ad91939be94afdc9543026853",
        "probe": {
          "kind": "rustc_verbose",
          "expected": "rustc 1.99.1 (58cf8f9c3 2026-10-13)\nbinary: rustc\ncommit-hash: 58cf8f9c35b355199727627d07ae34a4e2846d67\ncommit-date: 2026-10-13\nhost: x86_64-unknown-linux-gnu\nrelease: 1.99.1\nLLVM version: 23.1.1"
        }
      }
    ]
  }]
}"#;

struct TempTree(PathBuf);

impl TempTree {
    fn new() -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path =
            std::env::temp_dir().join(format!("velnor-named-rust-{}-{nonce}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn find_mise() -> Result<PathBuf, Box<dyn Error>> {
    let path = std::env::var_os("PATH").ok_or_else(|| io::Error::other("PATH is missing"))?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join("mise");
        if candidate.is_file() {
            return Ok(candidate.canonicalize()?);
        }
    }
    Err(io::Error::other("pinned mise is not available on PATH").into())
}

#[test]
fn gc1_t19_named_check_uses_its_qualified_rust_prefix() -> Result<(), Box<dyn Error>> {
    let qualified_rust: QualifiedTool = serde_json::from_str(QUALIFIED_RUST_1991)?;
    let check = MiseCheck {
        id: "named-rust-selection".to_owned(),
        task: "named-rust-selection".to_owned(),
        directory: ".".to_owned(),
        runner: CheckRunner {
            label: "ubuntu-26.04".to_owned(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: vec!["mise.toml".to_owned()],
        tools: vec!["rust".to_owned()],
        system_tools: Vec::new(),
        evidence: None,
        timeout_minutes: 5,
    };
    let tree = TempTree::new()?;
    let root = tree.path().join("project");
    let home = tree.path().join("home");
    fs::create_dir(&root)?;
    fs::create_dir_all(home.join("bin"))?;
    fs::write(root.join("mise.toml"), TASK_CONFIG)?;

    let discovered = discover_checks(
        &root,
        &[check.clone()],
        std::slice::from_ref(&qualified_rust),
    )?;
    assert_eq!(discovered.len(), 1);
    let discovered = discovered
        .into_iter()
        .next()
        .ok_or_else(|| io::Error::other("discovery returned no check"))?;
    assert_eq!(discovered.tool_specs, ["rust@1.99.1"]);
    assert_eq!(discovered.qualified_tools, [qualified_rust]);
    assert_eq!(discovered.config_source, TASK_CONFIG);

    let deadline = CheckDeadline::after(Duration::from_secs(30))?;
    let proof = verify_check_capabilities(&check.runner, None, deadline)?;
    let qualified = QualifiedCheck::new(
        home.clone(),
        root.clone(),
        discovered,
        proof,
        &[],
        MISE_VERSION.to_owned(),
    )?;
    let projection = qualified.bound_projection()?;
    fs::write(home.join("tasks.toml"), projection)?;

    let source_mise = find_mise()?;
    let staged_mise = home.join("bin/mise");
    fs::copy(source_mise, &staged_mise)?;
    let command = qualified.command(deadline)?;

    assert_eq!(command.program(), staged_mise.as_os_str());
    assert_eq!(command.cwd(), Some(&root));
    let rustup_toolchain = command
        .full_env()
        .into_iter()
        .find(|(key, _)| key == OsStr::new(RUSTUP_TOOLCHAIN_ENV))
        .map(|(_, value)| PathBuf::from(value))
        .ok_or_else(|| io::Error::other("qualified command omitted RUSTUP_TOOLCHAIN"))?;
    assert_eq!(rustup_toolchain, home.join("tools/rust/prefix"));
    assert_eq!(fs::read(root.join("mise.toml"))?, TASK_CONFIG.as_bytes());
    Ok(())
}
