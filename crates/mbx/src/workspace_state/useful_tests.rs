use super::*;

fn state(cas: &LocalCas, files: &[(&str, &[u8])]) -> Result<WorkspaceState> {
    let mut archive = tar::Builder::new(Vec::new());
    let mut metadata = Vec::new();
    for (path, bytes) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(123);
        header.set_cksum();
        archive.append_data(&mut header, path, *bytes)?;
        metadata.push(FileMetadata {
            path: PathBuf::from(path),
            mode: 0o644,
            modified_secs: 123,
            modified_nanos: 456,
        });
    }
    let bytes = archive.into_inner()?;
    let digest = CacheDigest::blake3(&bytes);
    cas.store_bytes(&digest, &bytes)?;
    serde_json::from_value(serde_json::json!({
        "owner": CacheDigest::blake3(b"original-owner"),
        "workspace_root": "/original/workspace",
        "cargo_roots": {
            "target_dir": "/original/workspace/target",
            "build_dir": "/original/workspace/target"
        },
        "signature": CacheDigest::blake3(b"original-signature"),
        "trees": [{"role":"target", "inline_archive":digest,
            "inline_files":metadata, "references":[], "symlinks":[]}],
        "owned_out_dirs":[]
    }))
    .map_err(Into::into)
}

fn copy(state: &WorkspaceState) -> Result<WorkspaceState> {
    serde_json::from_value(serde_json::to_value(state)?).map_err(Into::into)
}

fn unavailable() -> ReplacementCoverage {
    ReplacementCoverage::Unavailable(CoverageUnavailableReason::SchedulerValidityNotProven)
}

#[test]
fn identical_payload_survives_owner_authorized_root_relocation() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let cas = LocalCas::new(temporary.path());
    let before = state(&cas, &[("unit/output", b"compiled")])?;
    let mut after = copy(&before)?;
    after.workspace_root = PathBuf::from("/relocated/workspace");
    after.cargo_roots.target_dir = PathBuf::from("/relocated/workspace/target");
    after.cargo_roots.build_dir = after.cargo_roots.target_dir.clone();
    assert_eq!(
        replacement_coverage(&cas, &before, &after)?,
        ReplacementCoverage::Identical
    );
    Ok(())
}

#[test]
fn every_native_payload_field_keeps_changes_unavailable() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let cas = LocalCas::new(temporary.path());
    let before = state(&cas, &[("unit/output", b"compiled")])?;
    let mut after = copy(&before)?;
    after.trees[0].inline_files[0].modified_nanos += 1;
    assert_eq!(replacement_coverage(&cas, &before, &after)?, unavailable());
    after = copy(&before)?;
    after.trees[0].inline_files[0].mode = 0o755;
    assert_eq!(replacement_coverage(&cas, &before, &after)?, unavailable());
    after = copy(&before)?;
    after.signature = CacheDigest::blake3(b"different-signature");
    assert_eq!(replacement_coverage(&cas, &before, &after)?, unavailable());
    after = copy(&before)?;
    after.trees[0].symlinks.push(Symlink {
        path: PathBuf::from("unit/alias"),
        target: PathBuf::from("output"),
        directory: false,
    });
    assert_eq!(replacement_coverage(&cas, &before, &after)?, unavailable());
    Ok(())
}

#[test]
fn opaque_scheduler_query_and_pruning_changes_never_mint_coverage() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let cas = LocalCas::new(temporary.path());
    for path in [".fingerprint/unit/state", ".rustc_info.json", "unit/output"] {
        let before = state(&cas, &[(path, b"original")])?;
        let changed = state(&cas, &[(path, b"changed")])?;
        let pruned = state(&cas, &[])?;
        assert_eq!(
            replacement_coverage(&cas, &before, &changed)?,
            unavailable()
        );
        assert_eq!(replacement_coverage(&cas, &before, &pruned)?, unavailable());
    }
    Ok(())
}

#[test]
fn missing_or_corrupt_closure_is_typed_unavailable_even_when_payloads_match() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let cas = LocalCas::new(temporary.path());
    let before = state(&cas, &[("unit/output", b"compiled")])?;
    let after = copy(&before)?;
    let archive = cas.path_for(&before.trees[0].inline_archive)?;
    std::fs::remove_file(&archive)?;
    let expected =
        ReplacementCoverage::Unavailable(CoverageUnavailableReason::PayloadClosureUnavailable);
    assert_eq!(replacement_coverage(&cas, &before, &after)?, expected);
    std::fs::write(&archive, b"corrupt")?;
    assert_eq!(replacement_coverage(&cas, &before, &after)?, expected);
    Ok(())
}

