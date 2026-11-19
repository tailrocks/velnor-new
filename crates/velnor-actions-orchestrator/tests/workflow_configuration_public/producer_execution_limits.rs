// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::producer_execution_fixture::Fixture;
use serde_json::json;
use std::{error::Error, fs};

pub(super) fn event_payload_bounds(
    f: &Fixture,
    job: &str,
    task: &str,
    digest: &str,
) -> Result<(), Box<dyn Error>> {
    let request_path = f.temp.path().join("plan-v1-request.json");
    for (padding, accepted) in [(4 * 1024 * 1024, true), (8 * 1024 * 1024, false)] {
        let payload = serde_json::to_vec(
            &json!({"ref":"refs/heads/testmain", "after":f.head, "repository":{"default_branch":"testmain"}, "padding":"x".repeat(padding)}),
        )?;
        for mode in ["planner", "resolver", "writer"] {
            let mut command = f.command(
                job,
                task,
                digest,
                &f.head,
                (mode == "writer").then_some("0"),
            )?;
            if mode == "planner" {
                command
                    .env("VELNOR_PRODUCER_TEST_REQUEST", "1")
                    .env("VELNOR_REQUEST_FILE", &request_path);
            }
            fs::write(f.temp.path().join("push-event.json"), &payload)?;
            let output = command.output()?;
            assert_eq!(
                output.status.success(),
                accepted,
                "{mode} payload size {}: {}",
                payload.len(),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    Ok(())
}

pub(super) fn source_artifact_bounds(
    f: &Fixture,
    job: &str,
    task: &str,
    digest: &str,
) -> Result<(), Box<dyn Error>> {
    use velnor_actions_contract::strict_json::MAX_UNTRUSTED_DOCUMENT_BYTES;
    let mut descriptor = f.descriptor()?;
    let manifest_len = usize::try_from(
        fs::metadata(
            f.repo
                .path()
                .join(velnor_actions_contract::TASK_EXECUTION_MANIFEST_PATH),
        )?
        .len(),
    )?;
    let marked_len = |descriptor: &velnor_actions_contract::workflow::producer_execution_manifest::ProducerExecutionManifest| -> Result<usize, Box<dyn Error>> {
        let marker = velnor_actions_contract::workflow::task_execution_manifest_marker_line(&f.manifest.generator_version)?;
        Ok(marker.len() + 2 + velnor_actions_contract::canonical_json_bytes(descriptor)?.len())
    };
    let base = marked_len(&descriptor)?;
    descriptor.producer_tasks.insert(
        "state-producer-zz000000".to_owned(),
        [(task.to_owned(), digest.to_owned())].into_iter().collect(),
    );
    let per_job = marked_len(&descriptor)? - base;
    let wanted = MAX_UNTRUSTED_DOCUMENT_BYTES - manifest_len / 2;
    let count = (wanted - base) / per_job;
    for index in 1..=count {
        descriptor.producer_tasks.insert(
            format!("state-producer-zz{index:06}"),
            [(task.to_owned(), digest.to_owned())].into_iter().collect(),
        );
    }
    let length = marked_len(&descriptor)?;
    assert!(
        length <= MAX_UNTRUSTED_DOCUMENT_BYTES
            && length + manifest_len > MAX_UNTRUSTED_DOCUMENT_BYTES,
        "fixture does not cross the combined capture boundary"
    );
    descriptor.validate(&f.manifest)?;
    let head = commit_descriptor(f, &descriptor, "large producer descriptor")?;
    let mut plan = f.plan.clone();
    plan["head"] = json!(head);
    f.write_plan(&plan)?;
    for exit in [None, Some("0")] {
        let output = f.run(job, task, digest, &head, exit)?;
        assert!(
            output.status.success(),
            "valid large source artifacts rejected: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let extra = (MAX_UNTRUSTED_DOCUMENT_BYTES - length) / per_job + 2;
    for index in (count + 1)..=(count + extra) {
        descriptor.producer_tasks.insert(
            format!("state-producer-zz{index:06}"),
            [(task.to_owned(), digest.to_owned())].into_iter().collect(),
        );
    }
    descriptor.validate(&f.manifest)?;
    assert!(marked_len(&descriptor)? > MAX_UNTRUSTED_DOCUMENT_BYTES);
    let head = commit_descriptor(f, &descriptor, "oversized producer descriptor")?;
    plan["head"] = json!(head);
    f.write_plan(&plan)?;
    for exit in [None, Some("0")] {
        let output = f.run(job, task, digest, &head, exit)?;
        assert!(
            !output.status.success(),
            "oversized committed descriptor was accepted"
        );
    }
    Ok(())
}

fn commit_descriptor(
    f: &Fixture,
    descriptor: &velnor_actions_contract::workflow::producer_execution_manifest::ProducerExecutionManifest,
    message: &str,
) -> Result<String, Box<dyn Error>> {
    use super::producer_execution_fixture::git_head;
    use super::workflow_configuration_fixture::git;
    use velnor_actions_contract::workflow::producer_execution_manifest::PRODUCER_EXECUTION_MANIFEST_PATH;
    f.write_descriptor(descriptor)?;
    git(&["add", PRODUCER_EXECUTION_MANIFEST_PATH], f.repo.path())?;
    git(&["commit", "-qm", message], f.repo.path())?;
    git_head(f.repo.path())
}

pub(super) fn replace_ref_attack(
    f: &Fixture,
    job: &str,
    task: &str,
    digest: &str,
) -> Result<(), Box<dyn Error>> {
    use super::workflow_configuration_fixture::git;
    let other = f
        .jobs
        .keys()
        .find(|id| id.starts_with("state-producer-") && id.as_str() != job)
        .ok_or("other producer missing")?;
    let mut descriptor = f.descriptor()?;
    descriptor
        .producer_tasks
        .entry(other.clone())
        .or_default()
        .insert(task.to_owned(), digest.to_owned());
    let replacement = commit_descriptor(f, &descriptor, "forged producer descriptor")?;
    git(&["replace", &f.head, &replacement], f.repo.path())?;
    f.write_plan(&f.plan)?;
    for exit in [None, Some("0")] {
        let legitimate = f.run(job, task, digest, &f.head, exit)?;
        assert!(
            legitimate.status.success(),
            "legitimate producer rejected under replace ref: {}",
            String::from_utf8_lossy(&legitimate.stderr)
        );
        let forged = f.run(other, task, digest, &f.head, exit)?;
        assert!(
            !forged.status.success(),
            "replace ref authorized a task in another producer"
        );
        assert!(String::from_utf8_lossy(&forged.stderr).contains("producer_task_not_in_source"));
    }
    git(&["replace", "-d", &f.head], f.repo.path())?;
    Ok(())
}
