//! `[[images]]` publication jobs of `publish.yml` through the real generator.

use std::fs;

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, git, make_repo};
use crate::impl_schema2_routing::{job_body, required_file};

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PUBLISH_PATH: &str = ".github/workflows/publish.yml";

fn images_config(extra: &str) -> String {
    format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\
[[images]]\nid = \"demo-app\"\nrepository = \"ghcr.io/acme/demo-app\"\ndockerfile = \"svc/Dockerfile\"\n\
context = \"svc\"\nplatforms = [\"linux_amd64\", \"linux_arm64\"]\n{extra}"
    )
}

fn image_repo(config: &str) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let repo = make_repo(config)?;
    let root = repo.path();
    fs::create_dir_all(root.join("svc"))?;
    fs::write(
        root.join("svc/Dockerfile"),
        format!("FROM debian:12@{DIGEST}\nCOPY app /usr/local/bin/app\n"),
    )?;
    fs::write(root.join("svc/app"), "bin")?;
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
    Ok(repo)
}

fn publish_yaml(config: &str) -> Result<String, Box<dyn std::error::Error>> {
    let repo = image_repo(config)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    Ok(required_file(&tree, PUBLISH_PATH)?.to_owned())
}

#[test]
fn no_images_means_no_publish_workflow() -> TestResult {
    let repo = make_repo(crate::impl_common::config_with_branch())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    assert!(tree.files.iter().all(|f| f.path != PUBLISH_PATH));
    Ok(())
}

#[test]
fn publish_job_is_gated_privileged_and_checks_out_nothing() -> TestResult {
    let yaml = publish_yaml(&images_config(""))?;
    assert!(yaml.contains("branches:\n      - testmain"), "{yaml}");
    assert!(!yaml.contains("tags:"), "{yaml}");
    let job = job_body(&yaml, "image-publish-demo-app")?;
    assert!(
        job.contains("needs:\n      - publish-branch-authority"),
        "{job}"
    );
    assert!(job.contains("environment: images"), "{job}");
    for scope in [
        "actions: read",
        "attestations: write",
        "contents: read",
        "id-token: write",
        "packages: write",
    ] {
        assert!(job.contains(scope), "{scope}:\n{job}");
    }
    assert!(!job.contains("actions/checkout"), "{job}");
    assert!(!job.contains("contents: write"), "{job}");
    Ok(())
}

#[test]
fn publish_job_pushes_by_digest_and_attests_provenance_and_sboms() -> TestResult {
    let yaml = publish_yaml(&images_config(""))?;
    let job = job_body(&yaml, "image-publish-demo-app")?;
    assert!(job.contains("velnor-image-demo-app-$arch"), "{job}");
    assert!(job.contains("crane push"), "{job}");
    assert!(job.contains("sha-$GITHUB_SHA"), "{job}");
    assert!(job.contains("crane tag"), "{job}");
    assert!(job.contains("actions/attest-build-provenance@"), "{job}");
    assert_eq!(job.matches("uses: actions/attest@").count(), 2, "{job}");
    assert!(job.contains("subject-digest: ${{ steps.image.outputs.digest }}"));
    assert!(job.contains("steps.image.outputs.digest_amd64"), "{job}");
    assert!(job.contains("steps.image.outputs.digest_arm64"), "{job}");
    assert!(job.contains("gh attestation verify"), "{job}");
    assert!(job.contains("ghcr.io/acme/demo-app"), "{job}");
    assert!(job.contains("create-storage-record: \"false\""), "{job}");
    Ok(())
}

#[test]
fn registry_owner_must_match_the_repository_owner_at_run_time() -> TestResult {
    let yaml = publish_yaml(&images_config(""))?;
    let job = job_body(&yaml, "image-publish-demo-app")?;
    assert!(job.contains("read -r owner"), "{job}");
    assert!(job.contains("acme"), "{job}");
    assert!(job.contains("GITHUB_REPOSITORY_OWNER"), "{job}");
    Ok(())
}

#[test]
fn the_ci_workflow_holds_no_publish_job_or_registry_scope() -> TestResult {
    let repo = image_repo(&images_config(""))?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = required_file(&tree, ".github/workflows/ci.yml")?;
    assert!(!ci.contains("image-publish"), "{ci}");
    assert!(!ci.contains("packages:"), "{ci}");
    assert!(!ci.contains("attestations:"), "{ci}");
    Ok(())
}

