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
const CORRUPTOR: &str = include_str!("../src/schema2_mbx_corrupt_bundle.sh");
const SAMPLER_SHA: &str = "f8ee1c598e2eae6814e2ec82d2c990ad7bce4cbe5ef90c51f1c7e74d537a592a";
const PATH_VALIDATION_SHA: &str =
    "80a92b0cfe88f41c3ae72822c735c8c8861643598c7da0d349a1d6c77e99a48d";
const LINUX_IMAGE: &str = "velnor-runner:ubuntu-26.04-2.337.0";
const RUNTIME_IMAGE_ENV: &str = "VELNOR_MBX_RUNTIME_IMAGE";
const RUNTIME_PLATFORM_ENV: &str = "VELNOR_MBX_RUNTIME_PLATFORM";

const FIXTURE: &str = include_str!("mbx_resource_safety_runtime_fixture.sh");
const SESSION_FIXTURE: &str = include_str!("mbx_resource_safety_session_fixture.sh");

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

fn fixture_output(root: &Path) -> std::io::Result<Output> {
    let mut command = if cfg!(target_os = "linux") {
        let mut native = Command::new("timeout");
        native.args(["120s", "bash"]);
        native.arg(root.join("fixture.sh"));
        native.env("WORK", root);
        native
    } else {
        let image = std::env::var(RUNTIME_IMAGE_ENV).unwrap_or_else(|_| LINUX_IMAGE.to_owned());
        let platform = std::env::var(RUNTIME_PLATFORM_ENV).ok();
        let uid = Command::new("id").arg("-u").output()?;
        let gid = Command::new("id").arg("-g").output()?;
        let uid = String::from_utf8_lossy(&uid.stdout).trim().to_owned();
        let gid = String::from_utf8_lossy(&gid.stdout).trim().to_owned();
        let mount = format!("{}:/input:ro", root.display());
        let work_tmpfs = format!("/work:rw,exec,nosuid,size=128m,uid={uid},gid={gid},mode=700");
        let mut docker = Command::new("docker");
        docker.args(["run", "--rm", "--init"]);
        if let Some(platform) = platform {
            docker.arg(format!("--platform={platform}"));
        }
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
            "cp /input/* /work/ && exec timeout 120s bash /work/fixture.sh",
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
    write_script(&root, "corruptor.sh", CORRUPTOR)?;
    write_script(&root, "session-fixture.sh", SESSION_FIXTURE)?;
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
        "MBX_QUALIFICATION_CACHE_GENERATION:",
        "MBX_QUALIFICATION_RUSTC_IDENTITY:",
    ] {
        assert!(block.contains(key), "rendered stop env missing {key}");
    }
    Ok(())
}
