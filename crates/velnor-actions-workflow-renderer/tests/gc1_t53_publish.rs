// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_workflow_renderer::steps::{CompileDriver, MbxTransport, mbx_steps_for_driver};

use super::*;
pub(super) fn shell_scratch(name: &str) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let root = std::env::temp_dir().join(format!(
        "velnor-t53-{}-{}-{name}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| fail(&format!("clock unreadable: {error}")))?
            .as_nanos()
    ));
    std::fs::create_dir_all(&root)
        .map_err(|error| fail(&format!("scratch unwritable: {error}")))?;
    Ok(root)
}

pub(super) fn github_output_value(
    path: &std::path::Path,
    key: &str,
) -> Result<String, Box<dyn Error>> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| fail(&format!("github output unreadable: {error}")))?;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix(&format!("{key}=")) {
            return Ok(value.to_owned());
        }
    }
    Err(fail(&format!("github output lost: {key}")))
}

struct PublishSetup {
    scratch: std::path::PathBuf,
    runner: std::path::PathBuf,
    workspace: std::path::PathBuf,
    github_output: std::path::PathBuf,
    export: String,
    record: String,
}

fn publish_setup() -> Result<PublishSetup, Box<dyn Error>> {
    let text = render_text()?;
    let export = extract_run(step_section(&text, EXPORT_STEP)?, "export")?;
    let record = extract_run(step_section(&text, RECORD_STEP)?, "record")?;
    let scratch = shell_scratch("publish")?;
    let runner = scratch.join("runner");
    let workspace = scratch.join("workspace");
    std::fs::create_dir_all(runner.join("velnor"))
        .map_err(|error| fail(&format!("runner unwritable: {error}")))?;
    std::fs::create_dir_all(&workspace)
        .map_err(|error| fail(&format!("workspace unwritable: {error}")))?;
    std::fs::write(workspace.join("Cargo.lock"), "lock-fixture")
        .map_err(|error| fail(&format!("lock unwritable: {error}")))?;
    Ok(PublishSetup {
        github_output: scratch.join("github-output"),
        scratch,
        runner,
        workspace,
        export,
        record,
    })
}

/// Run the discovery prelude so the export/record scripts see a trusted
/// discovery record with a matching compat digest.
fn publish_discover(setup: &PublishSetup) -> Result<(), Box<dyn Error>> {
    let steps = mbx_steps_for_driver(
        &format!("jdx/mr-boxington-action@{}", "a".repeat(40)),
        CompileDriver::Mbx,
        "1.2.3",
        "1.89.0",
        driver_env()?,
        MbxTransport::SeedRead,
        None,
    )?
    .ok_or_else(|| fail("mbx driver produced no steps"))?;
    let discover = shell_script_of(&steps[3])?;
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(&discover)
        .env("RUNNER_TEMP", &setup.runner)
        .env("GITHUB_WORKSPACE", &setup.workspace)
        .env("GITHUB_OUTPUT", &setup.github_output)
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_REF_PROTECTED", "true")
        .env("VELNOR_DEFAULT_REF", "refs/heads/main")
        .env("VELNOR_MBX_BUNDLE_VERSION", "1.2.3")
        .env("VELNOR_MBX_BUNDLE_TOOLCHAIN", "1.89.0")
        .env("VELNOR_MBX_BUNDLE_ROOT", ".")
        .status()
        .map_err(|error| fail(&format!("discover failed to run: {error}")))?;
    if !status.success() {
        return Err(fail("discover script failed on trusted push"));
    }
    if github_output_value(&setup.github_output, "provenance")? != "trusted" {
        return Err(fail("discover misclassified a trusted push"));
    }
    // The producer built the state root in-job: record its success exactly
    // like the producer-mark step does.
    std::fs::write(setup.runner.join("velnor/state-producer-ok"), "")
        .map_err(|error| fail(&format!("mark unwritable: {error}")))?;
    Ok(())
}

fn write_state_file(setup: &PublishSetup, name: &str, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let root = setup.runner.join("velnor-gc1/state");
    std::fs::create_dir_all(&root)
        .map_err(|error| fail(&format!("state root unwritable: {error}")))?;
    std::fs::write(root.join(name), bytes)
        .map_err(|error| fail(&format!("state unwritable: {error}")))?;
    Ok(())
}

