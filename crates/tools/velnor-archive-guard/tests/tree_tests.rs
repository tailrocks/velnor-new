use std::error::Error;
use std::fs;

use super::archive_guard_inputs;
use super::fixture_root;

#[test]
fn source_tree_fingerprint_closure_includes_new_modules() -> Result<(), Box<dyn Error>> {
    let root = fixture_root()?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "mod new_module;\n")?;
    let before = archive_guard_inputs::collect_tree_files(&root, "src")?;
    fs::write(root.join("src/new_module.rs"), "pub fn value() {}\n")?;
    let after = archive_guard_inputs::collect_tree_files(&root, "src")?;
    assert!(!before.iter().any(|path| path == "src/new_module.rs"));
    assert!(after.iter().any(|path| path == "src/new_module.rs"));
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn source_tree_entry_discovery_is_bounded() -> Result<(), Box<dyn Error>> {
    let root = fixture_root()?;
    fs::create_dir_all(root.join("src"))?;
    for index in 0..513 {
        fs::write(root.join("src").join(format!("module-{index}.rs")), b"")?;
    }
    let result = archive_guard_inputs::collect_tree_files(&root, "src");
    fs::remove_dir_all(&root)?;
    let error = result.expect_err("oversized source tree was accepted");
    assert!(
        error
            .to_string()
            .contains("source tree entry count limit exceeded"),
        "unexpected error: {error}"
    );
    Ok(())
}
