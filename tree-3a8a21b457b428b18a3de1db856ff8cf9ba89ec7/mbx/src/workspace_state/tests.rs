use self::capture_fixture as capture;
use super::*;

pub(super) fn capture_fixture(store: &Path, targets: &[WorkspaceRoots]) -> Result<ExportAdditions> {
    let evidence = persisted_evidence(store, &fixture_evidence(targets))?;
    match super::capture(
        &Config::for_test(store),
        store,
        targets,
        &store.join("out-dirs"),
        &evidence,
    )? {
        CaptureOutcome::Captured(additions) | CaptureOutcome::RetainedOwner { additions, .. } => {
            Ok(additions)
        }
        CaptureOutcome::UnavailableManagedOverlap
        | CaptureOutcome::UnavailableOwnerProof { .. } => {
            bail!("unsupported fixture placement")
        }
    }
}

pub(super) fn fixture_evidence(
    targets: &[WorkspaceRoots],
) -> Vec<mbx_cache_store::ReceiptEvidence> {
    targets
        .iter()
        .map(|workspace| mbx_cache_store::ReceiptEvidence {
            lineage: None,
            workspace: workspace.clone(),
            identity: "a".repeat(64),
            context: None,
            predictions: vec![],
        })
        .collect()
}

pub(super) fn persist_evidence(
    store: &Path,
    evidence: &[mbx_cache_store::ReceiptEvidence],
) -> Result<()> {
    for receipt in evidence {
        mbx_cache_store::record_build_receipt(
            store,
            &"b".repeat(64),
            &receipt.identity,
            &receipt.workspace.workspace_root,
            Some(&receipt.workspace.cargo),
            None,
            receipt.predictions.clone(),
            receipt.context.as_ref(),
            receipt.lineage.as_ref(),
        )?;
    }
    Ok(())
}

pub(super) fn persisted_evidence(
    store: &Path,
    evidence: &[mbx_cache_store::ReceiptEvidence],
) -> Result<Vec<mbx_cache_store::ReceiptEvidence>> {
    persist_evidence(store, evidence)?;
    let stored = mbx_cache_store::stored_receipt_evidence(store)?;
    evidence
        .iter()
        .map(|requested| {
            stored
                .iter()
                .find(|record| *record == requested)
                .cloned()
                .ok_or_else(|| eyre::eyre!("native fixture receipt was not persisted exactly"))
        })
        .collect()
}

use std::fs;

struct Fixture {
    target: WorkspaceRoots,
    inline: PathBuf,
}

fn fixture(root: &Path, name: &str, reference: &[u8]) -> Result<Fixture> {
    let workspace_root = root.join(name);
    let target_dir = workspace_root.join("target");
    fs::create_dir_all(target_dir.join("nested/deps"))?;
    fs::write(
        workspace_root.join("Cargo.toml"),
        b"[package]\nname = \"fixture\"\n",
    )?;
    let inline = target_dir.join("nested/inline.txt");
    fs::write(&inline, b"inline contents")?;
    fs::write(target_dir.join("reference.bin"), reference)?;
    let reference_digest = CacheDigest::blake3(reference);
    LocalCas::new(root).store_bytes(&reference_digest, reference)?;
    let executable = std::env::current_exe()?;
    fs::copy(executable, target_dir.join("mbx-placeholder"))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("nested/inline.txt", target_dir.join("link"))?;
    Ok(Fixture {
        target: WorkspaceRoots {
            workspace_root,
            cargo: CargoBuildRoots {
                build_dir: target_dir.clone(),
                target_dir,
            },
        },
        inline,
    })
}

fn attachment(additions: &ExportAdditions) -> Result<CacheDigest> {
    additions
        .attachments
        .get(ATTACHMENT)
        .cloned()
        .ok_or_else(|| eyre::eyre!("fixture did not produce a workspace attachment"))
}

fn find_entry<'a>(
    inventory: &'a BTreeMap<String, serde_json::Value>,
    suffix: &str,
) -> Result<&'a serde_json::Value> {
    inventory
        .iter()
        .find_map(|(key, value)| key.ends_with(suffix).then_some(value))
        .ok_or_else(|| eyre::eyre!("missing semantic inventory entry {suffix}"))
}

