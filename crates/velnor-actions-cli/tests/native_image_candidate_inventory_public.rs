// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(unix)]
mod unix {
    use std::{
        error::Error,
        fs,
        os::unix::fs::symlink,
        path::{Path, PathBuf},
        process::{Command, Output},
    };

    use serde_json::json;
    use tempfile::TempDir;

    const TARGET: &str = "x86_64-unknown-linux-gnu";
    const PACKAGE: &str = "path+file:///fixture/crates/velnor-actions-cli#velnor-actions-cli@0.1.0";
    const PACKAGE_ROOT: &str = "crates/velnor-actions-cli";
    const DEPENDENCY: &str = "crates/velnor-actions-cli/src/main.rs";
    const SOURCE_SYMLINK: &str = "crates/velnor-actions-cli/src/input.rs";
    const INVENTORY: &str =
        "crates/velnor-actions-contract/src/image_candidate/closure_inventory.txt";
    const SUPPLEMENTAL: &str =
        "crates/velnor-actions-contract/src/image_candidate/closure_supplemental.txt";
    const GENERATOR: &str = "scripts/native-image-candidate-inventory.py";

    struct Fixture {
        _directory: TempDir,
        root: PathBuf,
        target: PathBuf,
        metadata: PathBuf,
        artifacts: PathBuf,
        expected: Vec<String>,
    }

    impl Fixture {
        fn new() -> Result<Self, Box<dyn Error>> {
            let directory = TempDir::new()?;
            let base = directory.path().canonicalize()?;
            let root = base.join("checkout");
            let target = base.join("mbx-target");
            let target_root = root.join("target");
            fs::create_dir_all(root.join(format!("{PACKAGE_ROOT}/src")))?;
            fs::create_dir_all(&target)?;
            symlink(&target, &target_root)?;
            write(&root, ".gitignore", "/target\n")?;

            let package_manifest = format!("{PACKAGE_ROOT}/Cargo.toml");
            write(&root, "Cargo.toml", "[workspace]\nmembers = []\n")?;
            write(&root, "Cargo.lock", "version = 4\n")?;
            write(
                &root,
                &package_manifest,
                "[package]\nname = \"velnor-actions-cli\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            )?;
            write(&root, DEPENDENCY, "pub fn fixture() {}\n")?;
            write(&root, &format!("{PACKAGE_ROOT}/build.rs"), "fn main() {}\n")?;
            write(&root, GENERATOR, "fixture generator\n")?;
            write(&root, INVENTORY, "")?;

            let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let supplemental = fs::read_to_string(project_root.join(
                "crates/velnor-actions-contract/src/image_candidate/closure_supplemental.txt",
            ))?;
            write(&root, SUPPLEMENTAL, &supplemental)?;
            for relative in supplemental.lines() {
                write(&root, relative, "fixture supplemental input\n")?;
            }
            let mut expected = vec![
                ".gitignore".to_owned(),
                "Cargo.lock".to_owned(),
                "Cargo.toml".to_owned(),
                package_manifest,
                DEPENDENCY.to_owned(),
                format!("{PACKAGE_ROOT}/build.rs"),
                INVENTORY.to_owned(),
                SUPPLEMENTAL.to_owned(),
                GENERATOR.to_owned(),
            ];
            expected.extend(supplemental.lines().map(str::to_owned));
            expected.sort_unstable();
            write(&root, INVENTORY, &format!("{}\n", expected.join("\n")))?;

            let (metadata, artifacts) = write_artifacts(&base, &root, &target)?;

            git(&root, &["init", "--quiet"])?;
            git(&root, &["config", "user.name", "Inventory fixture"])?;
            git(
                &root,
                &["config", "user.email", "inventory@example.invalid"],
            )?;
            git(&root, &["config", "commit.gpgsign", "false"])?;
            git(&root, &["add", "--all"])?;
            git(&root, &["commit", "--quiet", "-m", "inventory fixture"])?;

            Ok(Self {
                _directory: directory,
                root,
                target,
                metadata,
                artifacts,
                expected,
            })
        }

        fn check(&self) -> Result<Output, Box<dyn Error>> {
            let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            Ok(Command::new("python3")
                .arg(project_root.join(GENERATOR))
                .args(["--check", "--root"])
                .arg(&self.root)
                .arg("--metadata")
                .arg(&self.metadata)
                .arg("--artifacts")
                .arg(&self.artifacts)
                .args(["--target", TARGET])
                .output()?)
        }

        fn assert_expected_inventory(&self) -> Result<(), Box<dyn Error>> {
            let actual = fs::read_to_string(self.root.join(INVENTORY))?;
            assert_eq!(actual, format!("{}\n", self.expected.join("\n")));
            Ok(())
        }
    }

