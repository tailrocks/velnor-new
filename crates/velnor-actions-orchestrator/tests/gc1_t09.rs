// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

use velnor_actions_contract::canonical::Digest;
use velnor_actions_contract::config::{CheckExecutor, CheckPlatform, CheckRunner, MiseCheck};
use velnor_actions_contract::graph::{CachePolicy, ResourceClass, ResourceDemand};
use velnor_actions_contract::propose::{IdentityInputs, ProposedTask};
use velnor_actions_contract::workflow::cache_context::{
    CacheCapability, CacheDriver, CompilerAdapterV1, ExecutionContextV1, JobBindingV1,
    LaneContextV1,
};
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};
use velnor_actions_mise::checks::CheckCapabilityProof;
use velnor_actions_mise::{CheckDeadline, DiscoveredCheck, QualifiedCheck};

fn digest(byte: u8) -> Result<Digest, Box<dyn Error>> {
    Ok(Digest::parse(&format!("b3-{byte:064x}"))?)
}

fn sha40(byte: u8) -> String {
    format!("{byte:040x}")
}

fn job() -> Result<JobBindingV1, Box<dyn Error>> {
    Ok(JobBindingV1 {
        schema: 1,
        repository_id: 1,
        repository: "tailrocks/velnor-new".to_owned(),
        run_id: 7,
        run_attempt: 1,
        job_id: 11,
        job_key: "reader".to_owned(),
        event: "push".to_owned(),
        ref_name: "refs/heads/main".to_owned(),
        workflow_sha: sha40(0xa5),
        source_sha: sha40(0x5a),
        plan_digest: digest(0x01)?,
    })
}

fn lane(
    id: &str,
    family: &str,
    target: &str,
    build: Option<&str>,
) -> Result<LaneContextV1, Box<dyn Error>> {
    Ok(LaneContextV1 {
        lane_id: id.to_owned(),
        command_family: family.to_owned(),
        variant_digest: digest(0x02)?,
        target_dir: target.to_owned(),
        build_dir: build.map(str::to_owned),
    })
}

fn context(root: &Path) -> Result<ExecutionContextV1, Box<dyn Error>> {
    let owned = root.join("owned");
    Ok(ExecutionContextV1 {
        schema: 1,
        job: job()?,
        generation: GenerationId {
            family: Gc1Family::State,
            group_digest: digest(0x10)?,
            compatibility_digest: digest(0x11)?,
            coverage_digest: digest(0x12)?,
            repair_revision: 0,
        },
        requested_coverage_digest: digest(0x20)?,
        source_binding_digest: digest(0x21)?,
        workspace_root: root.join("workspace").to_string_lossy().into_owned(),
        owned_root: owned.join("gc1").to_string_lossy().into_owned(),
        cargo_home: owned.join("cargo").to_string_lossy().into_owned(),
        rustup_home: owned.join("rustup").to_string_lossy().into_owned(),
        tool_root: owned.join("tools").to_string_lossy().into_owned(),
        transport_root: owned.join("transport").to_string_lossy().into_owned(),
        observation_root: owned.join("observations").to_string_lossy().into_owned(),
        lanes: vec![
            lane("build", "build", "target/build", Some("build/build"))?,
            lane("clippy", "clippy", "target/clippy", Some("build/clippy"))?,
            lane("doc", "doc", "target/doc", None)?,
        ],
        compiler_adapters: vec![
            CompilerAdapterV1 {
                stable_path: owned
                    .join("tools/adapters/rustc")
                    .to_string_lossy()
                    .into_owned(),
                executable_identity: "rustc 1.99.1".to_owned(),
            },
            CompilerAdapterV1 {
                stable_path: owned
                    .join("tools/adapters/rustdoc")
                    .to_string_lossy()
                    .into_owned(),
                executable_identity: "rustdoc 1.99.1".to_owned(),
            },
        ],
        selected_driver: CacheDriver::Cargo,
        mbx_target_views: false,
        effective_cache_capability: CacheCapability::Write,
        trusted_default_writer: true,
        private_audience_digest: digest(0x30)?,
    })
}

fn check_lane_separation(valid: &ExecutionContextV1) -> Result<(), Box<dyn Error>> {
    let targets: Vec<&str> = valid
        .lanes
        .iter()
        .map(|lane| lane.target_dir.as_str())
        .collect();
    if targets.len() != 3
        || targets[0] == targets[1]
        || targets[0] == targets[2]
        || targets[1] == targets[2]
    {
        return Err(io::Error::other("lanes share a target dir").into());
    }
    let builds: Vec<&str> = valid
        .lanes
        .iter()
        .filter_map(|lane| lane.build_dir.as_deref())
        .collect();
    if builds.len() != 2
        || builds[0] == builds[1]
        || targets.contains(&builds[0])
        || targets.contains(&builds[1])
    {
        return Err(io::Error::other("lanes share a build dir").into());
    }
    if valid
        .lanes
        .iter()
        .any(|lane| lane.build_dir.is_some() && lane.lane_id == "doc")
    {
        return Err(io::Error::other("doc lane carries a build dir").into());
    }
    Ok(())
}

