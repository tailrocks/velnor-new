//! Adversarial regressions for the offline-fetch source scanner.

use std::error::Error;
use std::fs;
use std::path::Path;

use super::impl_orch_f2a_offline::{
    fetch_hits_of_text, is_source_prep_source, production_fetch_hits, production_source_of_text,
    production_src_files, source_prep_executes_text,
};

#[test]
fn scanner_masks_test_spans_but_preserves_same_line_unicode_production()
-> Result<(), Box<dyn Error>> {
    let source = "fn café() {} #[cfg(test)] fn test_fetch() { remote_fetch(); } pub fn production() { remote_fetch(); }\n";
    let production = production_source_of_text(source)?;
    assert!(
        production.contains("pub fn production() { remote_fetch(); }"),
        "masked source: {production:?}"
    );
    assert!(
        !production.contains("test_fetch"),
        "masked source: {production:?}"
    );
    assert_eq!(fetch_hits_of_text(&production)?.len(), 1);
    Ok(())
}

#[test]
fn token_scan_handles_raw_strings_and_ignores_comments() -> Result<(), Box<dyn Error>> {
    let source = r##"fn production() { let _ = r#"a"//"#; let _ = "fetch"; remote_fetch(); } // remote_fetch()
"##;
    let hits = fetch_hits_of_text(source)?;
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].1, "remote_fetch");
    Ok(())
}

#[test]
fn fetch_allowlist_matches_complete_identifiers_only() -> Result<(), Box<dyn Error>> {
    let allowed = "fn f() { fetch_inventory(); fetch_add(1); fetch_steps(); fetch_roots(); unsafe_fetch_root(); let _: FetchFailure; }";
    assert_eq!(fetch_hits_of_text(allowed)?.len(), 0);
    for identifier in ["remote_fetch_steps", "fetch_inventory_and_execute"] {
        let source = format!("fn f() {{ {identifier}(); }}");
        assert!(!fetch_hits_of_text(&source)?.is_empty(), "{identifier}");
    }
    Ok(())
}

#[test]
fn cfg_test_impl_and_trait_methods_are_removed_without_hiding_neighbors()
-> Result<(), Box<dyn Error>> {
    let source = r"
impl Fixture {
    #[cfg(test)] fn test_impl_fetch() { remote_fetch(); }
    fn production_impl() { remote_fetch(); }
}
trait FixtureContract {
    #[cfg(test)] fn test_trait_fetch(&self) { remote_fetch(); }
    fn production_default(&self) { remote_fetch(); }
}
#[cfg_attr(test, cfg(test))]
fn cfg_attr_preserves_production_path() { remote_fetch(); }
";
    let production = production_source_of_text(source)?;
    assert!(
        !production.contains("test_impl_fetch"),
        "masked source: {production:?}"
    );
    assert!(
        !production.contains("test_trait_fetch"),
        "masked source: {production:?}"
    );
    assert_eq!(fetch_hits_of_text(&production)?.len(), 3);
    Ok(())
}

#[test]
fn source_prep_detects_spaced_multiline_and_turbofish_run_calls() -> Result<(), Box<dyn Error>> {
    for call in [
        "command .run ( )",
        "command\n    .run\n    ::<u8>\n    ( )",
        "command.run::<u8>()",
    ] {
        let source = format!("fn production() {{ {call}; }}");
        assert!(source_prep_executes_text(&source)?, "{call}");
    }
    let test_call = "#[cfg(test)] fn helper() { command.run(); } fn production() {}";
    assert!(!source_prep_executes_text(test_call)?);
    let same_line = "#[cfg(test)] fn helper() { command.run(); } fn production() { command .run(); remote_fetch(); }";
    let production = production_source_of_text(same_line)?;
    assert!(source_prep_executes_text(&production)?);
    assert_eq!(fetch_hits_of_text(&production)?.len(), 1);
    Ok(())
}

