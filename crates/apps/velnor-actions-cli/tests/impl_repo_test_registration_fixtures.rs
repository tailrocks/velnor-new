//! Cargo/rustc registration regression fixtures.

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

type Outcome<T> = Result<T, Box<dyn Error>>;

struct ScratchDir(PathBuf);

impl ScratchDir {
    fn create() -> Outcome<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "velnor test registration-{}-{nanos}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn write(&self, relative: &str, body: &str) -> Outcome<PathBuf> {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, body)?;
        Ok(path)
    }

    fn cleanup(&self) -> Outcome<()> {
        fs::remove_dir_all(&self.0)?;
        Ok(())
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("cannot remove test fixture {}: {error}", self.0.display());
        }
    }
}

fn write_workspace(scratch: &ScratchDir) -> Outcome<()> {
    scratch.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\".\", \"no-tests\"]\nresolver = \"3\"\n\n[package]\nname = \"registration-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\nautotests = false\n\n[features]\ndisabled = []\n\n[[test]]\nname = \"registered\"\npath = \"tests/registered.rs\"\n\n[[test]]\nname = \"feature-registered\"\npath = \"tests/feature_registered.rs\"\nrequired-features = [\"disabled\"]\n",
    )?;
    scratch.write(
        "Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"no-tests\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"registration-fixture\"\nversion = \"0.1.0\"\n",
    )?;
    scratch.write(
        "no-tests/Cargo.toml",
        "[package]\nname = \"no-tests\"\nversion = \"0.1.0\"\nedition = \"2024\"\nautotests = false\n\n[lib]\ntest = false\n",
    )?;
    scratch.write(
        "no-tests/src/lib.rs",
        "#[test] fn package_without_test_target() {}\n",
    )?;
    Ok(())
}

fn write_test_sources(scratch: &ScratchDir) -> Outcome<()> {
    write_registered_test_roots(scratch)?;
    write_registered_module_sources(scratch)?;
    write_registered_include_sources(scratch)?;
    write_orphan_test_sources(scratch)?;
    write_package_source_candidates(scratch)
}

fn write_registered_test_roots(scratch: &ScratchDir) -> Outcome<()> {
    scratch.write(
        "tests/registered.rs",
        "macro_rules! selected { () => {}; }\n#[path = \"actual.rs\"] mod logical;\n#[path = \"alternate\"] mod inline { mod child; }\n#[path = \"fixtures/registered.rs\"] mod fixture;\n#[cfg(target_os = \"macos\")] mod macos_only;\n#[cfg_attr(feature = \"disabled\", cfg(any()))] mod cfg_attr_possible;\n#[cfg_attr(test, cfg(any()))] mod test_filtered;\n#[cfg(any())] mod impossible;\n#[cfg(target_os = \"macos\")] include!(\"macos_included.rs\");\ninclude!(\"included.rs\");\nmacro_rules! include_test { () => { include!(\"macro_included.rs\"); }; }\ninclude_test!();\nmacro_rules! unused_include { () => { include!(\"orphan_data.rs\"); }; }\nmod shadowed_macro { selected!(); macro_rules! selected { () => { include!(\"orphan_data.rs\"); }; } }\nconst _: &str = include_str!(\"orphan_data.rs\");\nmacro_rules! generated { () => { #[test] fn macro_test() {} }; }\ngenerated!();\n",
    )?;
    scratch.write(
        "tests/feature_registered.rs",
        "mod gated_child;\n#[test] fn feature_gated_target_test() {}\n",
    )?;
    scratch.write(
        "tests/gated_child.rs",
        "#[test] fn feature_gated_child_test() {}\n",
    )?;
    Ok(())
}

fn write_registered_module_sources(scratch: &ScratchDir) -> Outcome<()> {
    scratch.write("tests/actual.rs", "mod child;\n")?;
    scratch.write("tests/child.rs", "#[test] fn external_child() {}\n")?;
    scratch.write(
        "tests/alternate/child.rs",
        "#[test] fn inline_path_child() {}\n",
    )?;
    scratch.write(
        "tests/macos_only.rs",
        "#[test] fn platform_registered_test() {}\n",
    )?;
    scratch.write(
        "tests/cfg_attr_possible.rs",
        "#[test] fn cfg_attr_possible_test() {}\n",
    )?;
    scratch.write("tests/test_filtered.rs", "#[test] fn filtered_test() {}\n")?;
    Ok(())
}

