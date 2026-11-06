// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate as velnor_actions_orchestrator;

fn test_git() -> crate::crate_release_git::ReleaseGit {
    crate::crate_release_git::ReleaseGit::host_fixture()
}

use std::{fs, path::Path, process::Command};
use velnor_actions_contract::config::CrateReleaseIntent;
use velnor_actions_orchestrator::{
    OrchestratorError,
    crate_release_registry::{CrateRegistryRead, CrateRegistryResponse},
    crate_release_workspace_plan_with_registry,
};
struct NoRegistry;
impl CrateRegistryRead for NoRegistry {
    fn get(&self, _path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
        Err(velnor_actions_orchestrator::OrchestratorError::Inventory {
            problem: "workspace has no external prerequisites".into(),
        })
    }
}

fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
fn fixture(
    manifest: &str,
) -> Result<(tempfile::TempDir, String, CrateReleaseIntent), Box<dyn std::error::Error>> {
    fixture_with_version(manifest, "0.1.1")
}
fn fixture_with_version(
    manifest: &str,
    version: &str,
) -> Result<(tempfile::TempDir, String, CrateReleaseIntent), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    git(root, &["init", "-q"])?;
    git(root, &["config", "user.name", "Alexey Zhokhov"])?;
    git(root, &["config", "user.email", "alexey@zhokhov.com"])?;
    git(root, &["config", "commit.gpgsign", "false"])?;
    git(root, &["config", "core.hooksPath", "/dev/null"])?;
    fs::create_dir(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub fn demo() {}")?;
    fs::write(root.join("Cargo.toml"), manifest)?;
    fs::write(
        root.join("Cargo.lock"),
        format!("version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"{version}\"\n"),
    )?;
    git(root, &["add", "."])?;
    git(
        root,
        &[
            "commit",
            "-q",
            "-s",
            "-m",
            "fixture",
            "-m",
            "Co-authored-by: Codex <codex@openai.com>",
        ],
    )?;
    let sha = git(root, &["rev-parse", "HEAD"])?;
    let catalog = velnor_actions_mise::ToolCatalog::pinned();
    let rust = catalog.version(velnor_actions_mise::PinnedTool::Rust);
    let intent = serde_json::from_value(
        serde_json::json!({"schema":1,"repository":"example/demo","default_branch":"main","environment":"release","required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":139_017,"generator_version":"0.1.72","generator_source_sha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","generator_manifest_sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","release_plz_version":"0.4.3","runner":"ubuntu-26.04","authentication":"trusted-publishing","rust_version":rust,"manifest_path":"Cargo.toml","pr_write_paths":["CHANGELOG.md","Cargo.lock","Cargo.toml"],"registry":"crates-io","packages":["demo"],"publish_features":{"demo":[]},"tag_template":"{{ package }}-v{{ version }}","recovery":null}),
    )?;
    Ok((temp, sha, intent))
}
#[test]
fn workspace_plan_binds_source_and_recovery() -> Result<(), Box<dyn std::error::Error>> {
    let (temp, sha, mut intent) =
        fixture("[package]\nname = \"demo\"\nversion = \"0.1.1\"\nedition = \"2021\"\n")?;
    let result = crate_release_workspace_plan_with_registry(
        temp.path(),
        &sha,
        &intent,
        &NoRegistry,
        &test_git(),
        &crate::UnprivilegedSourceExecution,
    )?;
    assert_eq!(result.plan.graph.order, vec!["demo"]);
    assert_eq!(result.plan.selection.packages[0].version, "0.1.1");
    intent.recovery = Some(serde_json::from_value(
        serde_json::json!({"package":"demo","version":"0.1.2","source_sha":sha}),
    )?);
    assert!(
        crate_release_workspace_plan_with_registry(
            temp.path(),
            &sha,
            &intent,
            &NoRegistry,
            &test_git(),
            &crate::UnprivilegedSourceExecution,
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn workspace_members_and_project_configuration_are_bound_without_running_project_code()
-> Result<(), Box<dyn std::error::Error>> {
    let (temp, _, mut intent) = fixture("[workspace]\nmembers = [\"a\"]\nresolver = \"2\"\n")?;
    let root = temp.path();
    for (name, dependencies) in [
        (
            "a",
            "[dependencies]\nb = { path = \"../b\", version = \"0.1.0\" }\n",
        ),
        ("b", ""),
    ] {
        fs::create_dir_all(root.join(name).join("src"))?;
        fs::write(root.join(name).join("src/lib.rs"), "pub fn demo() {}")?;
        fs::write(
            root.join(name).join("Cargo.toml"),
            format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{dependencies}"
            ),
        )?;
    }
    fs::write(
        root.join("a/build.rs"),
        "fn main() { panic!(\"build script must not run\"); }",
    )?;
    fs::create_dir(root.join(".cargo"))?;
    fs::write(
        root.join(".cargo/config.toml"),
        "[build]\nrustc-wrapper = \"/bin/false\"\n[registry]\nglobal-credential-providers = [\"/bin/false\"]\n[net]\ngit-fetch-with-cli = true\n",
    )?;
    fs::remove_file(root.join("Cargo.lock"))?;
    git(root, &["add", "-A"])?;
    git(
        root,
        &[
            "commit",
            "-q",
            "-s",
            "-m",
            "workspace",
            "-m",
            "Co-authored-by: Codex <codex@openai.com>",
        ],
    )?;
    let sha = git(root, &["rev-parse", "HEAD"])?;
    intent.packages = vec!["a".into(), "b".into()];
    intent.publish_features = [("a".into(), vec![]), ("b".into(), vec![])].into();
    let plan = crate_release_workspace_plan_with_registry(
        root,
        &sha,
        &intent,
        &NoRegistry,
        &test_git(),
        &crate::UnprivilegedSourceExecution,
    )?;
    assert_eq!(plan.plan.graph.order, vec!["b", "a"]);
    fs::write(
        root.join(".cargo/config.toml"),
        "[build]\nrustc-wrapper = \"/bin/true\"\n",
    )?;
    assert!(
        crate_release_workspace_plan_with_registry(
            root,
            &sha,
            &intent,
            &NoRegistry,
            &test_git(),
            &crate::UnprivilegedSourceExecution,
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn workspace_plan_fetches_registry_prerequisites_and_rejects_yanked_or_denied()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::{
        OrchestratorError,
        crate_release_registry::{CrateRegistryRead, CrateRegistryResponse},
        crate_release_workspace_plan_with_registry,
    };
    struct Registry {
        status: u16,
        yanked: bool,
    }
    impl CrateRegistryRead for Registry {
        fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
            assert_eq!(path, "/api/v1/crates/serde");
            let body = serde_json::json!({"crate":{"id":"serde","name":"serde","versions":[1]},"versions":[{"id":1,"crate":"serde","num":"1.0.0","yanked":self.yanked}]}).to_string().into_bytes();
            Ok(CrateRegistryResponse {
                status: self.status,
                body,
            })
        }
    }
    let (temp, sha, intent) = fixture(
        "[package]\nname = \"demo\"\nversion = \"0.1.1\"\nedition = \"2021\"\n[dependencies]\nserde = \"=1.0.0\"\n",
    )?;
    let result = crate_release_workspace_plan_with_registry(
        temp.path(),
        &sha,
        &intent,
        &Registry {
            status: 200,
            yanked: false,
        },
        &test_git(),
        &crate::UnprivilegedSourceExecution,
    )?;
    assert_eq!(result.registry_evidence.len(), 1);
    assert_eq!(result.registry_evidence[0].package, "serde");
    assert_eq!(result.registry_evidence[0].versions["1.0.0"].id, 1);
    assert!(
        crate_release_workspace_plan_with_registry(
            temp.path(),
            &sha,
            &intent,
            &Registry {
                status: 200,
                yanked: true
            },
            &test_git(),
            &crate::UnprivilegedSourceExecution,
        )
        .is_err()
    );
    for status in [403, 404, 429, 500] {
        assert!(
            crate_release_workspace_plan_with_registry(
                temp.path(),
                &sha,
                &intent,
                &Registry {
                    status,
                    yanked: false
                },
                &test_git(),
                &crate::UnprivilegedSourceExecution,
            )
            .is_err(),
            "registry status {status} must not satisfy a dependency"
        );
    }
    Ok(())
}
