// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::error::Error;
use std::os::unix::fs::PermissionsExt as _;

#[test]
fn generated_start_and_coordinate_rehearsal() -> Result<(), Box<dyn Error>> {
    if let Some(operation) = std::env::var_os("VELNOR_COORDINATOR_CHILD") {
        return run_child(&operation.to_string_lossy());
    }
    for event in ["push", "workflow_dispatch"] {
        rehearse(event, false)?;
        rehearse(event, true)?;
    }
    Ok(())
}

fn rehearse(event: &str, interrupted: bool) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let source = temp.path().join("source");
    fs::create_dir(&source)?;
    let (mut intent, mut request, _, _) = make_fixture(&source)?;
    request.run_id = 11;
    if event == "push" {
        intent.recovery = None;
        request.event = "push".into();
        request.workflow_sha.clone_from(&request.source_sha);
        request.recovery_sha = None;
    }
    fs::write(
        temp.path().join("request.json"),
        serde_json::to_vec(&(intent, request))?,
    )?;
    let runner = temp.path().join("runner");
    let bin = runner.join("velnor/bin");
    fs::create_dir_all(&bin)?;
    let executable = bin.join(format!("velnor-actions-{}", env!("CARGO_PKG_VERSION")));
    fs::write(
        &executable,
        "#!/bin/sh\nexport VELNOR_COORDINATOR_CHILD=\"$2\"\nexec \"$VELNOR_TEST_BINARY\" --exact crate_release_controller::tests::coordinator_rehearsal::generated_start_and_coordinate_rehearsal\n",
    )?;
    fs::set_permissions(executable, fs::Permissions::from_mode(0o755))?;
    let steps = rendered_scripts()?;
    assert_eq!(
        steps
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["Release journal-start", "Release journal-coordinate"]
    );
    for (step, script) in &steps {
        let crash = interrupted && step == "Release journal-coordinate";
        let status = run_script(script, &runner, temp.path(), crash)?;
        if crash {
            assert!(!status.success(), "{event}: simulated interrupted upload");
            continue;
        }
        assert!(status.success(), "{event}: {step}");
    }
    let script = &steps.last().ok_or("coordinate step")?.1;
    assert!(run_script(script, &runner, temp.path(), false)?.success());
    if interrupted {
        assert!(run_script(script, &runner, temp.path(), false)?.success());
    }
    let journal = fs::read(temp.path().join("journal.json"))?;
    let published = temp.path().join("published.crate");
    let mut archive = fs::read(&published)?;
    archive.push(0);
    fs::write(published, archive)?;
    assert!(!run_script(script, &runner, temp.path(), false)?.success());
    assert_eq!(fs::read(temp.path().join("journal.json"))?, journal);
    Ok(())
}

fn run_script(
    script: &str,
    runner: &Path,
    root: &Path,
    interrupted: bool,
) -> Result<std::process::ExitStatus, Box<dyn Error>> {
    Ok(Command::new("bash")
        .args(["-e", "-c", script])
        .env("RUNNER_TEMP", runner)
        .env("VELNOR_TEST_BINARY", std::env::current_exe()?)
        .env("VELNOR_REHEARSAL_ROOT", root)
        .env(
            "VELNOR_REHEARSAL_INTERRUPTED",
            if interrupted { "1" } else { "0" },
        )
        .status()?)
}

