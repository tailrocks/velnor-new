//! Adversarial macro and conditional-path regressions for the offline gate.

use std::error::Error;
use std::fs;
use std::path::Path;

use super::impl_orch_f2a_offline::{
    fetch_hits_of_text, production_fetch_hits, production_src_files, source_prep_executes_text,
    source_prep_tree_executes,
};

#[test]
fn source_prep_detects_runs_in_associated_macro_tokens_and_raw_identifiers()
-> Result<(), Box<dyn Error>> {
    for source in [
        "fn production() { command.r#run(); }",
        "fn production() { Command::run(&command); }",
        "fn production() { <Command as Runner>::r#run(&command); }",
        "macro_rules! associated_run { () => { Command::r#run(&command) }; }",
        "impl Fixture { define_method!(fn generated() { command.run(); }); }",
        "trait Fixture { define_method!(fn generated() { command.r#run(); }); }",
        "macro_rules! invoke { ($command:expr) => { $command.run::<fn() -> u8>() }; }",
    ] {
        assert!(source_prep_executes_text(source)?, "{source}");
    }
    Ok(())
}

#[test]
fn fetch_scanner_detects_executable_cargo_fetch_but_ignores_descriptive_literals()
-> Result<(), Box<dyn Error>> {
    for source in [
        r#"fn production() { Command::new("cargo").arg("fetch"); }"#,
        r#"fn production() { std::process::Command::new("cargo").args(["fetch", "--locked"]); }"#,
        r#"macro_rules! hidden_fetch { () => { Command::new("cargo").arg("fetch") }; }"#,
        r#"macro_rules! parenthesized_repeated_fetch { ($($extra:expr),*) => { (Command::new("cargo")).args(["fetch", $($extra),*]).status(); }; } fn production() { parenthesized_repeated_fetch!("--locked"); }"#,
        r#"macro_rules! hidden { ($($extra:expr),*) => { (Command::new("cargo")).args([$($extra),*]).arg("fetch").status(); }; } fn production() { hidden!("--locked"); }"#,
        r#"fn production() { Command::new("cargo").args(["fetch"; 1]).status(); }"#,
        r#"macro_rules! repeated_array { () => { Command::new("cargo").args(["fetch"; 1]).status(); }; }"#,
        r#"macro_rules! qualified_repeated_args { ($($extra:expr),*) => { (std::process::Command::new("cargo")).args(["fetch", $($extra),*]).status(); }; } fn production() { qualified_repeated_args!("--locked"); }"#,
    ] {
        assert!(!fetch_hits_of_text(source)?.is_empty(), "{source}");
    }
    for source in [
        r#"let description = "cargo fetch";"#,
        r#"workflow_step("cargo", "fetch");"#,
        r#"println!("cargo fetch");"#,
        r#"fn production() { Command::new("cargo").args(["fetch"].map(|_| "build")).status(); }"#,
        r#"fn production() { Command::new("cargo").args(["fetch"; 0]).status(); }"#,
        r#"macro_rules! empty_repeated_array { () => { Command::new("cargo").args(["fetch"; 0]).status(); }; }"#,
    ] {
        assert!(fetch_hits_of_text(source)?.is_empty(), "{source}");
    }
    Ok(())
}

#[test]
fn production_walk_follows_qualified_and_macro_includes_and_cfg_attr_paths()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let source_root = temp.path().join("src");
    fs::create_dir_all(&source_root)?;
    let src = fs::canonicalize(source_root)?;
    fs::write(
        src.join("lib.rs"),
        r#"
std::include!("qualified_include.rs");
macro_rules! include_hidden { () => { include!("macro_include.rs") }; }
#[cfg_attr(not(test), path = "cfg_attr_hidden.rs")] mod conditional_path;
"#,
    )?;
    write_fetch_source(&src, "qualified_include.rs")?;
    write_fetch_source(&src, "macro_include.rs")?;
    write_fetch_source(&src, "cfg_attr_hidden.rs")?;

    let sources = production_src_files(&src)?;
    let relative = sources
        .iter()
        .map(|path| Ok(path.strip_prefix(&src)?.to_path_buf()))
        .collect::<Result<Vec<_>, std::path::StripPrefixError>>()?;
    let mut expected = [
        "lib.rs",
        "qualified_include.rs",
        "macro_include.rs",
        "cfg_attr_hidden.rs",
    ]
    .map(std::path::PathBuf::from)
    .to_vec();
    expected.sort();
    assert_eq!(relative, expected);
    for file in [
        "qualified_include.rs",
        "macro_include.rs",
        "cfg_attr_hidden.rs",
    ] {
        assert_eq!(production_fetch_hits(&src.join(file))?.len(), 1, "{file}");
    }

    let uncertain = temp.path().join("uncertain");
    fs::create_dir_all(&uncertain)?;
    fs::write(
        uncertain.join("lib.rs"),
        "#[cfg_attr(feature = \"unknown\", path = \"hidden.rs\")] mod hidden;\n",
    )?;
    write_fetch_source(&uncertain, "hidden.rs")?;
    let error = production_src_files(&uncertain).expect_err("unknown path cfg must fail closed");
    assert!(
        error
            .to_string()
            .contains("unresolved cfg_attr predicate controls a production module path"),
        "{error}"
    );
    Ok(())
}

#[test]
fn production_walk_fails_closed_on_macro_generated_modules() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let source_root = temp.path().join("src");
    fs::create_dir_all(&source_root)?;
    let src = fs::canonicalize(source_root)?;
    fs::write(
        src.join("lib.rs"),
        "macro_rules! declare { () => { #[path = \"hidden.rs\"] mod hidden; }; }\ndeclare!();\n",
    )?;
    write_fetch_source(&src, "hidden.rs")?;
    let error = production_src_files(&src).expect_err("module expansion must fail closed");
    assert!(
        error.to_string().contains("module-generating macro"),
        "{error}"
    );
    Ok(())
}

#[test]
fn source_prep_scans_literal_include_dependencies_for_run_calls() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let source_prep = temp.path().join("source_prep.rs");
    fs::write(&source_prep, "include!(\"source_prep_impl.rs\");\n")?;
    fs::write(
        temp.path().join("source_prep_impl.rs"),
        "fn hidden() { command.run(); }\n",
    )?;
    assert!(source_prep_tree_executes(&source_prep)?);
    Ok(())
}

fn write_fetch_source(directory: &Path, file: &str) -> Result<(), Box<dyn Error>> {
    fs::write(
        directory.join(file),
        "pub fn included() { remote_fetch(); }\n",
    )?;
    Ok(())
}
