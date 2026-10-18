//! Native preview generation keeps stable authorization independent.
use super::{ENABLED, binary_repo};
use crate::impl_common::{TestResult, git};
use velnor_actions_orchestrator::{prepare, render_staged_tree};

fn preview_config(stable: bool) -> Result<String, Box<dyn std::error::Error>> {
    let tool = serde_json::json!({"qualified_tools":[{
        "id":"rust","version":"1.98.1","backend":{"kind":"core","tool":"rust"},
        "options":{"kind":"rust","components":[],"targets":[]},"depends_on":[],
        "platforms":[{"platform":"linux_x64","artifacts":[{"url":"https://static.rust-lang.org/dist/rust-1.98.1-x86_64-unknown-linux-gnu.tar.xz","sha256":"a".repeat(64)}],"dependency_artifacts":[],"install_tree_sha256":"b".repeat(64),
            "executables":[{"name":"rustc","path":"bin/rustc","sha256":"c".repeat(64),"probe":{"kind":"version","expected":"rustc 1.98.1"}}]}]
    }]});
    let stable_config = if stable {
        ENABLED.to_owned()
    } else {
        ENABLED.replacen("enabled = true", "enabled = false", 1)
    };
    Ok(format!(
        "{stable_config}\n{}\n{}",
        r#"
[stacks.rust.binary_release.preview]
enabled = true
version_env = "DEMO_VERSION_OVERRIDE"
[stacks.rust.binary_release.preview.evidence]
profile = "signed-sbom-v1"
[stacks.rust.binary_release.preview.producer]
task = "release-preview"
tools = ["rust"]
inputs = ["mise.toml", "Cargo.toml", "Cargo.lock", "src/main.rs"]
[[stacks.rust.binary_release.preview.products]]
id = "demo"
package = "demo"
bins = ["demo"]
targets = ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"]
archive = "tar.gz"
"#,
        toml::to_string(&tool)?
    ))
}

#[test]
fn preview_generates_independently_with_sorted_static_inventory() -> TestResult {
    for stable in [false, true] {
        let repo = binary_repo(&preview_config(stable)?)?;
        std::fs::write(
            repo.path().join("mise.toml"),
            "[tools]\nrust = '1.98.1'\n[tasks.release-preview]\nrun = 'true'\n",
        )?;
        git(&["add", "mise.toml"], repo.path())?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        let preview = tree
            .get(".github/workflows/binary-preview.yml")
            .ok_or("preview workflow missing")?;
        assert!(preview.contains("execute-preview-producer-v1"));
        assert!(preview.contains("sign-package-release-v1"));
        assert!(preview.contains("publish-package-release-v1"));
        assert!(preview.contains("preview-publish"));
        assert_eq!(
            tree.get(".github/workflows/binary-release.yml").is_some(),
            stable
        );
    }
    Ok(())
}

#[test]
fn preview_refuses_unknown_binary_before_emitting_publication_workflow() -> TestResult {
    let config = preview_config(false)?.replace("bins = [\"demo\"]", "bins = [\"missing\"]");
    let repo = binary_repo(&config)?;
    let error = render_staged_tree(&prepare(repo.path())?)
        .err()
        .ok_or("missing binary accepted")?;
    assert!(error.to_string().contains("preview"));
    Ok(())
}