fn discovered() -> DiscoveredCheck {
    let check = MiseCheck {
        id: "gc1-t09-check".to_owned(),
        task: "lint".to_owned(),
        directory: ".".to_owned(),
        runner: CheckRunner {
            label: "ubuntu-24.04".to_owned(),
            platform: CheckPlatform::LinuxX64,
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
            environment: BTreeMap::new(),
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
        qualified_tools: Vec::new(),
        qualification_digest: "gc1-t09".to_owned(),
        config_source: String::new(),
    }
}

fn qualified_at(home: &Path, cwd: &Path) -> Result<QualifiedCheck, Box<dyn Error>> {
    let found = discovered();
    Ok(QualifiedCheck::new(
        home.to_path_buf(),
        cwd.to_path_buf(),
        found,
        CheckCapabilityProof { container: None },
        &[],
    )?)
}

fn check_projection_preserves_task_bytes() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().join("home");
    let cwd = temp.path().join("repo");
    fs::create_dir_all(&home)?;
    fs::create_dir_all(&cwd)?;
    let qualified = qualified_at(&home, &cwd)?;
    let projection = qualified.bound_projection()?;
    if !projection.contains("run = \"echo ok\"") {
        return Err(io::Error::other("projection dropped task bytes").into());
    }
    if projection.contains("sources = ") || projection.contains("outputs = ") {
        return Err(io::Error::other("projection kept sources or outputs").into());
    }
    let want = format!("dir = \"{}\"", cwd.to_string_lossy());
    if !projection.contains(&want) {
        return Err(io::Error::other("projection lost check cwd").into());
    }
    Ok(())
}

fn stage_command(qualified: &QualifiedCheck, home: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(home.join("bin"))?;
    let mise = home.join("bin/mise");
    fs::write(&mise, "#!/bin/sh\necho \"2026.10.21 fake\"\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mise, fs::Permissions::from_mode(0o755))?;
    }
    fs::write(home.join("tasks.toml"), qualified.bound_projection()?)?;
    Ok(())
}

fn check_child_command_reads_homes() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().join("home");
    let cwd = temp.path().join("repo");
    fs::create_dir_all(&home)?;
    fs::create_dir_all(&cwd)?;
    let qualified = qualified_at(&home, &cwd)?;
    stage_command(&qualified, &home)?;
    let state = temp.path().join("owned/state/group");
    let cargo = temp.path().join("owned/cargo");
    let rustup = temp.path().join("owned/rustup");
    let tools = temp.path().join("owned/tools");
    let overlay = qualified.restored_home_overlay(&state, &cargo, &rustup, &tools)?;
    let deadline = CheckDeadline::after(Duration::from_secs(120))?;
    let command = qualified.command(deadline)?.with_env(&overlay)?;
    if command.cwd() != Some(&cwd) {
        return Err(io::Error::other("child command moved cwd").into());
    }
    if command.argv().last() != Some(&OsString::from("lint")) {
        return Err(io::Error::other("child command lost task bytes").into());
    }
    let full: BTreeMap<OsString, OsString> = command.full_env().into_iter().collect();
    if full.get(OsStr::new("CARGO_HOME")) != Some(&cargo.as_os_str().to_owned())
        || full.get(OsStr::new("RUSTUP_HOME")) != Some(&rustup.as_os_str().to_owned())
    {
        return Err(io::Error::other("child command missed restored homes").into());
    }
    if full.get(OsStr::new("HOME")) != Some(&home.as_os_str().to_owned()) {
        return Err(io::Error::other("child command left its private home").into());
    }
    let parent = vec![
        (
            OsString::from("GITHUB_TOKEN"),
            OsString::from("parent-secret"),
        ),
        (
            OsString::from("CARGO_REGISTRY_TOKEN"),
            OsString::from("parent-secret"),
        ),
        (OsString::from("HOME"), OsString::from("/parent")),
    ];
    let spawned = command.spawn_env(&parent);
    if spawned.iter().any(|(_, value)| value == "parent-secret") {
        return Err(io::Error::other("child command leaked parent credentials").into());
    }
    if spawned.iter().any(|(key, _)| key == "GITHUB_TOKEN") {
        return Err(io::Error::other("child command kept denied credential key").into());
    }
    Ok(())
}

fn check_lane_binding() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().join("home");
    let cwd = temp.path().join("repo");
    fs::create_dir_all(&home)?;
    fs::create_dir_all(&cwd)?;
    let qualified = qualified_at(&home, &cwd)?;
    let state = temp.path().join("owned/state/group");
    let mut targets = Vec::new();
    for lane in ["build", "clippy", "doc"] {
        let target = qualified.lane_target_dir(&state, lane)?;
        let build = qualified.lane_build_dir(&state, lane)?;
        if target != state.join(lane).join("target") || build != state.join(lane).join("build") {
            return Err(io::Error::other(format!("lane {lane} paths drifted")).into());
        }
        targets.push(target);
    }
    if targets[0] == targets[1] || targets[0] == targets[2] || targets[1] == targets[2] {
        return Err(io::Error::other("lane target dirs collide").into());
    }
    Ok(())
}

#[path = "gc1_t09_a.rs"]
mod gc1_t09_a;
