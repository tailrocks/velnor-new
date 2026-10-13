//! Runtime safety fixtures for the fixed MBX evidence scripts.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use velnor_actions_contract::RoutingWorkflow;
use velnor_actions_workflow_renderer::{
    MbxQualificationPins, MiseSetup, Schema2WorkflowRequest, render_schema2_workflows,
    schema2::QUALIFICATION_WORKFLOW,
};

const START: &str = include_str!("../src/schema2_mbx_resource_start.sh");
const SAMPLER: &str = include_str!("../src/schema2_mbx_resource_sampler.sh");
const PATH_VALIDATION: &str = include_str!("../src/schema2_mbx_resource_path.sh");
const STOP: &str = include_str!("../src/schema2_mbx_resource_stop.sh");
const RECEIPT: &str = include_str!("../src/schema2_mbx_resource_receipt.sh");
const CORRUPTOR: &str = include_str!("../src/schema2_mbx_corrupt_bundle.sh");
const SAMPLER_SHA: &str = "b2a03511f0a36c6b7fca9fb4a95461676acbd54b82e1e902efa1313d3b8fcfd2";
const PATH_VALIDATION_SHA: &str =
    "5bcca14cce52a2d66891e384f6f9ab5a6b46819e8f9c30a6d1c779e665cf7ada";
const LINUX_IMAGE: &str = concat!(
    "ghcr.io/actions/actions-runner@sha256:",
    "e5496277be5d09bc968b3d64911b74e219ac4a3f2edce956a3ecf9271bea1ef4"
);
const RUNTIME_IMAGE_ENV: &str = "VELNOR_MBX_RUNTIME_IMAGE";
const RUNTIME_PLATFORM_ENV: &str = "VELNOR_MBX_RUNTIME_PLATFORM";

const FIXTURE: &str = include_str!("mbx_resource_safety_runtime_fixture.sh");
const SESSION_FIXTURE: &str = include_str!("mbx_resource_safety_session_fixture.sh");
const ROLE_FIXTURE: &str = include_str!("mbx_resource_safety_role_fixture.sh");

fn scratch_dir() -> std::io::Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(std::io::Error::other)?
        .as_nanos();
    let base = if cfg!(target_os = "macos") {
        PathBuf::from("/private/tmp")
    } else {
        std::env::temp_dir()
    };
    let path = base.join(format!("velnor-mbx-runtime-{}-{stamp}", std::process::id()));
    fs::create_dir(&path)?;
    Ok(path)
}

fn write_script(root: &Path, name: &str, value: &str) -> std::io::Result<()> {
    fs::write(root.join(name), value)
}

fn docker_text(args: &[&str]) -> std::io::Result<String> {
    let output = Command::new("docker").args(args).output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "docker {} exited {}: {}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn normalize_linux_platform(value: &str) -> Option<String> {
    let (os, architecture) = value.split_once('/')?;
    let architecture = architecture.split('/').next()?;
    let architecture = match (os, architecture) {
        ("linux", "amd64" | "x86_64") => "amd64",
        ("linux", "arm64" | "aarch64") => "arm64",
        _ => return None,
    };
    Some(format!("linux/{architecture}"))
}

fn validate_platforms(
    daemon: &str,
    requested: Option<&str>,
    image: &str,
    image_platform: &str,
) -> std::io::Result<String> {
    let daemon = normalize_linux_platform(daemon).ok_or_else(|| {
        std::io::Error::other(format!("unsupported Docker daemon platform {daemon:?}"))
    })?;
    let target = requested
        .map(|platform| {
            normalize_linux_platform(platform).ok_or_else(|| {
                std::io::Error::other(format!("unsupported runtime platform {platform:?}"))
            })
        })
        .transpose()?
        .unwrap_or_else(|| daemon.clone());
    if target != daemon {
        return Err(std::io::Error::other(format!(
            "runtime platform {target} differs from native Docker daemon {daemon}; emulation is disabled"
        )));
    }
    let image_platform = normalize_linux_platform(image_platform).ok_or_else(|| {
        std::io::Error::other(format!(
            "unsupported runtime image platform {image_platform:?} for {image}"
        ))
    })?;
    if image_platform != target {
        return Err(std::io::Error::other(format!(
            "runtime image {image} is {image_platform}, native Docker platform is {target}; set {RUNTIME_IMAGE_ENV} to a native image"
        )));
    }
    Ok(target)
}