#[test]
fn matching_payload_never_changes_owner_identity() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let cas = LocalCas::new(temporary.path());
    let before = state(&cas, &[("unit/output", b"compiled")])?;
    let mut after = copy(&before)?;
    after.owner = CacheDigest::blake3(b"foreign-owner");
    assert_eq!(
        replacement_coverage(&cas, &before, &after)?,
        ReplacementCoverage::Unavailable(CoverageUnavailableReason::OwnerIdentityChanged)
    );
    Ok(())
}

#[test]
fn executable_placeholder_equality_cannot_prove_original_binary_identity() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let cas = LocalCas::new(temporary.path());
    let mut before = state(&cas, &[])?;
    before.trees[0].references.push(FileReference {
        path: PathBuf::from("unit/mbx"),
        source: FileSource::Mbx,
        mode: 0o755,
        modified_secs: 123,
        modified_nanos: 456,
    });
    let after = copy(&before)?;
    assert_eq!(
        replacement_coverage(&cas, &before, &after)?,
        ReplacementCoverage::Unavailable(CoverageUnavailableReason::ExecutablePlaceholderNotProven)
    );
    Ok(())
}

#[test]
fn referenced_output_closure_is_required() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let cas = LocalCas::new(temporary.path());
    let mut before = state(&cas, &[])?;
    let digest = CacheDigest::blake3(b"mandatory-reference");
    before.trees[0].references.push(FileReference {
        path: PathBuf::from("unit/output"),
        source: FileSource::Cas(digest.clone()),
        mode: 0o644,
        modified_secs: 123,
        modified_nanos: 456,
    });
    let after = copy(&before)?;
    assert_eq!(
        replacement_coverage(&cas, &before, &after)?,
        ReplacementCoverage::Unavailable(CoverageUnavailableReason::PayloadClosureUnavailable)
    );
    cas.store_bytes(&digest, b"mandatory-reference")?;
    assert_eq!(
        replacement_coverage(&cas, &before, &after)?,
        ReplacementCoverage::Identical
    );
    Ok(())
}

#[test]
fn generated_input_closure_timestamps_roots_and_original_proofs_remain_required() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let cas = LocalCas::new(temporary.path());
    let mut before = state(&cas, &[])?;
    let digest = CacheDigest::blake3(b"generated input");
    let path = "generated.rs";
    let manifest = format!("f {} {}:{path}\n", digest.hash, path.len());
    let snapshot = serde_json::from_value(serde_json::json!({
        "source":"unit/out", "root":"/original/out-dirs",
        "digest":CacheDigest::blake3(manifest.as_bytes()), "directories":[],
        "files":[{"path":path,"digest":digest,"executable":false,
            "modified_secs":123,"modified_nanos":456}],
        "proofs":[{"identity":"a".repeat(64),
            "invocation":CacheDigest::blake3(b"invocation"),
            "action":CacheDigest::blake3(b"action"),
            "context":{"schema":1,"source":{"fixture":"source"},
                "tool":{"fixture":"tool"}}}]
    }))?;
    before.owned_out_dirs.push(snapshot);
    let mut after = copy(&before)?;
    assert_eq!(
        replacement_coverage(&cas, &before, &after)?,
        ReplacementCoverage::Unavailable(CoverageUnavailableReason::PayloadClosureUnavailable)
    );
    cas.store_bytes(&digest, b"generated input")?;
    assert_eq!(
        replacement_coverage(&cas, &before, &after)?,
        ReplacementCoverage::Identical
    );
    after.owned_out_dirs[0].files[0].modified_nanos += 1;
    assert_eq!(replacement_coverage(&cas, &before, &after)?, unavailable());
    after = copy(&before)?;
    after.owned_out_dirs[0].root = PathBuf::from("/different/out-dirs");
    assert_eq!(replacement_coverage(&cas, &before, &after)?, unavailable());
    after = copy(&before)?;
    after.owned_out_dirs[0].proofs[0].context.source = serde_json::json!({"fixture":"changed"});
    assert_eq!(replacement_coverage(&cas, &before, &after)?, unavailable());
    Ok(())
}