    fn write_artifacts(
        base: &Path,
        root: &Path,
        target: &Path,
    ) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
        let target_root = root.join("target");
        let release_deps = target.join("release/deps");
        fs::create_dir_all(&release_deps)?;
        let rlib = root.join("target/release/deps/libvelnor_actions_cli-fixture.rlib");
        fs::write(&rlib, b"fixture artifact")?;
        let rlib_dep = release_deps.join("velnor_actions_cli-fixture.d");
        fs::write(
            rlib_dep,
            format!(
                "{}: {} {} {}\n",
                rlib.display(),
                root.join("target/release/deps/metadata.rmeta").display(),
                root.join(DEPENDENCY).display(),
                root.display()
            ),
        )?;

        let build_script = target.join("release/build/velnor-actions-cli-abc");
        fs::create_dir_all(&build_script)?;
        let build_executable =
            root.join("target/release/build/velnor-actions-cli-abc/build-script-build");
        fs::write(&build_executable, b"fixture build script")?;
        fs::write(
            build_script.join("build_script_build-abc.d"),
            format!(
                "{}: {}\n",
                build_executable.display(),
                root.join(format!("{PACKAGE_ROOT}/build.rs")).display()
            ),
        )?;
        let build_output = target.join("release/build/velnor-actions-cli-output/output");
        fs::create_dir_all(build_output.parent().ok_or("build output has no parent")?)?;
        fs::write(
            &build_output,
            format!("cargo:rerun-if-changed={PACKAGE_ROOT}/src\n"),
        )?;

        let metadata = base.join("metadata.json");
        let artifacts = base.join("artifacts.jsonl");
        let manifest_path = root.join(format!("{PACKAGE_ROOT}/Cargo.toml"));
        let metadata_value = json!({
            "workspace_root": root,
            "target_directory": target_root,
            "packages": [{
                "id": PACKAGE,
                "name": "velnor-actions-cli",
                "manifest_path": manifest_path,
                "source": null
            }],
            "resolve": {"nodes": [{"id": PACKAGE, "deps": []}]}
        });
        fs::write(&metadata, serde_json::to_vec(&metadata_value)?)?;
        let artifacts_value = [
            json!({
                "reason": "compiler-artifact",
                "package_id": PACKAGE,
                "target": {"name": "velnor-actions-cli", "kind": ["rlib"]},
                "filenames": [rlib]
            }),
            json!({
                "reason": "compiler-artifact",
                "package_id": PACKAGE,
                "target": {"name": "build-script-build", "kind": ["custom-build"]},
                "filenames": [build_executable]
            }),
            json!({
                "reason": "build-script-executed",
                "package_id": PACKAGE,
                "out_dir": target.join("release/build/velnor-actions-cli-output/out")
            }),
        ];
        let lines = artifacts_value
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()?;
        fs::write(&artifacts, lines.join("\n") + "\n")?;

        Ok((metadata, artifacts))
    }

    fn write(root: &Path, relative: &str, contents: &str) -> Result<(), Box<dyn Error>> {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().ok_or("fixture file has no parent")?)?;
        fs::write(path, contents)?;
        Ok(())
    }

    fn git(root: &Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
        let path = std::env::var_os("PATH").ok_or("PATH is unavailable")?;
        let output = Command::new("git")
            .env_clear()
            .env("PATH", path)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .current_dir(root)
            .args(args)
            .output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into())
        }
    }

    #[test]
    fn excludes_target_symlink_outputs_but_rejects_source_symlinks() -> Result<(), Box<dyn Error>> {
        let fixture = Fixture::new()?;
        fixture.assert_expected_inventory()?;
        let positive = fixture.check()?;
        assert!(
            positive.status.success(),
            "target output symlink made the source inventory fail: {}",
            String::from_utf8_lossy(&positive.stderr)
        );

        let target_source = fixture.target.join("release/deps/metadata.rmeta");
        fs::write(&target_source, b"generated metadata")?;
        fs::write(
            fixture
                .target
                .join("release/build/velnor-actions-cli-output/output"),
            format!("cargo:rerun-if-changed={PACKAGE_ROOT}/build.rs\n"),
        )?;
        let source_link = fixture.root.join(SOURCE_SYMLINK);
        symlink(target_source, source_link)?;
        git(&fixture.root, &["add", "--all"])?;
        fs::write(
            fixture
                .target
                .join("release/deps/velnor_actions_cli-fixture.d"),
            format!(
                "{}: {} {} {}\n",
                fixture
                    .root
                    .join("target/release/deps/libvelnor_actions_cli-fixture.rlib")
                    .display(),
                fixture
                    .root
                    .join("target/release/deps/metadata.rmeta")
                    .display(),
                fixture.root.join(DEPENDENCY).display(),
                fixture.root.join(SOURCE_SYMLINK).display()
            ),
        )?;
        let negative = fixture.check()?;
        assert!(!negative.status.success());
        assert!(
            String::from_utf8_lossy(&negative.stderr).contains(
                "compiler input traverses a symlink: crates/velnor-actions-cli/src/input.rs"
            ),
            "source symlink was not rejected at the workspace boundary: {}",
            String::from_utf8_lossy(&negative.stderr)
        );
        Ok(())
    }
}
