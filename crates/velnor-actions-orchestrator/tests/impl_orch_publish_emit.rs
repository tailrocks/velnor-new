//! Publication workflow emission over tempdir-built fixtures.

use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare, render_staged_tree};

use crate::impl_common::{TestResult, err_of, git, make_repo, plan_for};

const PUBLISH_PATH: &str = ".github/workflows/publish.yml";

/// One bundle versioned from the root manifest, one from an inline literal.
const BUNDLE_CONFIG: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\
[[publish.bundles]]\nid = \"demo-schema\"\npaths = [\"schemas/a.graphql\", \"schemas/b.graphql\"]\nversion_file = \"Cargo.toml\"\n\
[[publish.bundles]]\nid = \"demo-tables\"\npaths = [\"tables\"]\nversion = \"3.1.0\"\n";

/// Fixture repo with bundle sources and a GitHub origin.
fn publish_repo(config: &str) -> Result<TempDir, Box<dyn std::error::Error>> {
    let repo = make_repo(config)?;
    let root = repo.path();
    fs::create_dir_all(root.join("schemas"))?;
    fs::create_dir_all(root.join("tables"))?;
    fs::write(root.join("schemas/a.graphql"), "type A\n")?;
    fs::write(root.join("schemas/b.graphql"), "type B\n")?;
    fs::write(root.join("tables/t.csv"), "x\n")?;
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widgets.git",
        ],
        root,
    )?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "publish fixture"], root)?;
    Ok(repo)
}

fn rendered(config: &str) -> Result<String, Box<dyn std::error::Error>> {
    let repo = publish_repo(config)?;
    let parent = TempDir::new()?;
    let preview = parent.path().join("preview");
    generate(
        &prepare(repo.path())?,
        &GenerateOptions {
            output_dir: Some(preview.clone()),
        },
    )?;
    Ok(fs::read_to_string(preview.join(PUBLISH_PATH))?)
}

fn config_error(config: &str) -> Result<String, Box<dyn std::error::Error>> {
    let repo = publish_repo(config)?;
    let err = err_of(
        render_staged_tree(&prepare(repo.path())?),
        "stale publish config",
    )?;
    Ok(err.to_string())
}

fn routed_bundle_config(mode: &str) -> String {
    let config = BUNDLE_CONFIG
        .replace("schema = 1", "schema = 2")
        .replace("name = \"CI\"", "name = \"CI\"\npolicy = \"consumer-v1\"");
    format!(
        "{config}\n[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\nmode = \"{mode}\"\n\
[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n\
[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\nplatform = \"linux/amd64\"\njob_container_image = \"ghcr.io/team/job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n"
    )
}

#[test]
fn no_bundle_means_no_publish_workflow() -> TestResult {
    let repo = make_repo(crate::impl_common::config_with_branch())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    assert!(tree.files.iter().all(|f| f.path != PUBLISH_PATH));
    Ok(())
}

#[test]
fn plan_lists_publish_workflow_iff_bundles_are_declared() -> TestResult {
    let repo = publish_repo(BUNDLE_CONFIG)?;
    assert!(plan_for(&prepare(repo.path())?)?.contains(PUBLISH_PATH));
    Ok(())
}