fn write_registered_include_sources(scratch: &ScratchDir) -> Outcome<()> {
    scratch.write(
        "tests/macos_included.rs",
        "mod macos_child;\n#[test] fn target_include_test() {}\n",
    )?;
    scratch.write(
        "tests/macos_child.rs",
        "#[test] fn target_include_child() {}\n",
    )?;
    scratch.write("tests/impossible.rs", "#[test] fn impossible_test() {}\n")?;
    scratch.write(
        "tests/fixtures/registered.rs",
        "#[test] fn registered_fixture() {}\n",
    )?;
    scratch.write("tests/orphan_data.rs", "#[test] fn data_only_test() {}\n")?;
    scratch.write("tests/included.rs", "#[test] fn included_test() {}\n")?;
    scratch.write(
        "tests/macro_included.rs",
        "#[test] fn macro_included_test() {}\n",
    )?;
    Ok(())
}

fn write_orphan_test_sources(scratch: &ScratchDir) -> Outcome<()> {
    scratch.write(
        "tests/logical/child.rs",
        "#[test] fn wrong_external_path() {}\n",
    )?;
    scratch.write(
        "tests/inline/child.rs",
        "#[test] fn wrong_inline_path() {}\n",
    )?;
    scratch.write("tests/orphan.rs", "#[test] fn orphan_test() {}\n")?;
    scratch.write(
        "tests/orphan_macro.rs",
        "macro_rules! emitted_test { ($name:ident) => { #[test] fn $name() {} }; }\nemitted_test!(emitted);\n",
    )?;
    scratch.write(
        "tests/orphan_macro_definition.rs",
        "macro_rules! emitted_elsewhere { () => { #[test] fn emitted() {} }; }\n",
    )?;
    scratch.write("tests/orphan_macro_call.rs", "emitted_elsewhere!();\n")?;
    scratch.write(
        "tests/fixtures/orphan.rs",
        "#[test] fn fixture_orphan() {}\n",
    )?;
    scratch.write(
        "tests/orphan_include.rs",
        "macro_rules! included { () => { include!(\"orphan_included.rs\") }; }\nincluded!();\n",
    )?;
    scratch.write(
        "tests/orphan_included.rs",
        "#[test] fn included_orphan() {}\n",
    )?;
    Ok(())
}

fn write_package_source_candidates(scratch: &ScratchDir) -> Outcome<()> {
    scratch.write(
        "src/main.rs",
        "#[cfg(test)]\nmod ordinary_tests {\n    #[test]\n    fn ordinary_artifact_is_compiled() {}\n}\n\n#[cfg(not(test))]\nmod ordinary_only;\nfn main() {}\n",
    )?;
    scratch.write(
        "src/ordinary_only.rs",
        "#[test] fn ordinary_artifact_test() {}\n",
    )?;
    scratch.write(
        "src/target/orphan.rs",
        "#[test] fn source_directory_named_target_is_scanned() {}\n",
    )?;
    scratch.write(
        "target/build_output.rs",
        "#[test] fn actual_cargo_target_output_is_ignored() {}\n",
    )?;
    Ok(())
}

#[test]
fn unproven_invoked_macro_includes_fail_closed() -> Outcome<()> {
    let scratch = ScratchDir::create()?;
    let source = scratch.write(
        "ambiguous.rs",
        "macro_rules! include_by_shape { () => { include!(\"one.rs\") }; ($value:tt) => { include!(\"two.rs\") }; }\ninclude_by_shape!();\n",
    )?;
    let findings = super::graph::source_findings(&source)?;
    assert!(super::graph::active_macro_includes(&findings).is_err());
    scratch.cleanup()?;
    Ok(())
}