fn rendered_scripts() -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let (spec, rust) = crate::release_steps::tests::fixture()?;
    let workflow = velnor_actions_workflow_renderer::release_tree::render_release_workflow(
        &spec,
        &velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext {
            generator_version: env!("CARGO_PKG_VERSION").into(),
            runs_on: "ubuntu-24.04".into(),
        },
        &rust,
    )?;
    let workflow: serde_json::Value = serde_saphyr::from_str(&workflow)?;
    let steps = workflow["jobs"]["release-publish"]["steps"]
        .as_array()
        .ok_or("publisher steps")?;
    steps
        .iter()
        .filter_map(|step| {
            let name = step["name"].as_str()?;
            matches!(name, "Release journal-start" | "Release journal-coordinate").then(|| {
                step["run"]
                    .as_str()
                    .map(|script| (name.to_owned(), script.to_owned()))
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| "missing step script".into())
}

fn run_child(operation: &str) -> Result<(), Box<dyn Error>> {
    let root = std::path::PathBuf::from(std::env::var_os("VELNOR_REHEARSAL_ROOT").ok_or("root")?);
    let source = root.join("source");
    let (intent, request): (CrateReleaseIntent, GitHubAdmissionRequest) =
        serde_json::from_slice(&fs::read(root.join("request.json"))?)?;
    let published = root.join("published.crate");
    let previously_published = published.exists();
    let metadata_completed = root.join("metadata-complete").exists();
    let base = Github {
        sha: request.source_sha.clone(),
        policy_sha: request.workflow_sha.clone(),
        approved: Cell::new(true),
        tag_exists: Cell::new(metadata_completed),
        tag_wrong: Cell::new(false),
        owner_match: Cell::new(true),
        release_variant: Cell::new(if metadata_completed { 0 } else { 4 }),
        write_error: Cell::new(0),
        writes: Cell::new(0),
        tool_archive: RefCell::new(Vec::new()),
        mutate_after_download: Cell::new(0),
        artifact_receipt: Cell::new(false),
        before_metadata_write: RefCell::new(None),
    };
    let github = HistoricalAuthority {
        base: &base,
        workflow: request.workflow_sha.clone(),
        older: None,
        event: if request.event == "push" {
            "push"
        } else {
            "workflow_dispatch"
        },
        defect: Cell::new(0),
    };
    let registry = Registry {
        absent: Cell::new(!previously_published),
        inconsistent: Cell::new(false),
        archive: RefCell::new(if previously_published {
            fs::read(&published)?
        } else {
            Vec::new()
        }),
    };
    let admission = if operation == "journal-start" {
        prepare_crate_release(&intent, &request, &github, &registry, &source)?
    } else {
        serde_json::from_slice(&fs::read(root.join("admission.json"))?)?
    };
    let journal = JournalRegistryFake::default();
    if operation == "journal-start" {
        use crate::package_release_signer::{
            AdmittedStart, fixture_attested_generator, record_admitted_start,
        };
        let generator = fixture_attested_generator();
        record_admitted_start(&AdmittedStart {
            journal: &journal,
            github: &github,
            registry: &registry,
            intent: &intent,
            request: &request,
            admission: &admission,
            source: &source,
            workflow_root: &source,
            generator: &generator,
        })?;
        fs::write(root.join("admission.json"), serde_json::to_vec(&admission)?)?;
        save_journal(&root, &journal)?;
        return Ok(());
    }
    if operation != "journal-coordinate" {
        return Err("unexpected operation".into());
    }
    load_journal(&root, &journal)?;
    coordinate(
        &source, &intent, &request, &github, &registry, &admission, &journal,
    )?;
    fs::write(published, &*registry.archive.borrow())?;
    fs::write(root.join("metadata-complete"), b"complete")?;
    save_journal(&root, &journal)
}

fn coordinate(
    source: &Path,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &HistoricalAuthority<'_>,
    registry: &Registry,
    admission: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    journal: &JournalRegistryFake,
) -> Result<(), Box<dyn Error>> {
    use crate::crate_release_controller::{
        CrateReleaseObservation, UploadAttempt, coordinate_release,
    };
    use crate::crate_release_journal::{JournalIdentity, JournalStage, inspect_journal};
    let root = source.parent().ok_or("source parent")?;
    let identity = JournalIdentity {
        repository: request.repository.clone(),
        workflow_sha: request.workflow_sha.clone(),
        source_sha: request.source_sha.clone(),
        run_id: request.run_id,
        run_attempt: request.run_attempt,
    };
    let started =
        inspect_journal(journal, &identity, JournalStage::Started)?.ok_or("missing start")?;
    let already_completed = inspect_journal(journal, &identity, JournalStage::Completed)?.is_some();
    let pending = !already_completed && journal.refs.borrow().len() > 1;
    let input = CrateReleaseObservation {
        journal,
        identity: &identity,
        journal_receipt: &started.receipt,
        admission,
        intent,
        request,
        github,
        registry,
        source,
        sequence: 1,
    };
    let archive = fixture_archive(source)?;
    let uploads = Cell::new(0);
    let result = coordinate_release(
        &input,
        |name| crate::crate_release_semver::fixture_verified_semver(admission, name),
        |_| {
            uploads.set(uploads.get() + 1);
            *registry.archive.borrow_mut() = archive.clone();
            registry.absent.set(false);
            if std::env::var("VELNOR_REHEARSAL_INTERRUPTED").as_deref() == Ok("1") {
                save_journal(root, journal).map_err(|_| err())?;
                fs::write(root.join("published.crate"), &archive).map_err(|_| err())?;
                return Err(err());
            }
            Ok(UploadAttempt::Uncertain)
        },
        || {},
    )?;
    assert_eq!(uploads.get(), u32::from(!already_completed && !pending));
    assert_eq!(result.history.len(), 6);
    assert_eq!(
        github.base.writes.get(),
        if already_completed { 0 } else { 2 }
    );
    let tools = crate::package_release_signer::fixture_measured_tools(
        started.receipt.expected_tools.clone(),
    );
    crate::crate_release_journal::record_reconciled_completion(&input, tools)?;
    assert!(inspect_journal(journal, &identity, JournalStage::Completed)?.is_some());
    let prior = journal.write_calls.get();
    coordinate_release(&input, |_| Err(err()), |_| Err(err()), || {})?;
    assert_eq!(journal.write_calls.get(), prior);
    assert_eq!(uploads.get(), u32::from(!already_completed && !pending));
    Ok(())
}

fn save_journal(root: &Path, journal: &JournalRegistryFake) -> Result<(), Box<dyn Error>> {
    let tags: BTreeMap<_, _> = journal.tags.borrow().iter().map(|(sha, tag)| (sha.clone(), json!({"name":tag.name,"object_type":tag.object_type,"object_sha":tag.object_sha,"message":tag.message}))).collect();
    fs::write(
        root.join("journal.json"),
        serde_json::to_vec(
            &json!({"refs":*journal.refs.borrow(),"blobs":*journal.blobs.borrow(),"tags":tags}),
        )?,
    )?;
    Ok(())
}

fn load_journal(root: &Path, journal: &JournalRegistryFake) -> Result<(), Box<dyn Error>> {
    let value: Value = serde_json::from_slice(&fs::read(root.join("journal.json"))?)?;
    *journal.refs.borrow_mut() = serde_json::from_value(value["refs"].clone())?;
    *journal.blobs.borrow_mut() = serde_json::from_value(value["blobs"].clone())?;
    for (sha, value) in value["tags"].as_object().ok_or("tags")? {
        journal.tags.borrow_mut().insert(
            sha.clone(),
            crate::crate_release_journal::JournalTag {
                name: value["name"].as_str().ok_or("tag name")?.into(),
                object_type: value["object_type"].as_str().ok_or("type")?.into(),
                object_sha: value["object_sha"].as_str().ok_or("sha")?.into(),
                message: value["message"].as_str().ok_or("message")?.into(),
            },
        );
    }
    Ok(())
}
