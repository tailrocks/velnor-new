use super::{prepare_finalization, validate_source_runner};
use crate::{OrchestratorError, test_support_modules::git_fixture};
use serde_json::{Value, json};
use std::{
    fs,
    process::{Command, Output},
    time::Duration,
};
use velnor_actions_contract::{
    CargoVersionBinding, CargoVersionPolicy, PACKAGE_RELEASE_POLICY_SCHEMA_V1,
    PackageReleasePlanProduct, PackageReleasePolicy,
    config::VerificationRunner,
    package_release::{PackageReleaseChannel, PackageReleasePayloadFormat},
};
use velnor_actions_mise::CheckDeadline;

const CHILD_PROBE: &str = "VELNOR_SCALE_SET_FINALIZATION_CHILD";

#[test]
fn scale_set_source_must_select_linux_amd64() {
    assert!(validate_source_runner(VerificationRunner::LinuxX64, true).is_ok());
    assert!(validate_source_runner(VerificationRunner::MacosArm64, true).is_err());
    assert!(validate_source_runner(VerificationRunner::Macos26Arm64, true).is_err());
    assert!(validate_source_runner(VerificationRunner::Xcode27Arm64, true).is_err());
    assert!(validate_source_runner(VerificationRunner::MacosArm64, false).is_ok());
}

#[test]
fn scale_set_finalization_rejects_non_linux_source_before_outputs() {
    for (runner, expected) in [
        ("macos-arm64", "preview_source_scale_set_runner"),
        ("linux-x64", "release_policy_environment"),
    ] {
        let workspace = source_workspace(runner);
        let source_sha =
            String::from_utf8(git_output(workspace.path(), ["rev-parse", "HEAD"]).stdout)
                .expect("fixture Git output is UTF-8")
                .trim()
                .to_owned();
        let runner_temp = tempfile::tempdir().expect("runner temp");
        let velnor = runner_temp.path().join("velnor");
        fs::create_dir_all(&velnor).expect("runner directory");
        let policy = seed_policy().canonical_bytes().expect("canonical policy");
        fs::write(velnor.join("preview-source-seed-policy.json"), &policy).expect("staged seed");
        let output_path = runner_temp.path().join("output");
        fs::write(&output_path, b"existing output\n").expect("output sentinel");

        let result = Command::new(std::env::current_exe().expect("test executable"))
            .arg("scale_set_finalization_child_probe")
            .arg("--nocapture")
            .env(CHILD_PROBE, expected)
            .env("RUNNER_TEMP", runner_temp.path())
            .env("GITHUB_WORKSPACE", workspace.path())
            .env("GITHUB_OUTPUT", &output_path)
            .env("GITHUB_REPOSITORY", "example/app")
            .env("GITHUB_SHA", "b".repeat(40))
            .env("GITHUB_WORKFLOW_SHA", "b".repeat(40))
            .env("GITHUB_REF", "refs/heads/main")
            .env("GITHUB_WORKFLOW_REF", workflow_ref())
            .env("GITHUB_EVENT_NAME", "workflow_run")
            .env("VELNOR_RELEASE_POLICY", String::from_utf8(policy).unwrap())
            .env(
                "VELNOR_REQUEST_FILE",
                velnor.join("preview-source-seed-policy.json"),
            )
            .env("VELNOR_RELEASE_SOURCE_SHA", source_sha)
            .env("VELNOR_RELEASE_WORKFLOW_AUTHORITY_SHA", "b".repeat(40))
            .env("VELNOR_RELEASE_REQUIRE_LINUX_AMD64", "true")
            .env_remove("VELNOR_RELEASE_CI_RUN_ID")
            .env_remove("VELNOR_RELEASE_CI_RUN_ATTEMPT")
            .env_remove("GITHUB_REPOSITORY_ID")
            .env_remove("GITHUB_RUN_ID")
            .env_remove("GITHUB_RUN_ATTEMPT")
            .output()
            .expect("run production finalization path in child process");
        assert_success(result);
        assert_eq!(
            fs::read(&output_path).expect("output remains readable"),
            b"existing output\n",
            "runner admission must not append output"
        );
        for name in [
            "preview-source-admission.json",
            "package-release-policy.json",
            "package-release-plan.json",
        ] {
            assert!(
                !velnor.join(name).exists(),
                "runner admission must precede writing {name}"
            );
        }
    }
}

