use super::*;

pub(super) struct PreparedArtifacts {
    pub(super) _temp: tempfile::TempDir,
    pub(super) plan: Plan,
    pub(super) entry: velnor_actions_contract::MatrixEntry,
    pub(super) directive: QualificationCacheDirective,
    pub(super) authenticated: AuthenticatedProducerContext,
    pub(super) source: QualificationSourceEvidenceDocument,
    pub(super) owner: QualificationCacheOwnerReceipt,
    pub(super) reports: PathBuf,
}

pub(super) fn prepared_artifacts() -> PreparedArtifacts {
    let (temp, plan, entry, obligation) = fixture();
    write_reports(&temp, &plan, &entry, 0);
    let (source, authenticated) = write_fixture_sidecar(&temp, &plan, &entry, &obligation);
    let run_dir = temp.path().join("velnor").join(&plan.run_key);
    let artifact = run_dir.join("reports").join(&entry.artifact_id);
    copy_lane_artifacts(&run_dir, &artifact, &entry, &plan);

    let directive = QualificationCacheDirective::for_plan(&plan, None)
        .expect("plan directive")
        .expect("qualification directive");
    let owner_directive = directive
        .owners
        .iter()
        .find(|owner| owner.owner_id == entry.job_id)
        .expect("plan owner directive");
    let owner = fixture_owner_receipt(&plan, &directive, owner_directive);
    write_owner_at(&artifact, &owner);
    PreparedArtifacts {
        _temp: temp,
        plan,
        entry,
        directive,
        authenticated,
        source,
        owner,
        reports: run_dir.join("reports"),
    }
}

fn copy_lane_artifacts(
    run_dir: &Path,
    artifact: &Path,
    entry: &velnor_actions_contract::MatrixEntry,
    plan: &Plan,
) {
    let lane_artifact = artifact.join(&entry.matrix_key);
    std::fs::create_dir_all(lane_artifact.join("tasks")).expect("artifact report directories");
    let task_id = task_report_id_for_task(&plan.run_key, &entry.matrix_key, &entry.task_digest)
        .expect("task report ID");
    for (source, destination) in [
        (
            run_dir
                .join(&entry.matrix_key)
                .join("tasks")
                .join(format!("{task_id}.json")),
            lane_artifact.join("tasks").join(format!("{task_id}.json")),
        ),
        (
            run_dir.join(&entry.matrix_key).join("matrix-report.json"),
            lane_artifact.join("matrix-report.json"),
        ),
        (
            run_dir
                .join(&entry.matrix_key)
                .join(SOURCE_EVIDENCE_FILENAME),
            lane_artifact.join(SOURCE_EVIDENCE_FILENAME),
        ),
    ] {
        std::fs::copy(source, destination).expect("copy plan-bound lane artifact");
    }
}

fn fixture_owner_receipt(
    plan: &Plan,
    directive: &QualificationCacheDirective,
    owner_directive: &velnor_actions_contract::workflow::QualificationCacheOwnerDirective,
) -> QualificationCacheOwnerReceipt {
    let layers = owner_directive
        .layers
        .iter()
        .map(|layer| fixture_layer_receipt(plan, directive, owner_directive, layer))
        .collect();
    QualificationCacheOwnerReceipt {
        owner_id: owner_directive.owner_id.clone(),
        platform_id: owner_directive.platform_id.clone(),
        member_digest: owner_directive.member_digest.clone(),
        members: owner_directive.members.clone(),
        layers,
    }
}

fn fixture_layer_receipt(
    plan: &Plan,
    directive: &QualificationCacheDirective,
    owner: &velnor_actions_contract::workflow::QualificationCacheOwnerDirective,
    layer: &velnor_actions_contract::workflow::QualificationCacheLayerDirective,
) -> QualificationCacheLayerReceipt {
    let mut receipt = QualificationCacheLayerReceipt {
        owner_id: owner.owner_id.clone(),
        platform_id: owner.platform_id.clone(),
        member_digest: owner.member_digest.clone(),
        layer: layer.layer,
        active: layer.active,
        identity_digest: layer.identity_digest.clone(),
        runtime_identity: None,
        state_digest: None,
        restore: disabled_restore(),
        save: disabled_save(),
    };
    if !layer.active {
        return receipt;
    }
    let requirements = layer.runtime.as_ref().expect("runtime requirements");
    let identity = fixture_runtime_identity(requirements);
    let bound = directive
        .bind_runtime_owner(plan, None, &owner.owner_id, layer.layer, &identity)
        .expect("valid runner identity binds cache layer");
    receipt.runtime_identity = Some(identity);
    receipt.state_digest = Some(digest_b3(receipt.identity_digest.as_bytes()));
    receipt.restore = fixture_restore(bound.restore.as_ref());
    receipt.save = fixture_save(bound.save_key.as_deref());
    receipt
}