fn validate_native_docker_image(image: &str, requested: Option<&str>) -> std::io::Result<String> {
    let daemon = docker_text(&["info", "--format", "{{.OSType}}/{{.Architecture}}"])?;
    let image_platform = docker_text(&[
        "image",
        "inspect",
        "--format",
        "{{.Os}}/{{.Architecture}}",
        image,
    ])?;
    validate_platforms(&daemon, requested, image, &image_platform)
}

fn fixture_output(root: &Path) -> std::io::Result<Output> {
    let mut command = if cfg!(target_os = "linux") {
        let mut native = Command::new("timeout");
        native.args(["120s", "bash"]);
        native.arg(root.join("fixture.sh"));
        native.env("WORK", root);
        native
    } else {
        let image = std::env::var(RUNTIME_IMAGE_ENV).unwrap_or_else(|_| LINUX_IMAGE.to_owned());
        let requested_platform = std::env::var(RUNTIME_PLATFORM_ENV).ok();
        let platform = validate_native_docker_image(&image, requested_platform.as_deref())?;
        let uid = Command::new("id").arg("-u").output()?;
        let gid = Command::new("id").arg("-g").output()?;
        let uid = String::from_utf8_lossy(&uid.stdout).trim().to_owned();
        let gid = String::from_utf8_lossy(&gid.stdout).trim().to_owned();
        let mount = format!("{}:/input:ro", root.display());
        let work_tmpfs = format!("/work:rw,exec,nosuid,size=128m,uid={uid},gid={gid},mode=700");
        let mut docker = Command::new("docker");
        docker.args(["run", "--rm", "--init"]);
        docker.arg(format!("--platform={platform}"));
        docker.args([
            "--network=none",
            "--cpus=0.5",
            "--memory=256m",
            "--pids-limit=64",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--read-only",
            "--tmpfs",
            "/tmp:rw,nosuid,size=32m",
            "--tmpfs",
            &work_tmpfs,
            "--user",
            &format!("{uid}:{gid}"),
            "-e",
            "WORK=/work",
            "-v",
            &mount,
            "-w",
            "/work",
            "--entrypoint",
            "/bin/bash",
            &image,
            "-lc",
            "cp /input/* /work/ && exec timeout 240s bash /work/fixture.sh",
        ]);
        docker.env("WORK", "/work");
        docker
    };
    command.output()
}

#[test]
fn fixed_scripts_reject_runtime_safety_regressions() -> std::io::Result<()> {
    let root = scratch_dir()?;
    write_script(&root, "start.template", START)?;
    write_script(&root, "sampler.sh", SAMPLER)?;
    write_script(&root, "path-validation.sh", PATH_VALIDATION)?;
    write_script(&root, "stop.sh", STOP)?;
    write_script(&root, "receipt.sh", RECEIPT)?;
    write_script(&root, "corruptor.sh", CORRUPTOR)?;
    write_script(&root, "session-fixture.sh", SESSION_FIXTURE)?;
    write_script(&root, "role-fixture.sh", ROLE_FIXTURE)?;
    let fixture = FIXTURE
        .replace("__SAMPLER_SHA__", SAMPLER_SHA)
        .replace("__PATH_VALIDATION_SHA__", PATH_VALIDATION_SHA);
    write_script(&root, "fixture.sh", &fixture)?;
    let output = fixture_output(&root)?;
    let transcript = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{transcript}");
    assert!(
        output.status.success(),
        "runtime fixture failed:\n{transcript}"
    );
    fs::remove_dir_all(root)?;
    Ok(())
}

