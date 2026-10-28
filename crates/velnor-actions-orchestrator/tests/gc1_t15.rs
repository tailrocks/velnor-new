// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, MiseCheck, QualifiedCargoInstallation,
    QualifiedTool, QualifiedToolArtifact, QualifiedToolBackend, QualifiedToolExecutable,
    QualifiedToolOptions, QualifiedToolPlatform, QualifiedToolProbe,
};
use velnor_actions_contract::graph::{CachePolicy, ResourceClass, ResourceDemand};
use velnor_actions_contract::propose::{IdentityInputs, ProposedTask};
use velnor_actions_contract::{canonical_json_bytes, digest_b3};
use velnor_actions_mise::check_tool_probes::{
    QualifiedExecutableObservation, QualifiedProbeHomes, validate_executable_proofs,
    verify_qualified_executable,
};
use velnor_actions_mise::checks::CheckCapabilityProof;
use velnor_actions_mise::{CheckDeadline, DiscoveredCheck, QualifiedCheck};
use velnor_actions_orchestrator::sha256_hex;

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn host_platform() -> Result<CheckPlatform, Box<dyn Error>> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok(CheckPlatform::MacosArm64),
        ("macos", "x86_64") => Ok(CheckPlatform::MacosX64),
        ("linux", "x86_64") => Ok(CheckPlatform::LinuxX64),
        (os, arch) => Err(fail(&format!("unsupported host {os}/{arch}"))),
    }
}

const TOOL_VERSION: &str = "1.2.3";
const RUST_VERSION: &str = "1.99.1";
const SRC_VERSION: &str = "0.4.1";
const SOURCE_LOCK: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn script_bytes() -> Vec<u8> {
    format!("#!/bin/sh\nprintf '%s\\n' \"mytool {TOOL_VERSION}\"\n").into_bytes()
}

fn declaration(name: &str, path: &str, bytes: &[u8], expected: &str) -> QualifiedToolExecutable {
    QualifiedToolExecutable {
        name: name.to_owned(),
        path: path.to_owned(),
        sha256: sha256_hex(bytes),
        probe: QualifiedToolProbe::Version {
            expected: expected.to_owned(),
        },
    }
}

fn tool_platform(
    platform: CheckPlatform,
    archive_url: &str,
    archive_sha: &str,
    tree_sha: &str,
    executables: Vec<QualifiedToolExecutable>,
) -> QualifiedToolPlatform {
    QualifiedToolPlatform {
        platform,
        artifacts: vec![QualifiedToolArtifact {
            url: archive_url.to_owned(),
            sha256: archive_sha.to_owned(),
        }],
        dependency_artifacts: Vec::new(),
        install_tree_sha256: tree_sha.to_owned(),
        executables,
    }
}

fn tree_placeholder(byte: u8) -> String {
    std::iter::repeat_n(char::from(b'a' + byte), 64).collect()
}

fn aqua_tool(
    platform: CheckPlatform,
    archive_sha: &str,
    exe: QualifiedToolExecutable,
) -> QualifiedTool {
    QualifiedTool {
        id: "mytool".to_owned(),
        backend: QualifiedToolBackend::Aqua {
            package: "acme/mytool".to_owned(),
        },
        version: TOOL_VERSION.to_owned(),
        options: QualifiedToolOptions::Default,
        depends_on: Vec::new(),
        platforms: vec![tool_platform(
            platform,
            "https://github.com/acme/mytool/releases/download/v1.2.3/mytool.tar.gz",
            archive_sha,
            &tree_placeholder(0),
            vec![exe],
        )],
    }
}

fn rust_tool(platform: CheckPlatform) -> QualifiedTool {
    let triple = platform.target();
    QualifiedTool {
        id: "rust".to_owned(),
        backend: QualifiedToolBackend::Core {
            tool: "rust".to_owned(),
        },
        version: RUST_VERSION.to_owned(),
        options: QualifiedToolOptions::Rust {
            components: vec!["rustc".to_owned()],
            targets: Vec::new(),
        },
        depends_on: Vec::new(),
        platforms: vec![tool_platform(
            platform,
            &format!("https://static.rust-lang.org/dist/rust-{RUST_VERSION}-{triple}.tar.gz"),
            &tree_placeholder(1),
            &tree_placeholder(2),
            vec![
                declaration(
                    "cargo",
                    "bin/cargo",
                    b"cargo-bytes",
                    &format!("cargo {RUST_VERSION}"),
                ),
                QualifiedToolExecutable {
                    name: "rustc".to_owned(),
                    path: "bin/rustc".to_owned(),
                    sha256: tree_placeholder(3),
                    probe: QualifiedToolProbe::RustcVerbose {
                        expected: format!(
                            "rustc {RUST_VERSION}\nbinary: rustc\ncommit-hash: {}\nhost: {triple}\nrelease: {RUST_VERSION}",
                            "a".repeat(40)
                        ),
                    },
                },
            ],
        )],
    }
}

fn source_tool(platform: CheckPlatform, exe: QualifiedToolExecutable) -> QualifiedTool {
    QualifiedTool {
        id: "srctool".to_owned(),
        backend: QualifiedToolBackend::Cargo {
            crate_name: "srctool".to_owned(),
        },
        version: SRC_VERSION.to_owned(),
        options: QualifiedToolOptions::Cargo {
            default_features: true,
            features: Vec::new(),
            installation: QualifiedCargoInstallation::Source {
                source_lock_sha256: SOURCE_LOCK.to_owned(),
            },
        },
        depends_on: vec!["rust".to_owned()],
        platforms: vec![tool_platform(
            platform,
            "https://crates.io/api/v1/crates/srctool/0.4.1/download",
            &tree_placeholder(4),
            &tree_placeholder(5),
            vec![exe],
        )],
    }
}