fn fixture_restore(
    expectation: Option<&velnor_actions_contract::workflow::QualificationCacheRestoreExpectation>,
) -> QualificationCacheRestore {
    let Some(expectation) = expectation else {
        return disabled_restore();
    };
    let (result, matched_key, cache) = expectation.expected_cache.as_ref().map_or(
        (
            QualificationCacheRestoreResult::Miss,
            None,
            QualificationCacheBackendObservation::Absent,
        ),
        |entry| {
            (
                QualificationCacheRestoreResult::Hit,
                Some(expectation.requested_key.clone()),
                QualificationCacheBackendObservation::Found(entry.clone()),
            )
        },
    );
    QualificationCacheRestore {
        requested_key: Some(expectation.requested_key.clone()),
        matched_key,
        matched_cache: cache,
        result,
    }
}

fn fixture_save(key: Option<&str>) -> QualificationCacheSave {
    let Some(key) = key else {
        return disabled_save();
    };
    QualificationCacheSave {
        requested_key: Some(key.to_owned()),
        action: QualificationCacheSaveActionResult::Succeeded,
        before: QualificationCacheBackendObservation::Absent,
        after: QualificationCacheBackendObservation::Found(
            velnor_actions_contract::workflow::QualificationCacheBackendEntry {
                id: 1,
                key: key.to_owned(),
                git_ref: "refs/heads/main".to_owned(),
                version: "a".repeat(64),
                size_bytes: 1,
            },
        ),
    }
}

fn owner_path(prepared: &PreparedArtifacts) -> PathBuf {
    prepared
        .reports
        .join(&prepared.entry.artifact_id)
        .join(OWNER_EVIDENCE_DIRECTORY)
        .join(format!("{}.json", prepared.owner.owner_id))
}

pub(super) fn source_path(prepared: &PreparedArtifacts) -> PathBuf {
    prepared
        .reports
        .join(&prepared.entry.artifact_id)
        .join(&prepared.entry.matrix_key)
        .join(SOURCE_EVIDENCE_FILENAME)
}

pub(super) fn write_owner(prepared: &PreparedArtifacts, owner: &QualificationCacheOwnerReceipt) {
    write_canonical(&owner_path(prepared), owner);
}

fn write_owner_at(artifact: &Path, owner: &QualificationCacheOwnerReceipt) {
    let path = artifact
        .join(OWNER_EVIDENCE_DIRECTORY)
        .join(format!("{}.json", owner.owner_id));
    write_canonical(&path, owner);
}

pub(super) fn write_canonical<T: serde::Serialize>(path: &Path, value: &T) {
    std::fs::create_dir_all(path.parent().expect("file parent")).expect("artifact directory");
    std::fs::write(
        path,
        canonical_json_bytes(value).expect("canonical fixture JSON"),
    )
    .expect("fixture artifact");
}

fn disabled_restore() -> QualificationCacheRestore {
    QualificationCacheRestore {
        requested_key: None,
        matched_key: None,
        matched_cache: QualificationCacheBackendObservation::NotQueried,
        result: QualificationCacheRestoreResult::Disabled,
    }
}

fn disabled_save() -> QualificationCacheSave {
    QualificationCacheSave {
        requested_key: None,
        action: QualificationCacheSaveActionResult::Disabled,
        before: QualificationCacheBackendObservation::NotQueried,
        after: QualificationCacheBackendObservation::NotQueried,
    }
}

fn fixture_runtime_identity(
    requirements: &velnor_actions_contract::workflow::QualificationRuntimeIdentityRequirements,
) -> QualificationRuntimeIdentity {
    let tools = requirements
        .toolchain_inputs
        .tools
        .iter()
        .map(|selector| {
            let (_, version) = selector.rsplit_once('@').expect("pinned tool");
            QualificationRuntimeToolObservation {
                selector: selector.clone(),
                reported_version: version.to_owned(),
                executable_sha256: "a".repeat(64),
            }
        })
        .collect();
    let mut abi = vec![QualificationRuntimeAbiObservation::Host {
        os: "Linux".to_owned(),
        arch: "x86_64".to_owned(),
        kernel: "6.12.0".to_owned(),
        libc: Some("glibc 2.39".to_owned()),
    }];
    if let Some(version) = requirements
        .toolchain_inputs
        .tools
        .iter()
        .find_map(|selector| selector.strip_prefix("rust@"))
    {
        abi.push(QualificationRuntimeAbiObservation::Rust {
            host: "x86_64-unknown-linux-gnu".to_owned(),
            target: requirements.target.clone(),
            version_verbose: format!(
                "rustc {version} (test build)\nhost: x86_64-unknown-linux-gnu\nrelease: {version}"
            ),
            target_cfg: "target_arch=\"x86_64\"\ntarget_os=\"linux\"".to_owned(),
            installed_components: requirements.toolchain_inputs.components.clone(),
        });
    }
    QualificationRuntimeIdentity {
        platform: QualificationRuntimePlatform {
            os: "Linux".to_owned(),
            arch: "X64".to_owned(),
            runner_label: requirements.runner_label.clone(),
            runs_on: requirements.runs_on.clone(),
            image_identity: QualificationRuntimeImageIdentity::Hosted {
                image_os: "ubuntu26".to_owned(),
            },
            target: requirements.target.clone(),
        },
        tools,
        abi,
    }
}
