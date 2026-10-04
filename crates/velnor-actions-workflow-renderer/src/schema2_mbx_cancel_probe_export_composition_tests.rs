use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use crate::mbx_bundle;
use crate::schema2::mbx_cancel_probe::{MbxQualificationPins, Phase, victim_job};
use crate::yaml::Yaml;
use velnor_actions_contract::{PullRequestCachePolicy, StepKind};

#[test]
fn during_save_writer_reaches_save_without_an_unattached_sampler() -> Result<(), Box<dyn Error>> {
    let request = pins();
    let hosted = Yaml::str("ubuntu-26.04");
    let id = Phase::DuringSave.victim_id();
    let job = victim_job(&request, Phase::DuringSave, &hosted)?;
    let mut jobs = BTreeMap::from([(id.to_owned(), job)]);
    mbx_bundle::append_single_bundle_saves(&mut jobs, PullRequestCachePolicy::ReadOnly)?;
    let job = jobs.remove(id).ok_or("cancellation victim missing")?;
    let export_index = job
        .steps
        .iter()
        .position(|step| step.name == mbx_bundle::MBX_BUNDLE_EXPORT_NAME)
        .ok_or("cancellation export missing")?;
    let save_index = job
        .steps
        .iter()
        .position(|step| step.name == mbx_bundle::MBX_BUNDLE_SAVE_NAME)
        .ok_or("cancellation save missing")?;
    assert!(export_index < save_index);
    assert!(job.steps[save_index]
        .condition
        .as_deref()
        .is_some_and(|condition| condition.contains("steps.mbx-export.outputs.ready == 'true'")));
    let StepKind::Shell { run, env } = &job.steps[export_index].kind else {
        return Err("cancellation export is not a shell step".into());
    };
    assert_eq!(
        env.get(mbx_bundle::MBX_RESOURCE_EVIDENCE_REQUIRED_ENV)
            .map(String::as_str),
        Some("false")
    );
    let script = run.get(2).ok_or("cancellation export command missing")?;
    assert_eq!(run.get(0).map(String::as_str), Some("bash"));
    assert_eq!(run.get(1).map(String::as_str), Some("-c"));
    assert!(script.contains("snapshot_if_required export-complete"));

    let root = super::temp_dir("during-save-unmeasured-export")?;
    let bin = super::fake_bin(&root)?;
    executable(
        &bin.join("mbx"),
        "#!/bin/sh\ncase \"$1:$2\" in cache:export) mkdir -p \"$RUNNER_TEMP/mbx-single-bundle\"; exit 0 ;; gc:--max-size) exit 0 ;; *) exit 63 ;; esac\n",
    )?;
    executable(&bin.join("df"), "#!/bin/sh\nexit 0\n")?;
    let output = root.join("output");
    let github_env = root.join("github-env");
    fs::write(&output, "")?;
    fs::write(&github_env, "")?;
    let mut path = bin.as_os_str().to_os_string();
    path.push(":/usr/bin:/bin");
    let result = Command::new("/bin/bash")
        .args(["-c", script])
        .env_clear()
        .env("PATH", path)
        .env("RUNNER_TEMP", &root)
        .env("GITHUB_OUTPUT", &output)
        .env("GITHUB_ENV", &github_env)
        .envs(env)
        .env("MBX_CACHE_EXPORT_GROUP", "fixture")
        .output()?;
    assert!(
        result.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!root.join("mbx-cache-evidence/sampler.sh").exists());
    let output = fs::read_to_string(output)?;
    assert!(output.contains("export_status=0\n"), "{output}");
    assert!(output.contains("gc_status=0\n"), "{output}");
    assert!(output.contains("ready=true\n"), "{output}");
    Ok(())
}

fn pins() -> MbxQualificationPins {
    MbxQualificationPins {
        mise_setup: crate::setup::MiseSetup {
            uses: "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5".to_owned(),
            version: "2025.9.5".to_owned(),
            sha256: "d".repeat(64),
        },
        mbx_action_uses: "jdx/mr-boxington-action@1687e54eb349cadf61fa38b5813a77875489e8e6"
            .to_owned(),
        mbx_version: "1.22.0".to_owned(),
        rust_version: "1.98.1".to_owned(),
    }
}

fn executable(path: &std::path::Path, body: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, body)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}
