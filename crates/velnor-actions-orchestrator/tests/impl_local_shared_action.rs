//! Local shared actions must pass pinned jactionlint.
use std::fs;

use velnor_actions_orchestrator::{
    ExecutionMode, GenerateOptions, generate, generate_dispatched, prepare, render_staged_tree_with,
};

use crate::impl_common::{TestResult, git, make_repo};

fn both_config() -> &'static str {
    "schema = 2\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\nmode = \"both\"\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"ubuntu-26.04-scale-set\", \"velnor\"]\nplatform = \"linux/amd64\"\njob_container_image = \"ghcr.io/team/job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n"
}

fn committed_repo() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let repo = make_repo(both_config())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "fixture"], root)?;
    Ok(repo)
}

#[test]
fn both_mode_generate_passes_jactionlint() -> TestResult {
    let repo = committed_repo()?;
    let prep = prepare(repo.path())?;
    let preview = tempfile::TempDir::new()?;
    let report = generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview.path().to_path_buf()),
        },
    )?;
    assert_eq!(
        report.validated_by,
        vec![
            "jactionlint@2.3.0".to_owned(),
            "shellcheck@0.11.0".to_owned(),
        ]
    );
    let ci = fs::read_to_string(preview.path().join(".github/workflows/ci.yml"))?;
    let actionlint = fs::read_to_string(preview.path().join(".github/jactionlint.yaml"))?;
    assert!(ci.contains("uses: ./.github/actions/"), "{ci}");
    assert!(!ci.contains("uses: $"), "{ci}");
    assert!(ci.len() <= 500_000, "ci.yml is {} bytes", ci.len());
    assert!(!actionlint.contains("paths:"), "{actionlint}");
    assert!(!actionlint.contains("shellcheck"), "{actionlint}");
    let action_dir = preview.path().join(".github/actions");
    let mut manifests = 0;
    for entry in fs::read_dir(action_dir)? {
        if entry?.path().join("action.yml").is_file() {
            manifests += 1;
        }
    }
    assert!(
        manifests > 0,
        "shared local action manifests were not emitted"
    );
    let hosted_dir = tempfile::TempDir::new()?;
    let hosted = generate_dispatched(
        &prep,
        &GenerateOptions {
            output_dir: Some(hosted_dir.path().to_path_buf()),
        },
        Some(ExecutionMode::Hosted),
    )?;
    assert!(
        hosted
            .validated_by
            .iter()
            .any(|spec| spec == "jactionlint@2.3.0")
    );
    let hosted_lint = fs::read_to_string(hosted_dir.path().join(".github/jactionlint.yaml"))?;
    let hosted_ci = fs::read_to_string(hosted_dir.path().join(".github/workflows/ci.yml"))?;
    assert!(!hosted_lint.contains("paths:"), "{hosted_lint}");
    assert!(!hosted_ci.contains("uses: $"), "{hosted_ci}");
    Ok(())
}

#[test]
fn scale_set_mode_generate_passes_jactionlint() -> TestResult {
    let repo = committed_repo()?;
    let prep = prepare(repo.path())?;
    let preview = tempfile::TempDir::new()?;
    let report = generate_dispatched(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview.path().to_path_buf()),
        },
        Some(ExecutionMode::ScaleSet),
    )?;
    assert_eq!(
        report.validated_by,
        vec![
            "jactionlint@2.3.0".to_owned(),
            "shellcheck@0.11.0".to_owned(),
        ]
    );
    let ci = fs::read_to_string(preview.path().join(".github/workflows/ci.yml"))?;
    assert!(
        ci.contains("runs-on: [velnor, ubuntu-26.04-scale-set]"),
        "{ci}"
    );
    assert!(!ci.contains("runs-on: ubuntu-26.04\n"), "{ci}");
    Ok(())
}

