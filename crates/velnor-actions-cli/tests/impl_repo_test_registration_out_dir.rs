//! Resolve compiler-evidenced generated Rust sources under Cargo `OUT_DIR`.

use std::collections::HashSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

type Outcome<T> = Result<T, Box<dyn Error>>;

pub(super) fn resolve_cargo_out_dir_source(
    suffix: &Path,
    compiler_dependencies: &HashSet<PathBuf>,
    compiler_out_dir: &Path,
) -> Outcome<PathBuf> {
    validate_suffix(suffix)?;
    let out_dir = compiler_out_dir.canonicalize()?;
    if !out_dir.is_dir() {
        return Err(format!("compiler OUT_DIR is not a directory: {}", out_dir.display()).into());
    }
    let expected = out_dir.join(suffix);
    let metadata = fs::symlink_metadata(&expected).map_err(|error| {
        format!(
            "compiler OUT_DIR source is missing {}: {error}",
            expected.display()
        )
    })?;
    if !metadata.file_type().is_file() {
        return Err(format!(
            "compiler OUT_DIR source is not a regular non-symlink file: {}",
            expected.display()
        )
        .into());
    }
    let source = expected.canonicalize()?;
    if source != expected {
        return Err(format!(
            "compiler OUT_DIR source resolves outside its exact path: {}",
            expected.display()
        )
        .into());
    }
    if !compiler_dependencies.contains(&source) {
        return Err(format!(
            "compiler OUT_DIR source is absent from this artifact's dep-info: {}",
            source.display()
        )
        .into());
    }
    Ok(source)
}

