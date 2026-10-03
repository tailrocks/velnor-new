use super::*;

fn recorded() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    record_build_receipt(
        directory.path(),
        &"b".repeat(64),
        &"a".repeat(64),
        &workspace,
        Some(&CargoBuildRoots {
            target_dir: workspace.join("target"),
            build_dir: workspace.join("build"),
        }),
        Some("group"),
        vec![],
        Some(&ReceiptContext {
            schema: 1,
            source: serde_json::json!({"origin":"source"}),
            tool: serde_json::json!({"sdk":"tool"}),
        }),
        None,
    )
    .unwrap();
    directory
}

#[test]
fn nested_group_and_checkout_aliases_never_select_or_root() {
    let directory = recorded();
    let store = directory.path();
    let group = group_receipt_path(store, "group", &"b".repeat(64));
    let alias = group
        .parent()
        .unwrap()
        .join("nested")
        .join(group_key("group"))
        .join(group.file_name().unwrap());
    std::fs::create_dir_all(alias.parent().unwrap()).unwrap();
    std::fs::rename(group, &alias).unwrap();
    assert!(read_build_receipt(store, &alias).is_none());
    assert!(group_receipt_evidence(store, "group").unwrap().is_empty());
    let latest = latest_receipt_path(store, &store.join("workspace"));
    let nested = latest
        .parent()
        .unwrap()
        .join("nested/checkouts")
        .join(latest.file_name().unwrap());
    std::fs::create_dir_all(nested.parent().unwrap()).unwrap();
    std::fs::rename(latest, &nested).unwrap();
    assert!(read_build_receipt(store, &nested).is_none());
    let rooted = receipt_gc::rooted_paths(store).unwrap();
    assert!(!rooted.contains(&alias));
    assert!(!rooted.contains(&nested));
}

#[test]
fn ledger_rejects_foreign_extension_and_nested_content_named_file() {
    for nested in [false, true] {
        let directory = recorded();
        let root = directory.path().join(BUILD_RECEIPTS_DIR).join("evidence");
        let original = walk_files(&root).unwrap().pop().unwrap().path;
        let foreign = if nested {
            root.join("nested").join(original.file_name().unwrap())
        } else {
            original.with_extension("txt")
        };
        std::fs::create_dir_all(foreign.parent().unwrap()).unwrap();
        std::fs::rename(original, foreign).unwrap();
        assert!(stored_receipt_evidence(directory.path()).is_err());
    }
}

#[test]
fn failed_new_evidence_write_preserves_previous_latest_receipt() {
    let directory = recorded();
    let store = directory.path();
    let workspace = store.join("workspace");
    let latest = latest_receipt_path(store, &workspace);
    let previous = std::fs::read(&latest).unwrap();
    let ledger = store.join(BUILD_RECEIPTS_DIR).join("evidence");
    std::fs::remove_dir_all(&ledger).unwrap();
    std::fs::write(ledger, b"blocking file").unwrap();
    let error = record_build_receipt(
        store,
        &"c".repeat(64),
        &"a".repeat(64),
        &workspace,
        Some(&CargoBuildRoots {
            target_dir: workspace.join("target"),
            build_dir: workspace.join("build"),
        }),
        None,
        vec![],
        Some(&ReceiptContext {
            schema: 1,
            source: serde_json::json!({"origin":"new"}),
            tool: serde_json::json!({"sdk":"tool"}),
        }),
        None,
    );
    assert!(error.is_err());
    assert_eq!(std::fs::read(latest).unwrap(), previous);
}
