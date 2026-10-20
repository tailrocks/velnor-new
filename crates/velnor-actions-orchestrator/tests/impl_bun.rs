//! `[stacks.bun]` member jobs through the real generator.

use std::fs;
use std::path::Path;

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, git, make_repo};
use crate::impl_schema2_routing::{job_body, required_file};

pub(crate) const ROOT_PACKAGE: &str = r#"{"name":"frontend","private":true,"packageManager":"bun@1.4.2","workspaces":["docs","packages/*"]}"#;
pub(crate) const LOCK: &str = r#"{
  "lockfileVersion": 2,
  "workspaces": {
    "": {"name": "frontend"},
    "docs": {"name": "chainargos-docs", "dependencies": {"@chainargos/ui": "workspace:*"}},
    "packages/ui": {"name": "@chainargos/ui"},
  },
  "packages": {},
}"#;

pub(crate) fn bun_config(extra: &str) -> String {
    format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\
[stacks.bun]\nroots = [\"frontend\"]\n{extra}"
    )
}

/// Write the workspace, declare Bun in `mise.toml` and stage everything.
pub(crate) fn write_workspace(root: &Path, mise: &str) -> TestResult {
    let write = |rel: &str, text: &str| -> TestResult {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().ok_or("parent")?)?;
        fs::write(path, text)?;
        Ok(())
    };
    write("mise.toml", mise)?;
    write("frontend/package.json", ROOT_PACKAGE)?;
    write("frontend/bun.lock", LOCK)?;
    write(
        "frontend/docs/package.json",
        r#"{"name":"chainargos-docs","scripts":{"typecheck":"tsc","test":"vitest run","build":"vite build","dev":"vite"}}"#,
    )?;
    write(
        "frontend/packages/ui/package.json",
        r#"{"name":"@chainargos/ui","scripts":{"lint":"oxlint","test":"vitest run"}}"#,
    )?;
    git(&["add", "."], root)?;
    Ok(())
}

pub(crate) const MISE: &str = "[tools]\nbun = \"1.4.2\"\n";

#[test]
fn each_member_gets_one_job_with_parallel_script_steps() -> TestResult {
    let repo = make_repo(&bun_config(""))?;
    write_workspace(repo.path(), MISE)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = required_file(&tree, ".github/workflows/ci.yml")?;
    let docs = job_body(ci, "bun-chainargos-docs")?;
    assert!(docs.contains("name: Bun / chainargos-docs"), "{docs}");
    assert!(docs.contains("runs-on: ubuntu-26.04\n"), "{docs}");
    assert!(docs.contains("contents: read"), "{docs}");
    assert!(!docs.contains("packages:"), "{docs}");
    assert!(docs.contains("persist-credentials: \"false\""), "{docs}");
    for step in ["Typecheck", "Test", "Build"] {
        let after = docs
            .split(&format!("- name: {step}\n"))
            .nth(1)
            .and_then(|rest| rest.lines().next())
            .map(str::trim);
        assert_eq!(after, Some("background: true"), "{step}: {docs}");
    }
    assert!(!docs.contains("name: Lint"), "{docs}");
    assert!(!docs.contains("name: Script dev"), "{docs}");
    assert!(docs.contains("--cwd=frontend/docs"), "{docs}");
    assert!(docs.contains("--frozen-lockfile"), "{docs}");
    assert!(docs.contains("--cwd=frontend"), "{docs}");
    assert!(docs.contains("wait-all"), "{docs}");
    assert!(docs.contains("VELNOR_TOOL_BUN"), "{docs}");
    assert!(!docs.contains("VELNOR_NPM_READ_TOKEN"), "{docs}");
    let ui = job_body(ci, "bun-chainargos-ui")?;
    assert!(ui.contains("--cwd=frontend/packages/ui"), "{ui}");
    assert!(ui.contains("name: Lint"), "{ui}");
    let required = job_body(ci, "required")?;
    assert!(required.contains("- bun-chainargos-docs"), "{required}");
    assert!(required.contains("- bun-chainargos-ui"), "{required}");
    Ok(())
}

#[test]
fn generation_is_deterministic() -> TestResult {
    let render = || -> Result<String, Box<dyn std::error::Error>> {
        let repo = make_repo(&bun_config(""))?;
        write_workspace(repo.path(), MISE)?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        Ok(required_file(&tree, ".github/workflows/ci.yml")?.to_owned())
    };
    assert_eq!(render()?, render()?);
    Ok(())
}