fn qualification_workflow() -> Result<String, Box<dyn std::error::Error>> {
    let request = Schema2WorkflowRequest {
        version: "0.1.1".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from([RoutingWorkflow::Qualification]),
        mbx_qualification: Some(MbxQualificationPins {
            mise_setup: MiseSetup {
                uses: "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5".to_owned(),
                version: "2026.9.18".to_owned(),
                sha256: "d24fe0bf7e613824ad99f7b8dac3f2b381a37b9f75f84dd250855217095a8de4"
                    .to_owned(),
            },
            mbx_action_uses: format!("jdx/mr-boxington-action@{}", "a".repeat(40)),
            mbx_version: "1.22.0".to_owned(),
            rust_version: "1.98.1".to_owned(),
        }),
    };
    render_schema2_workflows(&request)?
        .into_iter()
        .find(|file| file.path == QUALIFICATION_WORKFLOW)
        .map(|file| file.bytes)
        .ok_or_else(|| "qualification workflow missing".into())
}

#[test]
fn rendered_stop_step_supplies_receipt_identity_environment()
-> Result<(), Box<dyn std::error::Error>> {
    let workflow = qualification_workflow()?;
    let stop_start = workflow
        .find("name: Stop sampler and capture final MBX state")
        .ok_or("stop step missing")?;
    let stop_end = workflow[stop_start..]
        .find("\n      - name:")
        .map_or(workflow.len(), |offset| stop_start + offset);
    let block = &workflow[stop_start..stop_end];
    for key in [
        "MBX_QUALIFICATION_CACHE_PRIMARY:",
        "MBX_QUALIFICATION_CACHE_PREFIX:",
        "MBX_QUALIFICATION_ROLE:",
        "MBX_QUALIFICATION_CACHE_GENERATION:",
        "MBX_QUALIFICATION_RUSTC_IDENTITY:",
        "MBX_QUALIFICATION_CACHE_MATCHED_KEY:",
        "MBX_QUALIFICATION_CACHE_HIT:",
        "MBX_QUALIFICATION_RESTORE_PRIMARY_KEY:",
        "MBX_QUALIFICATION_RESTORE_CONCLUSION:",
    ] {
        assert!(block.contains(key), "rendered stop env missing {key}");
    }
    for output in [
        "steps.mbx-bundle-key.outputs.primary",
        "steps.mbx-bundle.outputs.cache-matched-key",
        "steps.mbx-bundle.outputs.cache-hit",
        "steps.mbx-bundle.outputs.cache-primary-key",
        "steps.mbx-bundle.conclusion",
    ] {
        assert!(
            block.contains(output),
            "rendered stop env does not bind stock output {output}"
        );
    }
    Ok(())
}

#[test]
fn docker_platform_aliases_normalize_to_native_architectures() {
    assert_eq!(
        normalize_linux_platform("linux/x86_64").as_deref(),
        Some("linux/amd64")
    );
    assert_eq!(
        normalize_linux_platform("linux/aarch64").as_deref(),
        Some("linux/arm64")
    );
    assert_eq!(
        normalize_linux_platform("linux/arm64/v8").as_deref(),
        Some("linux/arm64")
    );
    assert_eq!(normalize_linux_platform("windows/amd64"), None);
}

#[test]
fn docker_platform_validation_rejects_emulation_and_wrong_arch_images() {
    assert!(validate_platforms("linux/aarch64", None, "amd64-image", "linux/amd64").is_err());
    assert!(
        validate_platforms(
            "linux/aarch64",
            Some("linux/amd64"),
            "arm64-image",
            "linux/arm64"
        )
        .is_err()
    );
    assert!(matches!(
        validate_platforms("linux/aarch64", None, "arm64-image", "linux/arm64"),
        Ok(platform) if platform == "linux/arm64"
    ));
}
