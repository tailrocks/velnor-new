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

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// A Bun repository pinning one private release asset (HD-7), plus `extra`
/// `[stacks.bun]` configuration.
pub(crate) fn release_asset_repo(
    extra: &str,
) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let config = format!(
        "{}[private_sources]\nowner = \"acme\"\napp_id_variable = \"VELNOR_DEPS_APP_ID\"\n\
app_private_key_secret = \"VELNOR_DEPS_APP_PRIVATE_KEY\"\nnpm_scopes = [\"@chainargos\"]\n\
[[private_sources.release_assets]]\nid = \"ui-assets\"\nrepository = \"acme/ui-assets\"\n\
tag = \"v1.0.0\"\nasset = \"assets.tar.gz\"\nextract_to = \"ui-assets\"\n",
        bun_config(extra)
    );
    let repo = make_repo(&config)?;
    write_workspace(repo.path(), MISE)?;
    fs::write(
        repo.path().join(".velnor/deps.lock"),
        format!(
            "schema = 1\n[[release_assets]]\nid = \"ui-assets\"\nrepository = \"acme/ui-assets\"\n\
tag = \"v1.0.0\"\ncommit = \"{COMMIT}\"\nasset = \"assets.tar.gz\"\nsha256 = \"{SHA}\"\n\
extract_to = \"ui-assets\"\n"
        ),
    )?;
    git(&["add", "."], repo.path())?;
    Ok(repo)
}

#[test]
fn pinned_release_assets_wire_the_app_token_block_ahead_of_every_install() -> TestResult {
    let repo = release_asset_repo("")?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = required_file(&tree, ".github/workflows/ci.yml")?;
    let docs = job_body(ci, "bun-chainargos-docs")?;
    let position = |needle: &str| docs.find(needle).ok_or(format!("missing {needle}: {docs}"));
    let mint = position("name: Mint read-only token for private sources")?;
    let fetch = position("name: Fetch private sources")?;
    let revoke = position("name: Revoke private-source token")?;
    let registry = position("name: Install dependencies (registry phase)")?;
    let lifecycle = position("name: Install dependencies (lifecycle phase)")?;
    let script = position("name: Test")?;
    assert!(mint < fetch && fetch < revoke, "{docs}");
    assert!(
        revoke < registry && registry < lifecycle && lifecycle < script,
        "the token is revoked before any repository code: {docs}"
    );
    assert!(docs.contains("repositories: ui-assets"), "{docs}");
    assert!(
        docs.contains("VELNOR_PRIVATE_SOURCES: release-assets"),
        "{docs}"
    );
    assert!(
        !docs.contains("name: Fetch private sources\n")
            || docs.matches("deps-token.outputs").count() <= 2,
        "{docs}"
    );
    // The App token is bound to the fetch and revoke steps only.
    let after = &docs[revoke..];
    let after = after
        .split("name: Revoke private-source token")
        .nth(1)
        .ok_or("after")?;
    let after = after
        .split("- name:")
        .skip(1)
        .collect::<Vec<_>>()
        .join("- name:");
    assert!(!after.contains("deps-token"), "{after}");
    Ok(())
}

pub(super) fn npm_config(extra: &str) -> String {
    format!(
        "{}[[publish.npm]]\nid = \"ui\"\nroot = \"frontend\"\nmember = \"@chainargos/ui\"\n\
pack_script = \"pack\"\ntarballs = \"frontend/.portable/tarballs\"\nregistry = \"github\"\n{extra}",
        bun_config("")
    )
}

pub(super) fn write_publishable(root: &Path, member_json: &str) -> TestResult {
    write_workspace(root, MISE)?;
    fs::write(root.join("frontend/package.json"), ROOT_WITH_PACK)?;
    fs::write(root.join("frontend/packages/ui/package.json"), member_json)?;
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widgets.git",
        ],
        root,
    )?;
    git(&["add", "."], root)
}

const ROOT_WITH_PACK: &str = r#"{"name":"frontend","private":true,"packageManager":"bun@1.4.2","workspaces":["docs","packages/*"],"scripts":{"pack":"bun scripts/pack.ts"}}"#;
pub(super) const UI_PUBLISHABLE: &str = r#"{"name":"@chainargos/ui","version":"1.4.0","publishConfig":{"registry":"https://npm.pkg.github.com"},"scripts":{"test":"vitest run"}}"#;

#[test]
fn npm_entries_generate_the_build_attest_publish_chain() -> TestResult {
    let repo = make_repo(&npm_config(""))?;
    write_publishable(repo.path(), UI_PUBLISHABLE)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let publish = required_file(&tree, ".github/workflows/publish.yml")?;
    let build = job_body(publish, "npm-build-ui")?;
    assert!(build.contains("contents: read"), "{build}");
    assert!(!build.contains("packages:"), "{build}");
    assert!(build.contains("actions/checkout@"), "{build}");
    assert!(
        build.contains("tarballs differ between two packs"),
        "{build}"
    );
    assert!(build.contains("chainargos-ui"), "{build}");
    let attest = job_body(publish, "npm-attest-ui")?;
    assert!(attest.contains("id-token: write"), "{attest}");
    assert!(!attest.contains("actions/checkout@"), "{attest}");
    let push = job_body(publish, "npm-publish-ui")?;
    assert!(push.contains("packages: write"), "{push}");
    assert!(push.contains("environment: release"), "{push}");
    assert!(!push.contains("actions/checkout@"), "{push}");
    assert!(push.contains("https://npm.pkg.github.com"), "{push}");
    assert!(
        !push.contains("bun@") && !push.contains("npm publish --"),
        "{push}"
    );
    let authority = job_body(publish, "publish-authority")?;
    assert!(
        authority.contains("frontend/packages/ui/package.json"),
        "{authority}"
    );
    Ok(())
}

#[test]
fn npm_entries_fail_closed_on_policy_violations() -> TestResult {
    let cases = [
        (
            npm_config(""),
            r#"{"name":"@chainargos/ui","version":"1.4.0","private":true,"publishConfig":{"registry":"https://npm.pkg.github.com"},"scripts":{"test":"x"}}"#,
            "npm_member_private",
        ),
        (
            npm_config(""),
            r#"{"name":"@chainargos/ui","version":"1.4.0","publishConfig":{"registry":"https://registry.npmjs.org"},"scripts":{"test":"x"}}"#,
            "npm_publish_registry",
        ),
        (
            npm_config(""),
            r#"{"name":"@chainargos/ui","version":"^1.4.0","publishConfig":{"registry":"https://npm.pkg.github.com"},"scripts":{"test":"x"}}"#,
            "version_not_literal",
        ),
        (
            npm_config("").replace(
                "member = \"@chainargos/ui\"",
                "member = \"@chainargos/nope\"",
            ),
            UI_PUBLISHABLE,
            "unknown_bun_member",
        ),
        (
            npm_config("").replace("pack_script = \"pack\"", "pack_script = \"missing\""),
            UI_PUBLISHABLE,
            "npm_pack_script_missing",
        ),
        (
            npm_config("").replace("root = \"frontend\"", "root = \"elsewhere\""),
            UI_PUBLISHABLE,
            "not_a_stacks_bun_root",
        ),
    ];
    for (config, member, code) in cases {
        let repo = make_repo(&config)?;
        write_publishable(repo.path(), member)?;
        let error = prepare(repo.path())
            .and_then(|prepared| render_staged_tree(&prepared))
            .expect_err(code);
        assert!(error.to_string().contains(code), "{code}: {error}");
    }
    Ok(())
}
