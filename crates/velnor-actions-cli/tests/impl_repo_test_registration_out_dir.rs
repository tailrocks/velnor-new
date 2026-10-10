//! Resolve compiler-evidenced generated Rust sources under Cargo `OUT_DIR`.

use std::collections::HashSet;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

type Outcome<T> = Result<T, Box<dyn Error>>;

pub(super) fn resolve_cargo_out_dir_source(
    suffix: &Path,
    compiler_dependencies: &HashSet<PathBuf>,
    cargo_target_directory: &Path,
) -> Outcome<PathBuf> {
    validate_suffix(suffix)?;
    let target_directory = cargo_target_directory.canonicalize()?;
    let mut matches = compiler_dependencies
        .iter()
        .filter_map(|dependency| {
            let source = dependency.canonicalize().ok()?;
            (source
                .extension()
                .is_some_and(|extension| extension == "rs")
                && source.is_file()
                && cargo_build_out_source(&source, suffix, &target_directory))
            .then_some(source)
        })
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();
    match matches.as_slice() {
        [source] => Ok(source.clone()),
        [] => Err(format!(
            "Cargo OUT_DIR include has no matching Rust source in artifact dep-info under {}: {}",
            target_directory.display(),
            suffix.display()
        )
        .into()),
        _ => Err(format!(
            "Cargo OUT_DIR include is ambiguous in artifact dep-info for {}: {} candidates",
            suffix.display(),
            matches.len()
        )
        .into()),
    }
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

fn cargo_build_out_source(source: &Path, suffix: &Path, target: &Path) -> bool {
    let Ok(relative) = source.strip_prefix(target) else {
        return false;
    };
    let Some(parts) = path_components(relative) else {
        return false;
    };
    let Some(suffix_parts) = path_components(suffix) else {
        return false;
    };
    (1..parts.len()).any(|build| {
        parts[build] == OsStr::new("build")
            && (build == 1 || build == 2)
            && parts[build - 1] == OsStr::new("debug")
            && parts.get(build + 1).is_some_and(|unit| !unit.is_empty())
            && parts.get(build + 2) == Some(&OsString::from("out"))
            && parts.get(build + 3..) == Some(suffix_parts.as_slice())
    })
}

fn path_components(path: &Path) -> Option<Vec<OsString>> {
    path.components()
        .map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_os_string()),
            _ => None,
        })
        .collect()
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
        let target = scratch.0.join("target");
        let first = target.join("debug/build/first-123/out/generated.rs");
        let second = target.join("debug/build/second-456/out/generated.rs");
        let triple = target.join("aarch64-apple-darwin/debug/build/triple-789/out/generated.rs");
        let outside = scratch.0.join("generated.rs");
        for source in [&first, &second, &triple, &outside] {
            fs::create_dir_all(source.parent().ok_or("fixture source has no parent")?)?;
            fs::write(source, "#[test] fn generated() {}\n")?;
        }
        let suffix = Path::new("generated.rs");
        assert!(resolve_cargo_out_dir_source(suffix, &HashSet::new(), &target).is_err());
        let outside_dependency = HashSet::from([outside.canonicalize()?]);
        assert!(resolve_cargo_out_dir_source(suffix, &outside_dependency, &target).is_err());
        let ambiguous = HashSet::from([first.canonicalize()?, second.canonicalize()?]);
        assert!(resolve_cargo_out_dir_source(suffix, &ambiguous, &target).is_err());
        let unique = HashSet::from([first.canonicalize()?]);
        assert_eq!(
            resolve_cargo_out_dir_source(suffix, &unique, &target)?,
            first.canonicalize()?
        );
        let target_specific = HashSet::from([triple.canonicalize()?]);
        assert_eq!(
            resolve_cargo_out_dir_source(suffix, &target_specific, &target)?,
            triple.canonicalize()?
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
        let sources =
            super::super::source_closure(&artifact.source, &artifact.dependencies, &target)?;
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