#[test]
fn project_policy_failures_stop_generation() -> TestResult {
    let cases = [
        ("[tools]\nbun = \"latest\"\n", "latest"),
        ("[tools]\nnode = \"24.20.0\"\n", "bun_tool_not_declared"),
        (
            "[tools]\nbun = \"1.4.1\"\n",
            "conflicting_project_tool_versions",
        ),
    ];
    for (mise, code) in cases {
        let repo = make_repo(&bun_config(""))?;
        write_workspace(repo.path(), mise)?;
        let error = prepare(repo.path()).expect_err("rejected");
        assert!(error.to_string().contains(code), "{code}: {error}");
    }
    Ok(())
}

#[test]
fn foreign_lockfiles_and_registry_overrides_stop_generation() -> TestResult {
    let repo = make_repo(&bun_config(""))?;
    write_workspace(repo.path(), MISE)?;
    fs::write(repo.path().join("frontend/package-lock.json"), "{}")?;
    git(&["add", "."], repo.path())?;
    let error = prepare(repo.path()).expect_err("npm lockfile");
    assert!(
        error
            .to_string()
            .contains("unsupported_js_lockfile:package-lock.json"),
        "{error}"
    );
    let repo = make_repo(&bun_config(""))?;
    write_workspace(repo.path(), MISE)?;
    fs::write(
        repo.path().join("frontend/bunfig.toml"),
        "[install]\nregistry = \"https://evil.example/\"\n",
    )?;
    git(&["add", "."], repo.path())?;
    let error = prepare(repo.path()).expect_err("registry override");
    assert!(
        error.to_string().contains("bunfig_registry_override"),
        "{error}"
    );
    Ok(())
}

#[test]
fn configuration_naming_unknown_members_or_scripts_fails() -> TestResult {
    for (extra, code) in [
        (
            "[stacks.bun.members.nobody]\nscripts = [\"test\"]\n",
            "unknown_bun_member:nobody",
        ),
        (
            "[stacks.bun.members.\"@chainargos/ui\"]\nscripts = [\"typecheck\"]\n",
            "bun_script_missing",
        ),
    ] {
        let repo = make_repo(&bun_config(extra))?;
        write_workspace(repo.path(), MISE)?;
        let error = prepare(repo.path()).expect_err("rejected");
        assert!(error.to_string().contains(code), "{code}: {error}");
    }
    Ok(())
}

#[test]
fn private_npm_scopes_select_the_two_phase_install_with_packages_read() -> TestResult {
    let config = format!(
        "{}[private_sources]\nowner = \"acme\"\napp_id_variable = \"VELNOR_DEPS_APP_ID\"\n\
app_private_key_secret = \"VELNOR_DEPS_APP_PRIVATE_KEY\"\nnpm_scopes = [\"@chainargos\"]\n",
        bun_config("")
    );
    let repo = make_repo(&config)?;
    write_workspace(repo.path(), MISE)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = required_file(&tree, ".github/workflows/ci.yml")?;
    let docs = job_body(ci, "bun-chainargos-docs")?;
    assert!(docs.contains("packages: read"), "{docs}");
    assert!(docs.contains("contents: read"), "{docs}");
    let registry = docs
        .split("- name: Install dependencies (registry phase)\n")
        .nth(1)
        .ok_or("registry phase step")?;
    let registry = registry.split("- name:").next().ok_or("registry body")?;
    assert!(
        registry.contains("VELNOR_NPM_READ_TOKEN: ${{ github.token }}"),
        "{registry}"
    );
    assert!(
        registry.contains("VELNOR_BUN_SCOPES: \"@chainargos\""),
        "{registry}"
    );
    assert!(
        docs.contains("name: Install dependencies (lifecycle phase)"),
        "{docs}"
    );
    assert_eq!(docs.matches("github.token").count(), 1, "{docs}");
    // Scripts and the lifecycle phase never see the token.
    let after = docs.split("(lifecycle phase)").nth(1).ok_or("lifecycle")?;
    assert!(!after.contains("VELNOR_NPM_READ_TOKEN"), "{after}");
    let ui = job_body(ci, "bun-chainargos-ui")?;
    assert!(ui.contains("packages: read"), "{ui}");
    Ok(())
}
