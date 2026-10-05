//! The root workspace must not compile the nested runner as a member.

#[test]
fn root_workspace_excludes_runner() -> Result<(), &'static str> {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("../../../../Cargo.toml");
    let text = std::fs::read_to_string(&root).map_err(|_| "root manifest")?;
    let start = text.find("members = [").ok_or("members")?;
    let block = text[start..].split(']').next().ok_or("members end")?;
    assert!(!block.contains("velnor-runner"), "{block}");
    assert!(text.contains("exclude = [\"crates/velnor-runner\"]"));
    Ok(())
}
