use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use super::super::scoped_workspace::TestResult;

const SOURCE_ROOTS: [&str; 3] = [
    "crates/velnor-actions-cli/src",
    "crates/velnor-actions-freshness/src",
    "crates/velnor-archive-guard/src",
];

pub(super) fn assert_dedicated_document_directories(root: &Path) -> TestResult {
    for (directory, expected) in [
        (
            "docs/implemented/ci-maintenance",
            "docs/implemented/ci-maintenance/repo-verification.md",
        ),
        (
            "docs/proposed/ci-maintenance",
            "docs/proposed/ci-maintenance/rust-quality-contract.md",
        ),
    ] {
        let output = Command::new("git")
            .args(["ls-files", "-z", "--", &format!("{directory}/")])
            .current_dir(root)
            .output()?;
        assert!(
            output.status.success(),
            "git ls-files failed for {directory}"
        );
        let files = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .map(std::str::from_utf8)
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(files, [expected]);
    }
    Ok(())
}

pub(super) fn assert_source_closure(root: &Path, inputs: &BTreeSet<String>) -> TestResult {
    let output = Command::new("git")
        .args(["ls-files", "--"])
        .args(SOURCE_ROOTS)
        .current_dir(root)
        .output()?;
    assert!(output.status.success(), "git ls-files failed");
    let tracked = String::from_utf8(output.stdout)?;
    let missing: Vec<&str> = tracked
        .lines()
        .filter(|path| Path::new(path).extension().is_some_and(|ext| ext == "rs"))
        .filter(|path| !inputs.contains(*path))
        .collect();
    assert!(
        missing.is_empty(),
        "maintenance-helpers omits source files: {missing:?}"
    );
    Ok(())
}

pub(super) fn assert_metadata_input_closures(root: &Path) -> TestResult {
    let source = std::fs::read_to_string(root.join(".velnor/config.toml"))?;
    let config: toml::Value = toml::from_str(&source)?;
    let rules = config["stacks"]["rust"]["inputs"]
        .as_array()
        .ok_or("missing Rust input rules")?;
    for package in [
        "velnor-actions-freshness",
        "velnor-actions-mise",
        "velnor-actions-orchestrator",
    ] {
        let globs = rules
            .iter()
            .filter(|rule| {
                rule["packages"].as_array().is_some_and(|packages| {
                    packages.iter().any(|name| name.as_str() == Some(package))
                })
            })
            .flat_map(|rule| rule["globs"].as_array().into_iter().flatten())
            .filter_map(toml::Value::as_str)
            .collect::<Vec<_>>();
        assert!(!globs.is_empty(), "missing complete inputs for {package}");
        assert_workspace_metadata(root, &globs)?;
        assert_package_inputs(root, package, &globs)?;
    }
    for package in [
        "velnor-actions-rust",
        "velnor-runner-attestation",
        "velnor-actions-workflow-renderer",
        "velnor-archive-guard",
    ] {
        let globs = rules
            .iter()
            .filter(|rule| {
                rule["packages"].as_array().is_some_and(|packages| {
                    packages.iter().any(|name| name.as_str() == Some(package))
                })
            })
            .flat_map(|rule| rule["globs"].as_array().into_iter().flatten())
            .filter_map(toml::Value::as_str)
            .collect::<Vec<_>>();
        assert!(!globs.is_empty(), "missing complete inputs for {package}");
        assert_package_inputs(root, package, &globs)?;
    }
    Ok(())
}

