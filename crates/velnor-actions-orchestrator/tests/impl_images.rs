//! `[[images]]` build jobs through the real generator.

use std::fs;

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, git, make_repo};
use crate::impl_schema2_routing::{job_body, required_file};

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn images_config(extra: &str) -> String {
    format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\
[[images]]\nid = \"demo-app\"\nrepository = \"ghcr.io/acme/demo-app\"\ndockerfile = \"svc/Dockerfile\"\n\
context = \"svc\"\nplatforms = [\"linux_amd64\", \"linux_arm64\"]\n{extra}"
    )
}

fn write_dockerfile(root: &std::path::Path, body: &str) -> TestResult {
    fs::create_dir_all(root.join("svc"))?;
    fs::write(root.join("svc/Dockerfile"), body)?;
    fs::write(root.join("svc/app"), "bin")?;
    git(&["add", "."], root)?;
    Ok(())
}

fn pinned() -> String {
    format!("FROM debian:12@{DIGEST}\nCOPY app /usr/local/bin/app\n")
}

#[test]
fn image_jobs_are_native_unprivileged_and_required() -> TestResult {
    let repo = make_repo(&images_config(""))?;
    write_dockerfile(repo.path(), &pinned())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = required_file(&tree, ".github/workflows/ci.yml")?;
    let amd = job_body(ci, "image-demo-app-amd64")?;
    let arm = job_body(ci, "image-demo-app-arm64")?;
    assert!(amd.contains("runs-on: ubuntu-26.04\n"), "{amd}");
    assert!(arm.contains("runs-on: ubuntu-26.04-arm\n"), "{arm}");
    for body in [amd, arm] {
        assert!(body.contains("timeout-minutes: 30"), "{body}");
        assert!(body.contains("contents: read"), "{body}");
        assert!(!body.contains("packages:") && !body.contains("id-token: write"));
        assert!(body.contains("persist-credentials: \"false\""), "{body}");
        assert!(body.contains("moby/buildkit@sha256:"), "{body}");
        assert!(body.contains("--provenance=mode=max"), "{body}");
        assert!(body.contains("if: always()"), "{body}");
        assert!(body.contains("actions/upload-artifact@"), "{body}");
    }
    let required = job_body(ci, "required")?;
    assert!(required.contains("- image-demo-app-amd64"), "{required}");
    assert!(required.contains("- image-demo-app-arm64"), "{required}");
    Ok(())
}

#[test]
fn unpinned_base_secret_mount_and_untracked_dockerfile_fail_generation() -> TestResult {
    let cases = [
        ("FROM rust:1.99\n".to_owned(), "image_base_unpinned"),
        (
            format!("FROM debian@{DIGEST}\nRUN --mount=type=secret,id=t true\n"),
            "image_secret_mount",
        ),
    ];
    for (body, code) in cases {
        let repo = make_repo(&images_config(""))?;
        write_dockerfile(repo.path(), &body)?;
        let error = prepare(repo.path()).expect_err("rejected");
        assert!(error.to_string().contains(code), "{code}: {error}");
    }
    let repo = make_repo(&images_config(""))?;
    fs::create_dir_all(repo.path().join("svc"))?;
    fs::write(repo.path().join("svc/Dockerfile"), pinned())?;
    let error = prepare(repo.path()).expect_err("untracked");
    assert!(
        error
            .to_string()
            .contains("image_dockerfile_must_be_tracked"),
        "{error}"
    );
    Ok(())
}

#[test]
fn a_changed_dockerfile_changes_the_source_digest_binding() -> TestResult {
    let render = |body: &str| -> Result<String, Box<dyn std::error::Error>> {
        let repo = make_repo(&images_config(""))?;
        write_dockerfile(repo.path(), body)?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        Ok(required_file(&tree, ".github/workflows/ci.yml")?.to_owned())
    };
    let first = render(&pinned())?;
    let second = render(&format!("{}# changed\n", pinned()))?;
    assert_ne!(first, second);
    Ok(())
}

#[test]
fn image_jobs_are_hosted_only_in_every_schema_two_mode() -> TestResult {
    for mode in ["hosted", "scale-set", "both"] {
        let body = format!(
            "schema = 2\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\
[[images]]\nid = \"demo-app\"\nrepository = \"ghcr.io/acme/demo-app\"\ndockerfile = \"svc/Dockerfile\"\n\
context = \"svc\"\nplatforms = [\"linux_amd64\", \"linux_arm64\"]\n\
[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\nmode = \"{mode}\"\n\
[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n\
[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"ubuntu-26.04-scale-set\", \"velnor\", \"verification-worker\"]\nplatform = \"linux/amd64\"\n"
        );
        let repo = make_repo(&body)?;
        write_dockerfile(repo.path(), &pinned())?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        let ci = required_file(&tree, ".github/workflows/ci.yml")?;
        assert!(job_body(ci, "image-demo-app-amd64")?.contains("runs-on: ubuntu-26.04\n"));
        assert!(job_body(ci, "image-demo-app-arm64")?.contains("runs-on: ubuntu-26.04-arm\n"));
        assert!(!ci.contains("image-demo-app-amd64__"), "{mode}");
        assert!(!ci.contains("image-demo-app-arm64__"), "{mode}");
        let required = job_body(ci, "required")?;
        assert!(
            required.contains("- image-demo-app-amd64"),
            "{mode}: {required}"
        );
    }
    Ok(())
}

#[test]
fn image_job_rejects_a_scale_set_profile_override() -> TestResult {
    let body = "schema = 2\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\
[[images]]\nid = \"demo-app\"\nrepository = \"ghcr.io/acme/demo-app\"\ndockerfile = \"svc/Dockerfile\"\n\
context = \"svc\"\nplatforms = [\"linux_amd64\"]\n\
[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\nmode = \"hosted\"\n\
[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n\
[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"ubuntu-26.04-scale-set\", \"velnor\", \"verification-worker\"]\nplatform = \"linux/amd64\"\n\
[execution.overrides.\"image-demo-app-amd64\"]\nprofile = \"local\"\nrole = \"verification\"\n";
    let repo = make_repo(body)?;
    write_dockerfile(repo.path(), &pinned())?;
    let error = render_staged_tree(&prepare(repo.path())?).expect_err("hosted only");
    assert!(
        error
            .to_string()
            .contains("workflow_task_hosted_only_scale_set_override"),
        "{error}"
    );
    Ok(())
}