#[test]
fn tag_trigger_and_authority_gate() -> TestResult {
    let yaml = rendered(BUNDLE_CONFIG)?;
    assert!(
        yaml.contains("tags:\n      - '*-v[0-9]*.[0-9]*.[0-9]*'") || yaml.contains("*-v[0-9]*"),
        "{yaml}"
    );
    for forbidden in [
        "pull_request",
        "workflow_run",
        "workflow_dispatch",
        "schedule",
    ] {
        assert!(!yaml.contains(forbidden), "{forbidden}:\n{yaml}");
    }
    assert!(yaml.contains("group: publish-${{ github.ref }}"), "{yaml}");
    assert!(yaml.contains("cancel-in-progress: false"), "{yaml}");
    assert!(yaml.contains("acme/widgets"), "repo gate:\n{yaml}");
    assert!(
        yaml.contains("demo-schema|demo-tables"),
        "declared set:\n{yaml}"
    );
    assert!(
        yaml.contains("compare/testmain...$GITHUB_SHA"),
        "ancestry:\n{yaml}"
    );
    assert!(
        yaml.contains("check_name=Required"),
        "required gate:\n{yaml}"
    );
    assert!(yaml.contains("lightweight_tag"), "annotated gate:\n{yaml}");
    assert!(
        yaml.contains("steps.tag.outputs.artifact == 'demo-schema'"),
        "{yaml}"
    );
    assert!(
        yaml.contains("3.1.0") && yaml.contains("steps.tag.outputs.artifact == 'demo-tables'"),
        "inline version:\n{yaml}"
    );
    assert!(
        yaml.contains("contents/Cargo.toml?ref=$GITHUB_SHA"),
        "manifest read:\n{yaml}"
    );
    Ok(())
}

#[test]
fn publication_jobs_follow_the_configured_execution_mode() -> TestResult {
    let jobs = [
        "publish-authority",
        "bundle-build-demo-schema",
        "bundle-attest-demo-schema",
        "bundle-publish-demo-schema",
        "bundle-build-demo-tables",
        "bundle-attest-demo-tables",
        "bundle-publish-demo-tables",
    ];
    for (mode, routed) in [("scale-set", true), ("both", false)] {
        let yaml = rendered(&routed_bundle_config(mode))?;
        for id in jobs {
            let block = job_block(&yaml, id)?;
            if routed {
                assert!(
                    block.contains("runs-on: [velnor, ubuntu-26.04-scale-set]"),
                    "Execution mode \"{mode}\" must route job {id}. Details follow:\n{block}"
                );
                assert!(
                    block.contains("ghcr.io/team/job@sha256:aaaaaaaa"),
                    "Execution mode \"{mode}\" must set the immutable job image for job {id}. Details follow:\n{block}"
                );
            } else {
                assert!(
                    block.contains("runs-on: ubuntu-26.04"),
                    "Execution mode \"{mode}\" must preserve the hosted runner for job {id}. Details follow:\n{block}"
                );
                assert!(
                    !block.contains("container:"),
                    "Execution mode \"{mode}\" must not add a job container for {id}. Details follow:\n{block}"
                );
            }
        }
        assert!(yaml.contains("group: publish-${{ github.ref }}"), "{yaml}");
        assert!(yaml.contains("cancel-in-progress: false"), "{yaml}");
        assert!(yaml.contains("tags:"), "{yaml}");
        let publish = job_block(&yaml, "bundle-publish-demo-schema")?;
        assert!(publish.contains("environment: release"), "{publish}");
        assert!(publish.contains("contents: write"), "{publish}");
        assert!(!publish.contains("actions/checkout@"), "{publish}");
    }
    Ok(())
}

/// Text of one top-level job (two-space indent) up to the next job.
fn job_block<'a>(yaml: &'a str, id: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    let head = format!("\n  {id}:\n");
    let start = yaml.find(&head).ok_or("job missing")? + 1;
    let rest = &yaml[start..];
    let after = head.len() - 1;
    let end = rest[after..]
        .match_indices("\n  ")
        .find(|(at, _)| {
            let tail = &rest[after + at + 3..];
            !tail.starts_with(' ') && tail.lines().next().is_some_and(|line| line.ends_with(':'))
        })
        .map_or(rest.len(), |(at, _)| after + at);
    Ok(&rest[..end])
}

