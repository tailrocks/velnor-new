//! Regressions for Rust source-relative module and include resolution.

use std::error::Error;
use std::fs;

use super::impl_orch_f2a_offline::{
    production_fetch_hits, production_src_files, source_prep_executes_text,
    source_prep_tree_executes,
};

#[test]
fn source_prep_default_module_dir_beats_same_named_parent_file() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let src = temp.path().join("src");
    let module_dir = src.join("source_prep");
    fs::create_dir_all(&module_dir)?;
    let source_prep = src.join("source_prep.rs");
    fs::write(&source_prep, "mod child;\n")?;
    fs::write(src.join("child.rs"), "fn benign() {}\n")?;
    fs::write(
        module_dir.join("child.rs"),
        "fn hidden() { command.run(); }\n",
    )?;

    assert!(source_prep_tree_executes(&source_prep)?);
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinked_source_prep_keeps_logical_include_path() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir()?;
    let src = temp.path().join("src");
    let actual = src.join("actual");
    fs::create_dir_all(&actual)?;
    let source_prep = src.join("source_prep.rs");
    fs::write(actual.join("real.rs"), "include!(\"hidden.rs\");\n")?;
    symlink(actual.join("real.rs"), &source_prep)?;
    fs::write(src.join("hidden.rs"), "fn hidden() { command.run(); }\n")?;
    fs::write(actual.join("hidden.rs"), "fn benign() {}\n")?;

    assert!(source_prep_tree_executes(&source_prep)?);
    Ok(())
}

#[test]
fn included_source_resolves_modules_from_its_own_directory() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let src = temp.path().join("src");
    let included_dir = src.join("sub");
    fs::create_dir_all(&included_dir)?;
    fs::write(src.join("lib.rs"), "include!(\"sub/included.rs\");\n")?;
    fs::write(included_dir.join("included.rs"), "mod child;\n")?;
    fs::write(src.join("child.rs"), "fn benign() {}\n")?;
    fs::write(
        included_dir.join("child.rs"),
        "fn hidden() { remote_fetch(); }\n",
    )?;

    let files = production_src_files(&src)?;
    assert!(files.contains(&fs::canonicalize(included_dir.join("child.rs"))?));
    assert_eq!(
        production_fetch_hits(&included_dir.join("child.rs"))?.len(),
        1
    );
    Ok(())
}

#[test]
fn expression_include_is_parsed_as_an_expression() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let src = temp.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("lib.rs"),
        "const VALUE: u32 = include!(\"value.rs\");\n",
    )?;
    fs::write(src.join("value.rs"), "20 + 22\n")?;

    let files = production_src_files(&src)?;
    assert!(files.contains(&fs::canonicalize(src.join("value.rs"))?));
    assert_eq!(production_fetch_hits(&src.join("value.rs"))?.len(), 0);
    Ok(())
}

#[test]
fn statement_and_macro_token_includes_accept_expression_sources() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let src = temp.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("lib.rs"),
        "fn statement() { include!(\"statement.rs\"); }\
         macro_rules! token_argument { () => { include!(\"macro.rs\") }; }\n",
    )?;
    fs::write(src.join("statement.rs"), "20 + 22")?;
    fs::write(src.join("macro.rs"), "42")?;

    let files = production_src_files(&src)?;
    assert!(files.contains(&fs::canonicalize(src.join("statement.rs"))?));
    assert!(files.contains(&fs::canonicalize(src.join("macro.rs"))?));
    Ok(())
}

#[test]
fn expression_include_masks_test_only_remote_fetch_and_run_calls() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let src = temp.path().join("src");
    fs::create_dir_all(&src)?;
    fs::write(
        src.join("lib.rs"),
        "const VALUE: () = include!(\"test.rs\");\n",
    )?;
    let included = src.join("test.rs");
    fs::write(
        &included,
        "#[cfg(test)] { remote_fetch(); command.run(); 42 }\n",
    )?;

    assert_eq!(production_fetch_hits(&included)?.len(), 0);
    assert!(!source_prep_executes_text(&fs::read_to_string(included)?)?);
    Ok(())
}

#[test]
fn cfg_test_statement_macros_are_masked_and_skipped() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let src = temp.path().join("src");
    fs::create_dir_all(&src)?;
    let root = src.join("lib.rs");
    let source = "fn run() { #[cfg(test)] invoke!(command.run()); }\
                  fn missing_include() { #[cfg(test)] include!(\"missing.rs\"); }\n";
    fs::write(&root, source)?;

    assert!(!source_prep_executes_text(source)?);
    assert_eq!(production_src_files(&src)?, vec![fs::canonicalize(root)?]);
    Ok(())
}

#[test]
fn inner_cfg_test_source_contributes_no_modules_or_findings() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let src = temp.path().join("src");
    fs::create_dir_all(&src)?;
    let root = src.join("lib.rs");
    let disabled = src.join("disabled.rs");
    fs::write(&root, "mod disabled;\n")?;
    fs::write(
        &disabled,
        "#![cfg(test)]\nmod missing;\nfn hidden() { remote_fetch(); command.run(); }\n",
    )?;

    assert_eq!(production_src_files(&src)?, vec![fs::canonicalize(&root)?]);
    assert_eq!(production_fetch_hits(&disabled)?.len(), 0);
    assert!(!source_prep_executes_text(&fs::read_to_string(&disabled)?)?);
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinked_source_keeps_logical_include_path() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir()?;
    let src = temp.path().join("src");
    let actual = src.join("actual");
    fs::create_dir_all(&actual)?;
    fs::write(src.join("lib.rs"), "mod alias;\n")?;
    symlink(actual.join("real.rs"), src.join("alias.rs"))?;
    fs::write(actual.join("real.rs"), "include!(\"hidden.rs\");\n")?;
    fs::write(src.join("hidden.rs"), "fn hidden() { remote_fetch(); }\n")?;
    fs::write(actual.join("hidden.rs"), "fn benign() {}\n")?;

    let files = production_src_files(&src)?;
    let expected = fs::canonicalize(src.join("hidden.rs"))?;
    assert!(files.contains(&expected));
    assert_eq!(production_fetch_hits(&expected)?.len(), 1);
    Ok(())
}