#[test]
fn configured_and_dispatched_modes_route_every_ci_job_for_all_policies() -> TestResult {
    let mut failures = Vec::new();
    for policy in ["consumer", "velnor-locked", "velnor-preseed"] {
        let repo = mode_fixture(policy)?;
        let prep = prepare(repo.path())?;
        for mode in [ExecutionMode::Both, ExecutionMode::ScaleSet] {
            let tree = match render_staged_tree_with(&prep, Some(mode)) {
                Ok(tree) => tree,
                Err(error) => {
                    failures.push(format!("{policy}:{mode:?}:{error}"));
                    continue;
                }
            };
            let repeated = render_staged_tree_with(&prep, Some(mode))?;
            if tree != repeated {
                failures.push(format!(
                    "{policy}:{mode:?}:generated tree differs across renders"
                ));
            }
            let Some(ci) = tree.get(".github/workflows/ci.yml") else {
                failures.push(format!("{policy}:{mode:?}:missing ci.yml"));
                continue;
            };
            let support_id = if policy == "consumer" {
                "jactionlint"
            } else {
                "alint"
            };
            let Some(support_runner) = job_runner(ci, support_id) else {
                failures.push(format!(
                    "{policy}:{mode:?}:missing {support_id} runner\n{ci}"
                ));
                continue;
            };
            if mode == ExecutionMode::ScaleSet {
                if support_runner != "runs-on: [velnor, ubuntu-26.04-scale-set]" {
                    failures.push(format!("{policy}:{mode:?}:{support_id}:{support_runner}"));
                }
                if ci.contains("runs-on: ubuntu-26.04\n") {
                    failures.push(format!("{policy}:{mode:?}:hosted job in Scale Set CI"));
                }
            } else if !ci.contains("runs-on: ubuntu-26.04\n") {
                failures.push(format!("{policy}:{mode:?}:Both lost hosted lane"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

fn mode_fixture(policy: &str) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let config = match policy {
        "consumer" => both_config().to_owned(),
        "velnor-locked" | "velnor-preseed" => both_config().replace(
            "name = \"CI\"",
            "name = \"CI\"\npolicy = \"velnor-repository-v1\"",
        ),
        _ => {
            return Err(std::io::Error::other(format!("unknown policy {policy}")).into());
        }
    };
    let repo = make_repo(&config)?;
    let root = repo.path();
    if policy != "consumer" {
        git(
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/tailrocks/velnor-new.git",
            ],
            root,
        )?;
    }
    if policy == "velnor-locked" {
        write_three_target_lock(root)?;
    }
    git(&["add", "."], root)?;
    git(&["commit", "-m", "fixture"], root)?;
    Ok(repo)
}

fn write_three_target_lock(root: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::fmt::Write as _;
    let mut binaries = String::new();
    for target in velnor_actions_contract::SUPPORTED_TARGETS {
        writeln!(
            binaries,
            "[[generator.binaries]]\ntarget = \"{target}\"\nartifact = \"https://example.invalid/r/{target}\"\nsha256 = \"{}\"",
            "a".repeat(64)
        )?;
    }
    let lock = format!(
        "schema = 1\n[generator]\nbinary = \"velnor-actions\"\nversion = \"{}\"\ncommit = \"{}\"\n{binaries}[mise-bootstrap]\nversion = \"2026.10.13\"\nartifact = \"https://example.invalid/mise\"\nsha256 = \"{}\"\n",
        env!("CARGO_PKG_VERSION"),
        "e".repeat(40),
        "b".repeat(64)
    );
    fs::write(root.join(".velnor/generator.lock"), lock)?;
    Ok(())
}

fn job_runner<'a>(yaml: &'a str, id: &str) -> Option<&'a str> {
    let jobs = yaml.split_once("jobs:\n")?.1;
    let marker = format!("  {id}:\n");
    let body = jobs.split_once(&marker)?.1;
    body.lines()
        .take_while(|line| line.is_empty() || line.starts_with("    "))
        .find(|line| line.starts_with("    runs-on:"))
        .map(str::trim)
}
