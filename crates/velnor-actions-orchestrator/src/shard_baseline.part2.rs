/// Read an exact artifact and authenticate its original successful attempt.
fn download_authenticated(
    lookup: &BaselineLookup,
    artifact: &str,
    metadata: &BaselineArtifactMetadata,
    run_id: u64,
    run: &mut impl FnMut(Vec<OsString>) -> Result<String, String>,
    run_archive: &mut impl FnMut(Vec<OsString>) -> Result<Vec<u8>, String>,
) -> Result<BaselineManifest, String> {
    let temp = tempfile::tempdir().map_err(|_| "baseline_unavailable".to_owned())?;
    let archive_bytes = run_archive(lookup.artifact_zip_args(metadata.id)?)?;
    archive::stage_baseline_archive(temp.path(), artifact, metadata, &archive_bytes)?;
    let path = temp.path().join(artifact).join("baseline.json");
    let text = crate::retrieve_reports::read_staged_text(
        &path,
        crate::retrieve_reports::MAX_STAGED_REPORT_BYTES,
    )
    .map_err(|_| "baseline_unavailable".to_owned())?;
    let value = crate::internal_plan::snapshot::parse_canonical_json(&text)
        .map_err(|_| "baseline_unavailable".to_owned())?;
    let candidate: BaselineManifest =
        serde_json::from_value(value).map_err(|_| "baseline_unavailable".to_owned())?;
    let mut found = collect_manifests(
        temp.path(),
        &lookup.base_sha,
        run_id,
        candidate.run_attempt,
        artifact,
    )?;
    let parent = found
        .pop()
        .ok_or_else(|| "baseline_unavailable".to_owned())?;
    let endpoint = format!(
        "repos/{}/actions/runs/{run_id}/attempts/{}",
        lookup.repo, parent.run_attempt
    );
    let record = run(vec![OsString::from("api"), OsString::from(endpoint)])?;
    if !crate::retrieve_baseline::authentic_attempt(
        &record,
        &parent,
        &lookup.repo,
        &lookup.branch,
        &lookup.workflow,
    ) {
        return Err("baseline_unauthenticated".to_owned());
    }
    Ok(parent)
}

/// Repository slug scoping every lookup `gh` call.
///
/// Request-first with origin fallback; a conflict or an absence misses
/// before spawning anything, so no lookup ever queries a repo the
/// runner did not bless or the checkout cannot name.
fn resolve_lookup_repo(root: &Path, repository: Option<&str>) -> Result<String, String> {
    let origin = crate::cover_baseline::provenance_check::repository_slug_from_origin(root);
    let expected = crate::cover_baseline::provenance_resolve::resolve_expected_repository(
        origin.as_deref(),
        repository,
    );
    if expected.conflict {
        return Err("baseline_repo_conflict".to_owned());
    }
    expected
        .slug
        .ok_or_else(|| "baseline_repo_unresolved".to_owned())
}

/// Keep temp artifacts matching the exact base, run, attempt, id, shape.
///
/// The expected numeric ID derives from the artifact name, matching
/// the publisher-written fingerprint; the service listing ID never
/// binds manifest bytes.
fn collect_manifests(
    dir: &Path,
    base: &str,
    run_id: u64,
    attempt: u64,
    artifact: &str,
) -> Result<Vec<BaselineManifest>, String> {
    let expected = crate::cover_compat::baseline_artifact_numeric_id(artifact);
    let mut out = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|_| "baseline_unavailable".to_owned())?;
    for entry in entries {
        let entry = entry.map_err(|_| "baseline_unavailable".to_owned())?;
        if let Some(manifest) = crate::cover_baseline::baseline_entry_for(
            &entry.path(),
            base,
            run_id,
            attempt,
            expected,
        ) {
            out.push(manifest);
        }
    }
    if out.is_empty() {
        return Err("baseline_unavailable".to_owned());
    }
    out.sort_by(|left, right| left.artifact_name.cmp(&right.artifact_name));
    Ok(out)
}

#[cfg(test)]
#[path = "shard_baseline_retry_tests.rs"]
mod retry_tests;
#[cfg(test)]
#[path = "shard_baseline_tests.rs"]
mod tests;