#[test]
fn source_prep_detects_run_calls_inside_macro_tokens() -> Result<(), Box<dyn Error>> {
    for source in [
        "fn production() { dbg!(command.run()); }",
        "fn production() { invoke!(command . run::<u8> ()); }",
        "macro_rules! hidden_run { ($command:expr) => { $command.run() }; }",
    ] {
        assert!(source_prep_executes_text(source)?, "{source}");
    }
    Ok(())
}

#[test]
fn production_walk_follows_reachable_modules_and_ignores_test_subtrees()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let source_root = temp.path().join("src");
    fs::create_dir_all(&source_root)?;
    let src = fs::canonicalize(source_root)?;
    write_root_and_production_modules(&src)?;
    write_path_module_fixtures(&src)?;
    write_test_sidecars(&src)?;

    let sources = production_src_files(&src)?;
    let relative = sources
        .iter()
        .map(|path| Ok(path.strip_prefix(&src)?.to_path_buf()))
        .collect::<Result<Vec<_>, std::path::StripPrefixError>>()?;
    let mut expected = [
        "cfg_attr_production.rs",
        "explicit_dir/child.rs",
        "explicit_dir/module.rs",
        "folder_module/child.rs",
        "folder_module/mod.rs",
        "inline_api/children/child.rs",
        "inline_default/child.rs",
        "lib.rs",
        "production/nested_inline/inline_child.rs",
        "production/nested/child.rs",
        "production/nested.rs",
        "production.rs",
        "reachable_tests.rs",
        "source_prep.rs",
        "root_attr.rs",
        "nested/source_prep.rs",
        "type.rs",
        "included.rs",
        "impl_hidden.rs",
        "trait_hidden.rs",
        "local_include.rs",
        "local_hidden.rs",
    ]
    .map(std::path::PathBuf::from)
    .to_vec();
    expected.sort();
    assert_eq!(relative, expected);
    let shared = fs::read_to_string(src.join("reachable_tests.rs"))?;
    assert_eq!(fetch_hits_of_text(&shared)?.len(), 1);
    assert_eq!(
        production_fetch_hits(&src.join("cfg_attr_production.rs"))?.len(),
        1
    );
    assert_eq!(
        production_fetch_hits(&src.join("production/nested/child.rs"))?.len(),
        1
    );
    assert_eq!(
        production_fetch_hits(&src.join("inline_default/child.rs"))?.len(),
        1
    );
    assert_fetch_hit(&src, "production/nested_inline/inline_child.rs")?;
    let source_prep = src.join("source_prep.rs").canonicalize()?;
    let nested_source_prep = src.join("nested/source_prep.rs").canonicalize()?;
    assert!(is_source_prep_source(&source_prep, &source_prep));
    assert!(!is_source_prep_source(&nested_source_prep, &source_prep));
    assert_eq!(production_fetch_hits(&nested_source_prep)?.len(), 1);
    assert_eq!(production_fetch_hits(&src.join("included.rs"))?.len(), 1);
    assert_eq!(
        production_fetch_hits(&src.join("local_include.rs"))?.len(),
        1
    );
    assert_eq!(production_fetch_hits(&src.join("impl_hidden.rs"))?.len(), 1);
    assert_fetch_hit(&src, "trait_hidden.rs")?;
    assert_eq!(production_fetch_hits(&src.join("type.rs"))?.len(), 1);
    assert_eq!(
        production_fetch_hits(&src.join("local_hidden.rs"))?.len(),
        1
    );
    Ok(())
}