#[test]
fn semantic_inventory_preserves_content_shape_and_ignores_root_and_time() -> Result<()> {
    let store = tempfile::tempdir()?;
    let fixture = fixture(store.path(), "workspace", b"reference contents")?;
    let first = capture(store.path(), std::slice::from_ref(&fixture.target))?;
    let first_inventory = semantic_inventory(store.path(), Some(&attachment(&first)?))?;
    let first_json = serde_json::to_string(&first_inventory)?;
    assert!(!first_json.contains(&fixture.target.workspace_root.display().to_string()));
    assert!(!first_json.contains("modified_secs"));

    let inline = first_inventory
        .iter()
        .find(|(key, _)| key.ends_with("/nested/inline.txt"))
        .map(|(_, value)| value)
        .ok_or_else(|| eyre::eyre!("inline file is missing"))?;
    assert_eq!(inline["type"], "file");
    assert_eq!(inline["content"]["kind"], "digest");
    assert_eq!(inline["content"]["digest"]["algorithm"], "blake3");
    assert!(inline["mode"].is_u64());
    assert_eq!(
        find_entry(&first_inventory, "/nested")?["type"],
        "directory"
    );
    let reference = find_entry(&first_inventory, "/reference.bin")?;
    assert_eq!(reference["content"]["kind"], "digest");
    let mbx = find_entry(&first_inventory, "/mbx-placeholder")?;
    assert_eq!(mbx["content"]["kind"], "mbx");
    assert!(mbx["content"].get("digest").is_none());
    let marker = first_inventory
        .values()
        .find(|value| value["type"] == "workspace")
        .ok_or_else(|| eyre::eyre!("workspace marker is missing"))?;
    assert_eq!(marker["content"]["kind"], "digest");

    filetime::set_file_mtime(&fixture.inline, filetime::FileTime::from_unix_time(1, 2))?;
    let second = capture(store.path(), std::slice::from_ref(&fixture.target))?;
    let second_inventory = semantic_inventory(store.path(), Some(&attachment(&second)?))?;
    assert_eq!(first_inventory, second_inventory);
    #[cfg(unix)]
    assert_eq!(
        find_entry(&first_inventory, "/link")?["target"],
        "nested/inline.txt"
    );
    Ok(())
}

