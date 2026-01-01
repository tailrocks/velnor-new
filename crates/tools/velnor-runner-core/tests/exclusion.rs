//! The root workspace must compile the runner crates as members (slice 2 merge).

#[test]
fn root_workspace_includes_runner() -> Result<(), &'static str> {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("../../../Cargo.toml");
    let text = std::fs::read_to_string(&root).map_err(|_| "root manifest")?;
    let start = text.find("members = [").ok_or("members")?;
    let block = text[start..].split(']').next().ok_or("members end")?;
    assert!(block.contains("crates/tools/velnor-runner-cli"), "{block}");
    assert!(block.contains("crates/tools/velnor-runner-core"), "{block}");
    assert!(
        block.contains("crates/tools/velnor-runner-github"),
        "{block}"
    );
    assert!(block.contains("crates/tools/velnor-runner-host"), "{block}");
    assert!(!text.contains("exclude = ["), "{text}");
    Ok(())
}