fn discovered(platform: CheckPlatform, tools: Vec<QualifiedTool>) -> DiscoveredCheck {
    let check = MiseCheck {
        id: "gc1-t15-check".to_owned(),
        task: "lint".to_owned(),
        directory: ".".to_owned(),
        runner: CheckRunner {
            label: "self-hosted".to_owned(),
            platform,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: Vec::new(),
        tools: Vec::new(),
        system_tools: Vec::new(),
        evidence: None,
        timeout_minutes: 10,
    };
    let proposal = ProposedTask {
        task_id: "internal/lint/dev".to_owned(),
        stack_id: "mise".to_owned(),
        component_id: "lint".to_owned(),
        task_kind: "check".to_owned(),
        configuration: "default".to_owned(),
        depends_on: Vec::new(),
        gated_by: Vec::new(),
        reads: Vec::new(),
        writes: Vec::new(),
        outputs: Vec::new(),
        resource: ResourceDemand {
            class: ResourceClass::Lightweight,
            cpu_milli: None,
            memory_mb: None,
            needs_network: false,
            service: None,
        },
        cache_policy: CachePolicy {
            allow_compilation_reuse: false,
            allow_task_reuse: false,
        },
        identity: IdentityInputs {
            unit_id: "internal/lint/dev".to_owned(),
            unit_key: "lint".to_owned(),
            unit_path: "mise.toml".to_owned(),
            project_root: ".".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            features: Vec::new(),
            flags: Vec::new(),
            compile_driver: "cargo".to_owned(),
            test_runner: "cargo".to_owned(),
            environment: std::collections::BTreeMap::new(),
            declared_inputs: Vec::new(),
            undeclared_reads: false,
        },
        payload: vec![OsString::from("echo")],
        display_name: "lint".to_owned(),
        uses_clock: false,
        uses_random: false,
        no_targets: true,
        runner_profile: "host".to_owned(),
    };
    let task_config =
        "[tasks.lint]\nrun = \"echo ok\"\nsources = [\"a\"]\noutputs = [\"b\"]\n".to_owned();
    DiscoveredCheck {
        check,
        proposal,
        config_inputs: Vec::new(),
        task_config,
        tool_specs: Vec::new(),
        qualified_tools: tools,
        qualification_digest: "gc1-t15".to_owned(),
        config_source: String::new(),
    }
}

fn qualified_at(
    home: &Path,
    cwd: &Path,
    tools: Vec<QualifiedTool>,
) -> Result<QualifiedCheck, Box<dyn Error>> {
    Ok(QualifiedCheck::new(
        home.to_path_buf(),
        cwd.to_path_buf(),
        discovered(host_platform()?, tools),
        CheckCapabilityProof { container: None },
        &[],
        velnor_actions_mise::MISE_VERSION.to_owned(),
    )?)
}

fn canonical_temp() -> Result<(tempfile::TempDir, PathBuf), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    Ok((temp, root))
}

fn fixture_tools(platform: CheckPlatform) -> Result<(Vec<QualifiedTool>, Vec<u8>), Box<dyn Error>> {
    let script = script_bytes();
    let archive: Vec<u8> = b"archive-bytes".to_vec();
    let archive_sha = sha256_hex(&archive);
    let tools = vec![
        aqua_tool(
            platform,
            &archive_sha,
            declaration(
                "mytool",
                "mytool",
                &script,
                &format!("mytool {TOOL_VERSION}"),
            ),
        ),
        rust_tool(platform),
        source_tool(
            platform,
            declaration(
                "srctool",
                "srctool",
                b"srctool-bytes",
                &format!("srctool {SRC_VERSION}"),
            ),
        ),
    ];
    for tool in &tools {
        tool.validate("qualified_tools", &tool.id)?;
    }
    Ok((tools, archive))
}

fn check_shared_paths_deterministic(root: &Path) -> Result<QualifiedCheck, Box<dyn Error>> {
    let platform = host_platform()?;
    let (tools, _) = fixture_tools(platform)?;
    let first = root.join("velnor-check-9-0");
    let second = root.join("velnor-check-9-1");
    let cwd = root.join("repo");
    fs::create_dir_all(&first)?;
    fs::create_dir_all(&second)?;
    fs::create_dir_all(&cwd)?;
    let one = qualified_at(&first, &cwd, tools.clone())?;
    let two = qualified_at(&second, &cwd, tools)?;
    let shared = root.join("velnor-gc1").join("tools").join("mytool");
    if one.qualified_shared_tool_home("mytool")? != shared
        || two.qualified_shared_tool_home("mytool")? != shared
    {
        return Err(fail("shared tool home drifted across private homes"));
    }
    if one.qualified_shared_prefix("mytool")? != shared.join("prefix") {
        return Err(fail("shared prefix drifted"));
    }
    let archive_sha = sha256_hex(b"archive-bytes");
    if one.qualified_shared_archive("mytool", &archive_sha)?
        != shared.join("archives").join(&archive_sha)
    {
        return Err(fail("shared archive drifted"));
    }
    for home in [&first, &second] {
        if shared == *home
            || shared.starts_with(home)
            || home.starts_with(&shared)
            || shared.starts_with(&cwd)
        {
            return Err(fail("shared prefix overlaps a private home"));
        }
    }
    Ok(one)
}

#[path = "gc1_t15_a.rs"]
mod gc1_t15_a;
#[path = "gc1_t15_b.rs"]
mod gc1_t15_b;
pub(crate) use gc1_t15_a::*;