#[test]
fn compiler_closure_handles_paths_includes_macros_fixtures_and_orphans() -> Outcome<()> {
    let scratch = ScratchDir::create()?;
    write_workspace(&scratch)?;
    write_test_sources(&scratch)?;
    let manifest = scratch.0.join("Cargo.toml");
    let target_dir = scratch.0.join("target");
    let workspace = super::cargo_config::workspace_plan_at_target(&manifest, &target_dir)?;
    let data_only = scratch.0.join("tests/orphan_data.rs").canonicalize()?;
    assert!(compiler_dependencies_contain(&workspace, &data_only)?);
    let feature_target = scratch
        .0
        .join("tests/feature_registered.rs")
        .canonicalize()?;
    assert!(!compiler_dependencies_contain(&workspace, &feature_target)?);
    let (registered, orphans) = super::registration_audit(&[workspace])?;
    assert!(
        target_dir.is_dir(),
        "fixture did not use Cargo's target dir"
    );
    assert!(!orphans.contains(&scratch.0.join("target/build_output.rs").canonicalize()?));
    assert!(orphans.contains(&scratch.0.join("src/target/orphan.rs").canonicalize()?));
    let registered_root = scratch.0.join("tests/registered.rs").canonicalize()?;
    let macos_included = scratch.0.join("tests/macos_included.rs").canonicalize()?;
    let root_findings = super::graph::source_findings(&registered_root)?;
    assert!(root_findings.includes.iter().any(|include| {
        include.path == macos_included && include.condition == super::graph::Possibility::Sometimes
    }));
    for source in [
        "tests/registered.rs",
        "tests/child.rs",
        "tests/alternate/child.rs",
        "tests/feature_registered.rs",
        "tests/gated_child.rs",
        "tests/macos_only.rs",
        "tests/cfg_attr_possible.rs",
        "tests/macos_included.rs",
        "tests/macos_child.rs",
        "tests/fixtures/registered.rs",
        "tests/included.rs",
        "tests/macro_included.rs",
    ] {
        let path = scratch.0.join(source).canonicalize()?;
        assert!(
            registered.contains(&path),
            "compiler closure omitted {source}"
        );
        assert!(
            super::graph::source_findings(&path)?.test_bearing,
            "test syntax was not found in {source}"
        );
    }
    assert!(!registered.contains(&data_only));
    let actual = super::relative_paths(&scratch.0.canonicalize()?, &orphans)?;
    let mut expected = vec![
        "no-tests/src/lib.rs",
        "src/ordinary_only.rs",
        "tests/impossible.rs",
        "tests/orphan_data.rs",
        "tests/test_filtered.rs",
        "tests/fixtures/orphan.rs",
        "tests/inline/child.rs",
        "tests/logical/child.rs",
        "tests/orphan.rs",
        "tests/orphan_macro.rs",
        "tests/orphan_macro_definition.rs",
        "tests/orphan_included.rs",
        "src/target/orphan.rs",
    ];
    expected.sort_unstable();
    assert_eq!(actual, expected);
    scratch.cleanup()?;
    Ok(())
}

fn compiler_dependencies_contain(
    workspace: &super::WorkspacePlan,
    source: &std::path::Path,
) -> Outcome<bool> {
    let output = super::cargo_config::cargo_output_at_target(
        &workspace.manifest,
        &["nextest", "list", "--locked", "--message-format", "json"],
        &workspace.target_directory,
    )?;
    let document = super::nextest_document(&output)?;
    for suite in document["rust-suites"]
        .as_object()
        .into_iter()
        .flat_map(|suites| suites.values().collect::<Vec<_>>())
    {
        let Some(executable) = suite["binary-path"].as_str() else {
            continue;
        };
        let dep_info = std::path::PathBuf::from(executable).with_extension("d");
        if !dep_info.is_file() {
            continue;
        }
        if super::dep_info::sources(&dep_info, &workspace.root)?
            .into_iter()
            .any(|dependency| dependency == source)
        {
            return Ok(true);
        }
    }
    Ok(false)
}