/// Fill the state root with real bytes over the family ceiling. Real bytes
/// (not a sparse hole) so `du` reports them on every platform.
fn write_over_ceiling_state(setup: &PublishSetup) -> Result<(), Box<dyn Error>> {
    let root = setup.runner.join("velnor-gc1/state");
    std::fs::create_dir_all(&root)
        .map_err(|error| fail(&format!("state root unwritable: {error}")))?;
    let chunk = vec![0u8; 1024 * 1024];
    let mut file = std::fs::File::create(root.join("bulk"))
        .map_err(|error| fail(&format!("bulk unwritable: {error}")))?;
    for _ in 0..(OVER_CEILING_BYTES / 1024 / 1024) {
        file.write_all(&chunk)
            .map_err(|error| fail(&format!("bulk unwritable: {error}")))?;
    }
    Ok(())
}

fn run_export(setup: &PublishSetup) -> Result<String, Box<dyn Error>> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&setup.export)
        .env("RUNNER_TEMP", &setup.runner)
        .env("GITHUB_WORKSPACE", &setup.workspace)
        .env("GITHUB_OUTPUT", &setup.github_output)
        .env("MBX_CACHE_EXPORT_GROUP", "velnor")
        .output()
        .map_err(|error| fail(&format!("export failed to run: {error}")))?;
    if !output.status.success() {
        return Err(fail(&format!(
            "export script failed the step: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn run_record(setup: &PublishSetup) -> Result<String, Box<dyn Error>> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&setup.record)
        .env("RUNNER_TEMP", &setup.runner)
        .env("GITHUB_WORKSPACE", &setup.workspace)
        .output()
        .map_err(|error| fail(&format!("record failed to run: {error}")))?;
    if !output.status.success() {
        return Err(fail(&format!(
            "record script failed the step: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

#[test]
#[cfg(unix)]
fn gc1_t53_export_stages_for_save() -> Result<(), Box<dyn Error>> {
    let setup = publish_setup()?;
    publish_discover(&setup)?;
    write_state_file(&setup, "payload", b"state bytes")?;
    let output = run_export(&setup)?;
    require(&output, "readback verified", "export behavior")?;
    forbid(&output, "state snapshot published (", "export behavior")?;
    if github_output_value(&setup.github_output, "exported")? != "true" {
        return Err(fail("export must admit a small snapshot"));
    }
    if !setup
        .runner
        .join("velnor/state-publication.staged")
        .is_file()
    {
        return Err(fail("export must stage its attestation"));
    }
    if setup
        .runner
        .join("velnor/state-publication.record")
        .exists()
    {
        return Err(fail("export must not write the publication record"));
    }
    // The save succeeded: the record step now publishes the evidence.
    let recorded = run_record(&setup)?;
    require(&recorded, "state snapshot published (", "record behavior")?;
    if !setup
        .runner
        .join("velnor/state-publication.record")
        .is_file()
    {
        return Err(fail("record must write the publication record"));
    }
    std::fs::remove_dir_all(&setup.scratch)
        .map_err(|error| fail(&format!("scratch stuck: {error}")))?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn gc1_t53_record_without_save_writes_nothing() -> Result<(), Box<dyn Error>> {
    let setup = publish_setup()?;
    publish_discover(&setup)?;
    write_state_file(&setup, "payload", b"state bytes")?;
    // The save never ran: no staged attestation exists.
    let output = run_record(&setup)?;
    forbid(&output, "state snapshot published (", "record behavior")?;
    if setup
        .runner
        .join("velnor/state-publication.record")
        .exists()
    {
        return Err(fail("record must not publish without a save"));
    }
    std::fs::remove_dir_all(&setup.scratch)
        .map_err(|error| fail(&format!("scratch stuck: {error}")))?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn gc1_t53_export_refuses_uncompressed_over_ceiling() -> Result<(), Box<dyn Error>> {
    let setup = publish_setup()?;
    publish_discover(&setup)?;
    write_over_ceiling_state(&setup)?;
    let output = run_export(&setup)?;
    // Highly compressible zeros: the compressed tarball would fit, but the
    // uncompressed root must be refused before packing.
    require(&output, "not published (over_ceiling)", "export ceiling")?;
    forbid(&output, "state snapshot published (", "export ceiling")?;
    if setup
        .runner
        .join("velnor-gc1/transport/state/payload.tar.gz")
        .exists()
    {
        return Err(fail("over-ceiling root must never be packed"));
    }
    if setup
        .runner
        .join("velnor/state-publication.staged")
        .exists()
        || setup
            .runner
            .join("velnor/state-publication.record")
            .exists()
    {
        return Err(fail("over-ceiling root must leave no evidence"));
    }
    std::fs::remove_dir_all(&setup.scratch)
        .map_err(|error| fail(&format!("scratch stuck: {error}")))?;
    Ok(())
}