const PROMOTE_EXTRA: &str = "version_file = \"svc/package.json\"\n";

fn promotable(extra: &str) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let repo = image_repo(&images_config(&format!("{PROMOTE_EXTRA}{extra}")))?;
    fs::write(
        repo.path().join("svc/package.json"),
        "{\"name\": \"demo-app\", \"version\": \"1.2.3\"}\n",
    )?;
    git(&["add", "."], repo.path())?;
    Ok(repo)
}

#[test]
fn a_version_file_adds_the_tag_trigger_and_a_promote_job() -> TestResult {
    let repo = promotable("")?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let yaml = required_file(&tree, PUBLISH_PATH)?;
    assert!(yaml.contains("*-v[0-9]*.[0-9]*.[0-9]*"), "{yaml}");
    assert!(yaml.contains("demo-app"), "declared set:\n{yaml}");
    let authority = job_body(yaml, "publish-authority")?;
    assert!(
        authority.contains("steps.tag.outputs.artifact == 'demo-app'"),
        "{authority}"
    );
    assert!(
        authority.contains("contents/svc/package.json?ref=$GITHUB_SHA"),
        "{authority}"
    );
    let branch = job_body(yaml, "publish-branch-authority")?;
    assert!(
        branch.contains("if: github.ref_type == 'branch'"),
        "{branch}"
    );
    assert!(
        authority.contains("if: github.ref_type == 'tag'"),
        "{authority}"
    );
    Ok(())
}

#[test]
fn promote_retags_the_attested_digest_and_never_builds() -> TestResult {
    let repo = promotable("")?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let yaml = required_file(&tree, PUBLISH_PATH)?;
    let job = job_body(yaml, "image-promote-demo-app")?;
    assert!(job.contains("needs:\n      - publish-authority"), "{job}");
    assert!(
        job.contains("needs.publish-authority.outputs.artifact == 'demo-app'"),
        "{job}"
    );
    assert!(job.contains("environment: images"), "{job}");
    for scope in ["attestations: read", "packages: write", "contents: read"] {
        assert!(job.contains(scope), "{scope}:\n{job}");
    }
    for forbidden in [
        "id-token",
        "actions/checkout",
        "buildx",
        "docker build",
        "crane push",
    ] {
        assert!(!job.contains(forbidden), "{forbidden}:\n{job}");
    }
    assert!(job.contains("gh attestation verify"), "{job}");
    assert!(job.contains("--source-digest"), "{job}");
    assert!(job.contains("--source-ref refs/heads/testmain"), "{job}");
    assert!(job.contains("sha-$GITHUB_SHA"), "{job}");
    assert!(job.contains("crane tag"), "{job}");
    Ok(())
}

#[test]
fn an_unreadable_or_unsupported_version_file_fails_generation() -> TestResult {
    let repo = image_repo(&images_config(PROMOTE_EXTRA))?;
    let error = prepare(repo.path()).and_then(|prep| render_staged_tree(&prep));
    let text = error.expect_err("missing manifest").to_string();
    assert!(text.contains("unreadable_version_file"), "{text}");
    let repo = image_repo(&images_config("version_file = \"svc/VERSION\"\n"))?;
    let text = prepare(repo.path()).expect_err("unsupported").to_string();
    assert!(text.contains("unsupported_image_version_file"), "{text}");
    Ok(())
}

#[test]
fn image_and_bundle_ids_share_one_tag_namespace() -> TestResult {
    let config = images_config(&format!(
        "{PROMOTE_EXTRA}[[publish.bundles]]\nid = \"demo-app\"\npaths = [\"svc\"]\nversion = \"1.0.0\"\n"
    ));
    let repo = promotable("")?;
    fs::write(repo.path().join(".velnor/config.toml"), config)?;
    git(&["add", "."], repo.path())?;
    let text = render_staged_tree(&prepare(repo.path())?)
        .expect_err("collision")
        .to_string();
    assert!(text.contains("artifact_id_collides:demo-app"), "{text}");
    Ok(())
}
