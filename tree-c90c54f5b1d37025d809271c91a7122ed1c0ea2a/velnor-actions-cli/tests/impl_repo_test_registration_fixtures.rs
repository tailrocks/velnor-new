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
        "[workspace]\nmembers = [\".\", \"no-tests\"]\nresolver = \"3\"\n\n[package]\nname = \"registration-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\nautotests = false\n\n[[test]]\nname = \"registered\"\npath = \"tests/registered.rs\"\n",
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
    scratch.write(
        "tests/registered.rs",
        "#[path = \"actual.rs\"] mod logical;\n#[path = \"alternate\"] mod inline { mod child; }\n#[path = \"fixtures/registered.rs\"] mod fixture;\n#[cfg(target_os = \"macos\")] mod macos_only;\n#[cfg(any())] mod impossible;\ninclude!(\"included.rs\");\nmacro_rules! include_test { () => { include!(\"macro_included.rs\"); }; }\ninclude_test!();\nmacro_rules! unused_include { () => { include!(\"orphan_unused_macro_include.rs\"); }; }\nconst _: &str = include_str!(\"orphan_data.rs\");\nmacro_rules! generated { () => { #[test] fn macro_test() {} }; }\ngenerated!();\n",
    )?;
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
    scratch.write("tests/impossible.rs", "#[test] fn impossible_test() {}\n")?;
    scratch.write(
        "tests/fixtures/registered.rs",
        "#[test] fn registered_fixture() {}\n",
    )?;
    scratch.write("tests/orphan_data.rs", "#[test] fn data_only_test() {}\n")?;
    scratch.write(
        "tests/orphan_unused_macro_include.rs",
        "#[test] fn unused_macro_data() {}\n",
    )?;
    scratch.write("tests/included.rs", "#[test] fn included_test() {}\n")?;
    scratch.write(
        "tests/macro_included.rs",
        "#[test] fn macro_included_test() {}\n",
    )?;
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
        "macro_rules! emitted_test { () => { #[test] fn emitted() {} }; }\nemitted_test!();\n",
    )?;
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
    scratch.write(
        "src/main.rs",
        "#[cfg(not(test))] mod ordinary_only;\nfn main() {}\n",
    )?;
    scratch.write(
        "src/ordinary_only.rs",
        "#[test] fn ordinary_artifact_test() {}\n",
    )?;
    Ok(())
}

#[test]
fn compiler_closure_handles_paths_includes_macros_fixtures_and_orphans() -> Outcome<()> {
    let scratch = ScratchDir::create()?;
    write_workspace(&scratch)?;
    write_test_sources(&scratch)?;
    let manifest = scratch.0.join("Cargo.toml");
    let workspace = super::workspace_plan(&manifest)?;
    let (registered, orphans) = super::registration_audit(&[workspace])?;
    for source in [
        "tests/registered.rs",
        "tests/child.rs",
        "tests/alternate/child.rs",
        "tests/macos_only.rs",
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
    let actual = super::relative_paths(&scratch.0, &orphans)?;
    let mut expected = vec![
        "no-tests/src/lib.rs",
        "src/ordinary_only.rs",
        "tests/impossible.rs",
        "tests/orphan_data.rs",
        "tests/orphan_unused_macro_include.rs",
        "tests/fixtures/orphan.rs",
        "tests/inline/child.rs",
        "tests/logical/child.rs",
        "tests/orphan.rs",
        "tests/orphan_macro.rs",
        "tests/orphan_included.rs",
    ];
    expected.sort_unstable();
    assert_eq!(actual, expected);
    scratch.cleanup()?;
    Ok(())
}