fn write_root_and_production_modules(src: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        src.join("lib.rs"),
        r#"
mod production;
#[path = "reachable_tests.rs"] pub mod shared_production;
#[cfg(test)] #[path = "reachable_tests.rs"] mod shared_test_alias;
#[path = "explicit_dir/module.rs"] pub mod explicit;
#[cfg_attr(test, cfg(test))] #[path = "cfg_attr_production.rs"] mod cfg_attr_production;
mod folder_module;
#[path = "source_prep.rs"] mod source_prep;
#[path = "nested/source_prep.rs"] mod nested_source_prep;
mod r#type;
#[cfg(test)] #[path = "test_sidecars/root.rs"] mod tests;
#[cfg(test)] mod inline_tests { #[path = "inline_sidecars/helper.rs"] mod helper; }
#[path = "inline_api"] pub mod inline_api { #[path = "children/child.rs"] pub mod child; }
pub mod inline_default { mod child; }
include!("included.rs");
fn local_production_module() {
    #[path = "local_hidden.rs"] mod hidden;
    hidden::remote_fetch();
    #[cfg(test)] #[path = "local_test.rs"] mod test_helper;
    include!("local_include.rs");
}
impl Fixture {
    #[cfg(test)] fn test_local_module() { #[path = "impl_test.rs"] mod helper; }
    fn production_local_module() { #[path = "impl_hidden.rs"] mod helper; }
}
trait FixtureContract {
    #[cfg(test)] fn test_local_module() { #[path = "trait_test.rs"] mod helper; }
    fn production_local_module() { #[path = "trait_hidden.rs"] mod helper; }
}
"#,
    )?;
    fs::write(
        src.join("production.rs"),
        r#"
#[cfg(test)] mod helpers;
#[cfg(test)] #[path = "sidecars/inline.rs"] mod inline_tests;
#[cfg(test)] mod nested { #[cfg(test)] mod helpers; #[path = "nested_sidecars/child.rs"] mod child; }
#[path = "root_attr.rs"] mod external_root_attr;
pub mod nested_inline { #[path = "inline_child.rs"] mod child; }
pub mod nested;
pub fn production() {}
"#,
    )?;
    fs::create_dir_all(src.join("production/nested"))?;
    fs::write(src.join("production/nested.rs"), "mod child;\n")?;
    fs::write(
        src.join("production/nested/child.rs"),
        "pub fn nested_leaf() { remote_fetch(); }\n",
    )?;
    fs::create_dir_all(src.join("production/nested_inline"))?;
    fs::write(
        src.join("production/nested_inline/inline_child.rs"),
        "pub fn nested_inline_leaf() { remote_fetch(); }\n",
    )?;
    fs::write(src.join("root_attr.rs"), "pub fn root_attr_leaf() {}\n")?;
    Ok(())
}

fn write_path_module_fixtures(src: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        src.join("reachable_tests.rs"),
        "pub fn reachable_production() { remote_fetch(); }\n",
    )?;
    fs::write(
        src.join("cfg_attr_production.rs"),
        "pub fn cfg_attr_production() { remote_fetch(); }\n",
    )?;
    fs::create_dir_all(src.join("explicit_dir"))?;
    fs::write(src.join("explicit_dir/module.rs"), "mod child;\n")?;
    fs::write(
        src.join("explicit_dir/child.rs"),
        "pub fn explicit_path_child() {}\n",
    )?;
    fs::create_dir_all(src.join("folder_module"))?;
    fs::write(src.join("folder_module/mod.rs"), "mod child;\n")?;
    fs::write(
        src.join("folder_module/child.rs"),
        "pub fn folder_leaf() {}\n",
    )?;
    fs::create_dir_all(src.join("inline_api/children"))?;
    fs::write(
        src.join("inline_api/children/child.rs"),
        "pub fn inline_child() {}\n",
    )?;
    fs::create_dir_all(src.join("inline_default"))?;
    fs::write(
        src.join("inline_default/child.rs"),
        "pub fn inline_nested_leaf() { remote_fetch(); }\n",
    )?;
    fs::write(src.join("source_prep.rs"), "pub fn source_prep_leaf() {}\n")?;
    fs::create_dir_all(src.join("nested"))?;
    fs::write(
        src.join("nested/source_prep.rs"),
        "pub fn nested_source_prep_leaf() { remote_fetch(); }\n",
    )?;
    fs::write(
        src.join("type.rs"),
        "pub fn raw_ident_leaf() { remote_fetch(); }\n",
    )?;
    fs::write(
        src.join("included.rs"),
        "pub fn included_leaf() { remote_fetch(); }\n",
    )?;
    fs::write(src.join("local_hidden.rs"), "pub fn remote_fetch() {}\n")?;
    fs::write(
        src.join("local_include.rs"),
        "fn included_local() { remote_fetch(); }\n",
    )?;
    write_fetch_fixture(src, "impl_test.rs")?;
    write_fetch_fixture(src, "trait_test.rs")?;
    write_fetch_fixture(src, "impl_hidden.rs")?;
    write_fetch_fixture(src, "trait_hidden.rs")?;
    Ok(())
}

fn write_fetch_fixture(src: &Path, name: &str) -> Result<(), Box<dyn Error>> {
    fs::write(src.join(name), "fn leaf() { remote_fetch(); }\n")?;
    Ok(())
}

fn assert_fetch_hit(src: &Path, file: &str) -> Result<(), Box<dyn Error>> {
    assert_eq!(production_fetch_hits(&src.join(file))?.len(), 1);
    Ok(())
}

fn write_test_sidecars(src: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        src.join("local_test.rs"),
        "pub fn local_test_fetch() { remote_fetch(); }\n",
    )?;
    fs::write(
        src.join("orphan_tests.rs"),
        "fn orphan() { remote_fetch(); }\n",
    )?;
    fs::create_dir_all(src.join("production/helpers"))?;
    fs::write(src.join("production/helpers.rs"), "fn test_fetch() {}\n")?;
    fs::create_dir_all(src.join("production/sidecars"))?;
    fs::write(
        src.join("production/sidecars/inline.rs"),
        "fn test_fetch() {}\n",
    )?;
    fs::create_dir_all(src.join("production/nested/helpers"))?;
    fs::write(
        src.join("production/nested/helpers.rs"),
        "fn test_fetch() {}\n",
    )?;
    fs::create_dir_all(src.join("production/nested_sidecars"))?;
    fs::write(
        src.join("production/nested_sidecars/child.rs"),
        "fn test_fetch() {}\n",
    )?;
    fs::create_dir_all(src.join("test_sidecars"))?;
    fs::write(
        src.join("test_sidecars/root.rs"),
        "mod child; #[path = \"inline.rs\"] mod nested { mod grandchild; }\n",
    )?;
    fs::create_dir_all(src.join("test_sidecars/child"))?;
    fs::write(
        src.join("test_sidecars/child.rs"),
        "mod grandchild; fn sidecar_test_fetch() {}\n",
    )?;
    fs::write(
        src.join("test_sidecars/child/grandchild.rs"),
        "fn nested_sidecar_test_fetch() {}\n",
    )?;
    fs::create_dir_all(src.join("test_sidecars/inline.rs"))?;
    fs::write(
        src.join("test_sidecars/inline.rs/grandchild.rs"),
        "fn inline_path_test_fetch() {}\n",
    )?;
    fs::create_dir_all(src.join("inline_sidecars"))?;
    fs::write(
        src.join("inline_sidecars/helper.rs"),
        "fn test_fetch() {}\n",
    )?;
    Ok(())
}

#[test]
fn production_walk_rejects_missing_and_ambiguous_modules() -> Result<(), Box<dyn Error>> {
    for (files, expected) in [
        (vec![("lib.rs", "mod missing;\n")], "no source"),
        (
            vec![
                ("lib.rs", "mod both;\n"),
                ("both.rs", ""),
                ("both/mod.rs", ""),
            ],
            "ambiguous",
        ),
        (
            vec![("lib.rs", "include!(include_path);\n")],
            "must use a literal path",
        ),
    ] {
        let temp = tempfile::tempdir()?;
        let src = temp.path().join("src");
        fs::create_dir_all(&src)?;
        for (path, contents) in files {
            let file = src.join(path);
            if let Some(parent) = file.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(file, contents)?;
        }
        let error = production_src_files(&src).expect_err("bad module graph must fail");
        assert!(error.to_string().contains(expected), "{error}");
    }
    Ok(())
}
