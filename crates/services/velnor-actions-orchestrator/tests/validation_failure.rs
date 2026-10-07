//! Validation-failure hermetic case: validator failure preserves output.
//!
//! Unix-only: the test re-executes this binary with a fake `mise` first on
//! `PATH`, so validator invocations fail while the rest of the toolchain
//! keeps working. No process-env mutation, no new dependencies.

#[cfg(unix)]
mod probe {
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use tempfile::TempDir;
    use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};
    use velnor_actions_orchestrator_core::OrchestratorError;

    /// Marker selecting the scrubbed child scenario.
    const MARKER: &str = "VELNOR_VALIDATION_PROBE";
    /// Real `mise` location for the fake to delegate to.
    const REAL_MISE: &str = "VELNOR_REAL_MISE";

    /// Fake `mise`: validator tool specs fail, everything else delegates.
    const FAKE_MISE: &str = "#!/bin/sh
for arg in \"$@\"; do
  case \"$arg\" in
    actionlint@*|shellcheck@*|zizmor@*) echo 'fake-mise: validators disabled' >&2; exit 1;;
  esac
done
exec \"$VELNOR_REAL_MISE\" \"$@\"
";

    #[test]
    fn validation_failure_preserves_output() -> Result<(), Box<dyn std::error::Error>> {
        if std::env::var(MARKER).is_ok() {
            return scrubbed_child();
        }
        let fake_dir = TempDir::new()?;
        let script = fake_dir.path().join("mise");
        fs::write(&script, FAKE_MISE)?;
        let mut perms = fs::metadata(&script)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms)?;
        let output = Command::new(std::env::current_exe()?)
            .arg("probe::validation_failure_preserves_output")
            .arg("--exact")
            .env(MARKER, "1")
            .env(REAL_MISE, find_mise()?)
            .env("PATH", scrubbed_path(fake_dir.path())?)
            .output()?;
        assert!(
            output.status.success(),
            "scrubbed child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }

    /// Git-init one fixture repo with the shared test identity.
    fn git_init(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
        for args in [
            vec!["init", "-b", "testmain"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            assert!(
                Command::new("git")
                    .args(&args)
                    .current_dir(root)
                    .status()?
                    .success()
            );
        }
        Ok(())
    }

    /// The child scenario: validators fail, every output is preserved.
    fn scrubbed_child() -> Result<(), Box<dyn std::error::Error>> {
        let dir = TempDir::new()?;
        let root = dir.path();
        git_init(root)?;
        fs::create_dir_all(root.join(".velnor"))?;
        fs::write(
            root.join(".velnor/config.toml"),
            "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n",
        )?;
        fs::write(
            root.join(".velnor/release-manifest.json"),
            manifest_fixture(),
        )?;
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )?;
        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("src/lib.rs"), "pub fn f() {}\n")?;
        let prep = prepare(root)?;
        fs::create_dir_all(root.join(".github/workflows"))?;
        fs::write(root.join(".github/workflows/old.yml"), "old: true\n")?;
        let before = content(root)?;

        // In-place generate fails validation before any write.
        let err = generate(&prep, &GenerateOptions { output_dir: None })
            .err()
            .ok_or("expected validation failure")?;
        assert!(
            matches!(err, OrchestratorError::Validation { .. }),
            "validation error, got {err}"
        );
        assert!(
            err.to_string().contains("fake-mise"),
            "fake intercepted validators, got {err}"
        );
        assert_eq!(before, content(root)?, "failed validation writes nothing");
        assert_eq!(
            fs::read(root.join(".github/workflows/old.yml"))?,
            b"old: true\n",
            "old tree preserved"
        );

        // Preview generate fails the same way without touching the repo.
        let preview_parent = TempDir::new()?;
        let preview_root = preview_parent.path().join("preview");
        let err = generate(
            &prep,
            &GenerateOptions {
                output_dir: Some(preview_root.clone()),
            },
        )
        .err()
        .ok_or("expected preview validation failure")?;
        assert!(
            matches!(err, OrchestratorError::Validation { .. }),
            "validation error, got {err}"
        );
        assert!(!preview_root.exists(), "no partial preview write");
        assert_eq!(before, content(root)?, "failed preview writes nothing");
        assert_tofu_failure_preserves()?;
        Ok(())
    }

    /// Same failure over tofu paths: locks and sources preserved.
    fn assert_tofu_failure_preserves() -> Result<(), Box<dyn std::error::Error>> {
        let tofu_dir = TempDir::new()?;
        let tofu_root = tofu_dir.path();
        git_init(tofu_root)?;
        fs::create_dir_all(tofu_root.join(".velnor"))?;
        fs::write(
            tofu_root.join(".velnor/config.toml"),
            "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.tofu]\nroots = [\"stacks/a\"]\n",
        )?;
        fs::write(
            tofu_root.join(".velnor/release-manifest.json"),
            manifest_fixture(),
        )?;
        fs::create_dir_all(tofu_root.join("stacks/a"))?;
        fs::write(tofu_root.join("stacks/a/main.tf"), "variable \"x\" {}\n")?;
        fs::write(
            tofu_root.join("stacks/a/.terraform.lock.hcl"),
            "provider \"example.com/a/b\" {\nversion = \"1.0.0\"\n}\n",
        )?;
        let tofu_prep = prepare(tofu_root)?;
        let tofu_before = content(tofu_root)?;
        let err = generate(&tofu_prep, &GenerateOptions { output_dir: None })
            .err()
            .ok_or("expected tofu validation failure")?;
        assert!(
            matches!(err, OrchestratorError::Validation { .. }),
            "validation error, got {err}"
        );
        assert!(
            err.to_string().contains("fake-mise"),
            "fake intercepted tofu validators, got {err}"
        );
        assert_eq!(
            tofu_before,
            content(tofu_root)?,
            "failed tofu validation writes nothing"
        );
        Ok(())
    }

    /// Debug-only consumer-manifest fixture for the scrubbed repo.
    fn manifest_fixture() -> String {
        let version = env!("CARGO_PKG_VERSION");
        let targets = velnor_actions_contract_release::SUPPORTED_TARGETS
        .iter()
        .map(|target| {
            format!(
                "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}\",\"sha256\":\"{}\"}}",
                "a".repeat(64)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
        format!(
            "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{}\",\"targets\":[{targets}]}}",
            "a".repeat(40)
        )
    }

    /// Real `mise` resolved before the fake shadows it.
    fn find_mise() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let path = std::env::var_os("PATH").ok_or("no PATH")?;
        std::env::split_paths(&path)
            .map(|dir| dir.join("mise"))
            .find(|candidate| candidate.is_file())
            .ok_or("mise not on PATH".into())
    }

    /// `PATH` with the fake directory first and the original rest intact.
    fn scrubbed_path(fake_dir: &Path) -> Result<std::ffi::OsString, Box<dyn std::error::Error>> {
        let mut paths = vec![fake_dir.to_path_buf()];
        if let Some(path) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&path));
        }
        Ok(std::env::join_paths(paths)?)
    }

    /// Repository bytes keyed by relative path.
    fn content(root: &Path) -> Result<BTreeMap<String, Vec<u8>>, Box<dyn std::error::Error>> {
        let mut out = BTreeMap::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let mut entries = Vec::new();
            for entry in fs::read_dir(&dir)? {
                entries.push(entry?.path());
            }
            entries.sort();
            for path in entries {
                let meta = fs::symlink_metadata(&path)?;
                if meta.is_dir() && !meta.is_symlink() {
                    stack.push(path);
                } else if meta.is_file() {
                    out.insert(
                        path.strip_prefix(root)?.display().to_string(),
                        fs::read(&path)?,
                    );
                }
            }
        }
        Ok(out)
    }
}

#[cfg(unix)]
#[path = "zizmor_staging.rs"]
mod zizmor_staging;