fn assert_workspace_metadata(root: &Path, globs: &[&str]) -> TestResult {
    for manifest in ["Cargo.toml", "crates/velnor-runner/Cargo.toml"] {
        assert_metadata_path_matches(globs, manifest);
        let lock = manifest
            .strip_suffix("Cargo.toml")
            .unwrap_or_default()
            .to_owned()
            + "Cargo.lock";
        assert_metadata_path_matches(globs, &lock);
        let output = Command::new("cargo")
            .args([
                "metadata",
                "--locked",
                "--no-deps",
                "--format-version",
                "1",
                "--manifest-path",
                manifest,
            ])
            .current_dir(root)
            .output()?;
        assert!(
            output.status.success(),
            "cargo metadata failed for {manifest}"
        );
        let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        for package in metadata["packages"]
            .as_array()
            .ok_or("missing metadata packages")?
        {
            let manifest = Path::new(
                package["manifest_path"]
                    .as_str()
                    .ok_or("missing metadata manifest path")?,
            );
            assert_metadata_path_matches(globs, &manifest.strip_prefix(root)?.to_string_lossy());
            for target in package["targets"]
                .as_array()
                .ok_or("missing metadata targets")?
            {
                let path = Path::new(
                    target["src_path"]
                        .as_str()
                        .ok_or("missing target source path")?,
                );
                assert_metadata_path_matches(globs, &path.strip_prefix(root)?.to_string_lossy());
            }
        }
    }
    Ok(())
}

fn assert_package_inputs(root: &Path, package: &str, globs: &[&str]) -> TestResult {
    let paths: &[&str] = match package {
        "velnor-actions-freshness" => &[
            ".velnor/freshness-inventory.json",
            ".velnor/version-policy.toml",
            ".cargo/mutants.toml",
            "deny.toml",
        ],
        "velnor-actions-rust" => &["crates/velnor-actions-rust/src/lib.rs"],
        "velnor-runner-attestation" => &[
            "crates/velnor-runner/Cargo.toml",
            "crates/velnor-runner/Cargo.lock",
            "crates/velnor-runner/crates/velnor-runner-attestation/src/main.rs",
            "crates/velnor-runner/crates/velnor-runner-attestation/src/sigstore-production-tuf-root.json",
        ],
        "velnor-actions-workflow-renderer" => &[
            ".github/workflows/ci.yml",
            ".velnor/config.toml",
            "scripts/generator-release/preflight-release-tag.sh",
            "scripts/generator-release/create-release-manifest.sh",
            "scripts/upgrades/bump-gradle-catalog.py",
            "scripts/derive-rust-tree-digest",
            "LICENSE",
            "NOTICE",
        ],
        "velnor-actions-orchestrator" => &[
            ".velnor/config.toml",
            ".velnor/controller-approvals.json",
            ".velnor/generator.lock",
            ".velnor/qualification/homebrew/jackin/transition.json",
            ".velnor/qualification/homebrew/preview-plan.json",
            ".velnor/qualification/homebrew/formula-spec.json",
            ".velnor/qualification/homebrew/preview-a.json",
            ".velnor/qualification/homebrew/preview-d.json",
            ".velnor/qualification/homebrew/preview-a-source-admission.json",
            ".velnor/qualification/homebrew/jackin/cutover-author-policy.json",
            "fixtures/bun-install-lifecycle-runtime/legacy-install-step.sh",
            "fixtures/bun-workspace-suites/input/.velnor/config.toml",
            "mise.toml",
            "mise.lock",
            "scripts/generator-release/create-release-manifest.sh",
            "scripts/generator-release/source-qualification-validator-recipes.json",
        ],
        "velnor-archive-guard" => &[
            ".config",
            ".github",
            ".velnor",
            "crates/velnor-actions-cli/src/main.rs",
            "fixtures",
            "images",
            "scripts/archive-guard-inputs.txt",
            "docs/proposed/opentofu-goldens/cases/dogfood/plan.txt",
            ".gitignore",
            ".mise-version",
            "Cargo.lock",
            "Cargo.toml",
            "mise.lock",
            "mise.toml",
            "rust-toolchain.toml",
        ],
        _ => &[],
    };
    for path in paths {
        assert!(
            root.join(path).exists(),
            "missing observed metadata input {path}"
        );
        assert_metadata_path_matches(globs, path);
    }
    if package == "velnor-archive-guard" {
        for line in std::fs::read_to_string(root.join("scripts/archive-guard-inputs.txt"))?.lines()
        {
            let (_, path) = line.split_once(' ').ok_or("invalid archive guard input")?;
            assert_metadata_path_matches(globs, path);
        }
    }
    Ok(())
}

fn assert_metadata_path_matches(globs: &[&str], path: &str) {
    assert!(
        globs
            .iter()
            .any(|glob| velnor_actions_contract::matches_glob(glob, path)),
        "no complete metadata input rule matches {path}"
    );
}