#[test]
fn semantic_workspace_identity_is_stable_when_same_signature_sibling_is_omitted() -> Result<()> {
    let store = tempfile::tempdir()?;
    let first = fixture(store.path(), "first", b"reference contents")?;
    let second = fixture(store.path(), "second", b"reference contents")?;
    fs::write(&second.inline, b"second workspace contents")?;
    let both = capture(store.path(), &[first.target.clone(), second.target.clone()])?;
    let only_second = capture(store.path(), std::slice::from_ref(&second.target))?;
    let all = semantic_inventory(store.path(), Some(&attachment(&both)?))?;
    let subset = semantic_inventory(store.path(), Some(&attachment(&only_second)?))?;
    for (key, value) in subset {
        assert_eq!(all.get(&key), Some(&value), "subset entry moved: {key}");
    }
    Ok(())
}
#[test]
fn semantic_inventory_excludes_only_root_compiler_probe_cache() -> Result<()> {
    let store = tempfile::tempdir()?;
    let fixture = fixture(store.path(), "workspace", b"reference contents")?;
    let probe = fixture.target.cargo.target_dir.join(".rustc_info.json");
    std::fs::write(&probe, br#"{"rustc_fingerprint":1}"#)?;
    let first = capture(store.path(), std::slice::from_ref(&fixture.target))?;
    let before = semantic_inventory(store.path(), Some(&attachment(&first)?))?;
    std::fs::write(&probe, br#"{"rustc_fingerprint":2}"#)?;
    let second = capture(store.path(), std::slice::from_ref(&fixture.target))?;
    let after = semantic_inventory(store.path(), Some(&attachment(&second)?))?;
    assert_eq!(before, after);
    let cas = LocalCas::new(store.path());
    let bundle: Bundle =
        serde_json::from_slice(&std::fs::read(cas.path_for(&attachment(&second)?)?)?)?;
    assert!(
        bundle.workspaces[0].trees[0]
            .inline_files
            .iter()
            .any(|entry| entry.path == Path::new(".rustc_info.json"))
    );
    std::fs::write(
        fixture
            .target
            .cargo
            .target_dir
            .join("nested/.rustc_info.json"),
        b"actual nested content",
    )?;
    let nested = capture(store.path(), std::slice::from_ref(&fixture.target))?;
    assert_ne!(
        after,
        semantic_inventory(store.path(), Some(&attachment(&nested)?))?
    );
    Ok(())
}
#[test]
fn retained_workspace_union_preserves_unbound_current_and_omitted_roots() -> Result<()> {
    let store = tempfile::tempdir()?;
    let first = fixture(store.path(), "first", b"first output")?;
    let second = fixture(store.path(), "second", b"second output")?;
    let baseline = capture(store.path(), &[first.target.clone(), second.target.clone()])?;
    let baseline_inventory = semantic_inventory(store.path(), Some(&attachment(&baseline)?))?;
    std::fs::write(&first.inline, b"updated current state")?;
    let current = capture(store.path(), std::slice::from_ref(&first.target))?;
    let outcome = retain(store.path(), current, Some(&attachment(&baseline)?))?;
    assert!(!outcome.unavailable_reasons.is_empty());
    let merged = outcome.additions;
    let cas = LocalCas::new(store.path());
    let bundle: Bundle =
        serde_json::from_slice(&std::fs::read(cas.path_for(&attachment(&merged)?)?)?)?;
    assert_eq!(bundle.workspaces.len(), 2);
    assert_eq!(
        bundle
            .workspaces
            .iter()
            .filter(|state| state.workspace_root == first.target.workspace_root)
            .count(),
        1
    );
    let inventory = semantic_inventory(store.path(), Some(&attachment(&merged)?))?;
    let second_only = capture(store.path(), std::slice::from_ref(&second.target))?;
    let omitted = semantic_inventory(store.path(), Some(&attachment(&second_only)?))?;
    assert!(
        omitted
            .iter()
            .all(|(key, value)| inventory.get(key) == Some(value))
    );
    assert_eq!(baseline_inventory, inventory);
    let unchanged = retain(
        store.path(),
        ExportAdditions::default(),
        Some(&attachment(&merged)?),
    )?
    .additions;
    assert_eq!(
        inventory,
        semantic_inventory(store.path(), Some(&attachment(&unchanged)?))?
    );
    let retained_inline = bundle.workspaces[0].trees[0].inline_archive.clone();
    std::fs::write(cas.path_for(&retained_inline)?, b"tampered")?;
    assert!(semantic_inventory(store.path(), Some(&attachment(&unchanged)?)).is_err());
    Ok(())
}

#[test]
fn semantic_inventory_validation_rejects_unknown_fields_and_unsafe_links() -> Result<()> {
    let store = tempfile::tempdir()?;
    let fixture = fixture(store.path(), "workspace", b"reference contents")?;
    let additions = capture(store.path(), std::slice::from_ref(&fixture.target))?;
    let inventory = semantic_inventory(store.path(), Some(&attachment(&additions)?))?;
    validate_semantic_inventory(&inventory)?;

    let marker_key = inventory
        .iter()
        .find(|(_, value)| value["type"] == "workspace")
        .map(|(key, _)| key.clone())
        .ok_or_else(|| eyre::eyre!("workspace marker is missing"))?;
    let mut unknown = inventory.clone();
    unknown
        .get_mut(&marker_key)
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| eyre::eyre!("workspace marker is not an object"))?
        .insert("unexpected".to_owned(), serde_json::json!(true));
    assert!(validate_semantic_inventory(&unknown).is_err());

    #[cfg(unix)]
    {
        let link_key = inventory
            .keys()
            .find(|key| key.ends_with("/link"))
            .cloned()
            .ok_or_else(|| eyre::eyre!("symlink is missing"))?;
        let mut unsafe_link = inventory;
        unsafe_link
            .get_mut(&link_key)
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| eyre::eyre!("symlink is not an object"))?
            .insert("target".to_owned(), serde_json::json!("../../outside"));
        assert!(validate_semantic_inventory(&unsafe_link).is_err());
    }
    Ok(())
}