#[test]
fn scale_set_finalization_child_probe() {
    let Ok(expected) = std::env::var(CHILD_PROBE) else {
        return;
    };
    let deadline = CheckDeadline::after(Duration::from_secs(30)).expect("deadline");
    let error = prepare_finalization(deadline).err();
    assert!(
        matches!(error, Some(OrchestratorError::Internal { ref problem }) if problem == &expected),
        "expected finalization to stop at {expected}, got {error:?}"
    );
}

fn source_workspace(runner: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("source workspace");
    let velnor = root.path().join(".velnor");
    fs::create_dir_all(&velnor).expect("config directory");
    fs::write(velnor.join("config.toml"), source_config(runner)).expect("source config");
    fs::write(
        root.path().join("Cargo.toml"),
        "[workspace]\nmembers = []\n",
    )
    .expect("manifest");
    fs::write(
        root.path().join("mise.toml"),
        "[tools]\nrust = \"1.98.1\"\n",
    )
    .expect("Mise source");
    git_output(root.path(), ["init"]);
    git_output(root.path(), ["add", "."]);
    git_output(root.path(), ["commit", "-m", "fixture"]);
    root
}

fn source_config(runner: &str) -> String {
    let qualified = json!({
        "qualified_tools": [{
            "id": "rust",
            "version": "1.98.1",
            "backend": {"kind": "core", "tool": "rust"},
            "options": {"kind": "rust", "components": [], "targets": []},
            "depends_on": [],
            "platforms": [
                qualified_platform("linux_x64", "x86_64-unknown-linux-gnu", "a", "b", "c"),
                qualified_platform("macos_arm64", "aarch64-apple-darwin", "d", "e", "f"),
            ]
        }]
    });
    format!(
        "schema = 1\n[workflow]\npolicy = \"consumer-v1\"\n[stacks.rust.binary_release]\nversion_package = \"repo-scan\"\nversion_env = \"REPO_SCAN_VERSION_OVERRIDE\"\n[stacks.rust.binary_release.evidence]\nprofile = \"signed-sbom-v1\"\n[stacks.rust.binary_release.producer]\ntask = \"release-native\"\ntools = [\"rust\"]\ninputs = [\"mise.toml\"]\nrunner = \"{runner}\"\n[[stacks.rust.binary_release.products]]\nid = \"repo-scan\"\npackage = \"repo-scan\"\nbins = [\"repo-scan\"]\ntargets = [\"aarch64-apple-darwin\"]\narchive = \"tar.gz\"\n[stacks.rust.binary_release.preview]\nenabled = true\n{}",
        toml::to_string(&qualified).expect("qualified tool TOML")
    )
}

fn qualified_platform(
    platform: &str,
    target: &str,
    artifact: &str,
    tree: &str,
    executable: &str,
) -> Value {
    json!({
        "platform": platform,
        "artifacts": [{
            "url": format!("https://static.rust-lang.org/dist/rust-1.98.1-{target}.tar.xz"),
            "sha256": artifact.repeat(64)
        }],
        "dependency_artifacts": [],
        "install_tree_sha256": tree.repeat(64),
        "executables": [{
            "name": "rustc",
            "path": "bin/rustc",
            "sha256": executable.repeat(64),
            "probe": {"kind": "version", "expected": "rustc 1.98.1"}
        }]
    })
}

fn seed_policy() -> PackageReleasePolicy {
    PackageReleasePolicy {
        schema: PACKAGE_RELEASE_POLICY_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: "example/app".to_owned(),
        default_branch: "main".to_owned(),
        version: CargoVersionPolicy {
            version_package: "repo-scan".to_owned(),
            bindings: vec![CargoVersionBinding {
                package: "repo-scan".to_owned(),
                manifest_path: "Cargo.toml".to_owned(),
                workspace_manifest_path: "Cargo.toml".to_owned(),
            }],
        },
        products: vec![PackageReleasePlanProduct {
            id: "repo-scan".to_owned(),
            package: "repo-scan".to_owned(),
            bins: vec!["repo-scan".to_owned()],
            targets: vec!["aarch64-apple-darwin".to_owned()],
            format: PackageReleasePayloadFormat::TarGz,
        }],
        companions: Vec::new(),
    }
}

fn workflow_ref() -> String {
    format!(
        "example/app/{}@refs/heads/main",
        PackageReleaseChannel::Preview.workflow_path()
    )
}

fn git_output<const N: usize>(root: &std::path::Path, args: [&str; N]) -> Output {
    let output = git_fixture::sterile_command()
        .current_dir(root)
        .args(args)
        .output()
        .expect("run fixture Git");
    assert!(
        output.status.success(),
        "Git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "child finalization probe failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