fn validate_suffix(suffix: &Path) -> Outcome<()> {
    if suffix.as_os_str().is_empty()
        || suffix.is_absolute()
        || suffix.to_string_lossy().contains(['\\', ':', '\0'])
        || suffix
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        || suffix.extension().is_none_or(|extension| extension != "rs")
    {
        return Err("Cargo OUT_DIR suffix is not a safe relative Rust source path".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::resolve_cargo_out_dir_source;
    use crate::impl_repo_test_registration as registration;
    use crate::impl_repo_test_registration::graph::{IncludePath, direct_include_path};
    use std::collections::HashSet;
    use std::error::Error;
    use std::fs;
    use std::path::{Path, PathBuf};

    type Outcome<T> = Result<T, Box<dyn Error>>;

    fn parse_include(expression: &str) -> Outcome<IncludePath> {
        let tokens = expression.parse()?;
        direct_include_path(&tokens, Path::new("tests/root.rs"))
    }

    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn create() -> Outcome<Self> {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "velnor out-dir evidence-{}-{unique}",
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
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            if let Err(error) = fs::remove_dir_all(&self.0)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                eprintln!("cannot remove fixture {}: {error}", self.0.display());
            }
        }
    }

    #[test]
    fn out_dir_resolution_requires_unique_in_scope_dep_info() -> Outcome<()> {
        let scratch = ScratchDir::create()?;
        let out_dir = scratch.0.join("compiler cache with spaces/out");
        let other_out_dir = scratch.0.join("other compiler cache/out");
        let generated = out_dir.join("nested/generated.rs");
        let other_generated = other_out_dir.join("nested/generated.rs");
        let unrelated = scratch.0.join("unrelated/generated.rs");
        for source in [&generated, &other_generated, &unrelated] {
            fs::create_dir_all(source.parent().ok_or("fixture source has no parent")?)?;
            fs::write(source, "#[test] fn generated() {}\n")?;
        }
        let suffix = Path::new("nested/generated.rs");
        assert!(resolve_cargo_out_dir_source(suffix, &HashSet::new(), &out_dir).is_err());
        let cross_artifact = HashSet::from([other_generated.canonicalize()?]);
        assert!(resolve_cargo_out_dir_source(suffix, &cross_artifact, &out_dir).is_err());
        let unrelated_dependency = HashSet::from([unrelated.canonicalize()?]);
        assert!(resolve_cargo_out_dir_source(suffix, &unrelated_dependency, &out_dir).is_err());
        let unique = HashSet::from([generated.canonicalize()?]);
        assert_eq!(
            resolve_cargo_out_dir_source(suffix, &unique, &out_dir)?,
            generated.canonicalize()?
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn out_dir_resolution_rejects_symlinked_generated_source() -> Outcome<()> {
        use std::os::unix::fs::symlink;

        let scratch = ScratchDir::create()?;
        let out_dir = scratch.0.join("out");
        let outside = scratch.0.join("outside.rs");
        let generated = out_dir.join("generated.rs");
        fs::create_dir_all(&out_dir)?;
        fs::write(&outside, "#[test] fn generated() {}\n")?;
        symlink(&outside, &generated)?;
        let dependencies = HashSet::from([outside.canonicalize()?]);
        assert!(
            resolve_cargo_out_dir_source(Path::new("generated.rs"), &dependencies, &out_dir)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn direct_out_dir_concat_requires_a_safe_literal_suffix() -> Outcome<()> {
        assert!(matches!(
            parse_include("concat!(env!(\"OUT_DIR\"), \"/compile_identity.rs\")")?,
            IncludePath::CargoOutDir(path) if path == Path::new("compile_identity.rs")
        ));
        for expression in [
            "concat!(env!(\"OUT_DIR\"), \"../escape.rs\")",
            "concat!(env!(\"OUT_DIR\"), env!(\"SOURCE\"))",
            "concat!(env!(\"OTHER\"), \"/generated.rs\")",
            "concat!(env!(\"OUT_DIR\"), \"/generated.txt\")",
            "concat!(env!(\"OUT_DIR\"), \"/generated.rs\", suffix)",
            "concat!(env!(\"OUT_DIR\"), \"//generated.rs\")",
            "concat!{env!(\"OUT_DIR\"), \"/generated.rs\"}",
            "concat!(env!{\"OUT_DIR\"}, \"/generated.rs\")",
        ] {
            assert!(parse_include(expression).is_err(), "accepted {expression}");
        }
        assert!(matches!(
            parse_include("\"included.rs\"")?,
            IncludePath::Relative(path) if path == Path::new("tests/included.rs")
        ));
        Ok(())
    }

    #[test]
    fn compiled_out_dir_include_is_in_test_source_closure() -> Outcome<()> {
        let scratch = ScratchDir::create()?;
        scratch.write(
            "Cargo.toml",
            "[package]\nname = \"out-dir-include-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\nautotests = false\n\n[[test]]\nname = \"registered\"\npath = \"tests/registered.rs\"\n",
        )?;
        scratch.write(
            "Cargo.lock",
            "version = 4\n\n[[package]]\nname = \"out-dir-include-fixture\"\nversion = \"0.1.0\"\n",
        )?;
        scratch.write(
            "build.rs",
            "fn main() { let out = std::env::var(\"OUT_DIR\").expect(\"OUT_DIR\"); std::fs::write(std::path::PathBuf::from(out).join(\"generated.rs\"), \"#[test] fn generated_test() {}\\n\").expect(\"write generated source\"); }\n",
        )?;
        scratch.write(
            "tests/registered.rs",
            "include!(concat!(env!(\"OUT_DIR\"), \"/generated.rs\"));\n",
        )?;
        let manifest = scratch.0.join("Cargo.toml");
        let target = scratch.0.join("target");
        let workspace = registration::cargo_config::workspace_plan_at_target(&manifest, &target)?;
        let output = registration::cargo_config::cargo_output_at_target(
            &manifest,
            &["test", "--no-run", "--locked", "--message-format=json"],
            &target,
        )?;
        let package_ids = workspace
            .packages
            .iter()
            .map(|package| package.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        let mut artifact = None;
        for line in output
            .stdout
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let message: serde_json::Value = serde_json::from_slice(line)?;
            if let Some(found) = registration::test_artifact(&message, &package_ids, &workspace)? {
                assert!(
                    artifact.replace(found).is_none(),
                    "fixture produced extra test artifacts"
                );
            }
        }
        let artifact = artifact.ok_or("fixture produced no compiled test artifact")?;
        let out_dir = artifact
            .out_dir
            .as_deref()
            .ok_or("artifact dep-info omits Cargo OUT_DIR binding")?;
        assert!(
            super::super::source_closure(&artifact.source, &artifact.dependencies, None).is_err()
        );
        let sources =
            super::super::source_closure(&artifact.source, &artifact.dependencies, Some(out_dir))?;
        let generated = artifact
            .dependencies
            .iter()
            .find(|source| {
                source
                    .file_name()
                    .is_some_and(|name| name == "generated.rs")
            })
            .ok_or("artifact dep-info omits generated test source")?;
        assert!(
            sources.contains(generated),
            "generated source is absent from closure"
        );
        assert!(registration::graph::source_findings(generated)?.test_bearing);
        assert!(super::super::declared_target_source_closure(&artifact.source).is_err());
        Ok(())
    }
}