#[test]
fn privileged_jobs_hold_exact_permissions_and_never_check_out() -> TestResult {
    let yaml = rendered(BUNDLE_CONFIG)?;
    let ids = [
        "publish-authority",
        "bundle-build-demo-schema",
        "bundle-attest-demo-schema",
        "bundle-publish-demo-schema",
        "bundle-build-demo-tables",
        "bundle-attest-demo-tables",
        "bundle-publish-demo-tables",
    ];
    for id in ids {
        let block = job_block(&yaml, id)?;
        let checkout = block.contains("actions/checkout@");
        assert_eq!(checkout, id.starts_with("bundle-build-"), "{id}:\n{block}");
        assert_eq!(block.contains("needs:"), id != "publish-authority", "{id}");
    }
    let publish = job_block(&yaml, "bundle-publish-demo-schema")?;
    assert!(publish.contains("environment: release"), "{publish}");
    assert!(publish.contains("contents: write"), "{publish}");
    assert!(!publish.contains("id-token"), "{publish}");
    let attest = job_block(&yaml, "bundle-attest-demo-schema")?;
    assert!(
        attest.contains("id-token: write") && attest.contains("attestations: write"),
        "{attest}"
    );
    assert!(
        !attest.contains("environment:") && attest.contains("contents: read"),
        "{attest}"
    );
    let authority = job_block(&yaml, "publish-authority")?;
    assert!(
        authority.contains("checks: read") && authority.contains("contents: read"),
        "{authority}"
    );
    assert!(!authority.contains("write"), "{authority}");
    let build = job_block(&yaml, "bundle-build-demo-schema")?;
    assert!(
        build.contains("needs.publish-authority.outputs.artifact == 'demo-schema'"),
        "{build}"
    );
    Ok(())
}

#[test]
fn bundle_pack_is_deterministic_by_construction() -> TestResult {
    let yaml = rendered(BUNDLE_CONFIG)?;
    for flag in [
        "--sort=name",
        "--mtime=@0",
        "--owner=0",
        "--group=0",
        "--numeric-owner",
        "gzip -n",
    ] {
        assert!(yaml.contains(flag), "{flag}:\n{yaml}");
    }
    assert!(!yaml.contains("--clobber"), "assets are immutable:\n{yaml}");
    Ok(())
}

#[test]
fn generation_is_deterministic() -> TestResult {
    assert_eq!(rendered(BUNDLE_CONFIG)?, rendered(BUNDLE_CONFIG)?);
    Ok(())
}

#[test]
fn stale_declarations_fail_at_generation() -> TestResult {
    let missing = BUNDLE_CONFIG.replace("schemas/b.graphql", "schemas/gone.graphql");
    assert!(config_error(&missing)?.contains("missing_path:schemas/gone.graphql"));
    let collides = format!(
        "{}[stacks.rust.release]\nenabled = true\npackages = [\"demo\"]\n",
        BUNDLE_CONFIG.replace("demo-schema", "demo")
    );
    assert!(config_error(&collides)?.contains("artifact_id_collides_with_package"));
    Ok(())
}

#[test]
fn symlinked_bundle_paths_are_refused() -> TestResult {
    let repo = publish_repo(BUNDLE_CONFIG)?;
    let root: &Path = repo.path();
    fs::remove_file(root.join("schemas/b.graphql"))?;
    std::os::unix::fs::symlink("a.graphql", root.join("schemas/b.graphql"))?;
    let err = err_of(render_staged_tree(&prepare(root)?), "symlink path")?.to_string();
    assert!(err.contains("symlink_path:schemas/b.graphql"), "{err}");
    Ok(())
}

#[test]
fn manifests_without_a_literal_version_are_refused() -> TestResult {
    let config = BUNDLE_CONFIG.replace(
        "version_file = \"Cargo.toml\"",
        "version_file = \"package.json\"",
    );
    let repo = publish_repo(&config)?;
    fs::write(repo.path().join("package.json"), "{\"name\": \"demo\"}\n")?;
    let err = err_of(render_staged_tree(&prepare(repo.path())?), "no version")?.to_string();
    assert!(err.contains("version_not_literal:package.json"), "{err}");
    Ok(())
}

#[test]
fn unsupported_version_manifest_is_a_config_error() -> TestResult {
    let config = BUNDLE_CONFIG.replace(
        "version_file = \"Cargo.toml\"",
        "version_file = \"tables/t.csv\"",
    );
    let repo = publish_repo(&config)?;
    let err = err_of(prepare(repo.path()).map(|_| ()), "unsupported manifest")?.to_string();
    assert!(err.contains("unsupported_version_file"), "{err}");
    Ok(())
}
