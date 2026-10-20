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
    add_origin(root)?;
    git(&["add", "."], root)?;
    Ok(())
}

/// Images always emit `publish.yml`, which names the repository by its origin.
fn add_origin(root: &std::path::Path) -> TestResult {
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widgets.git",
        ],
        root,
    )
}

fn pinned() -> String {
    format!("FROM debian:12@{DIGEST}\nCOPY app /usr/local/bin/app\n")
}

fn named_step<'a>(job: &'a str, name: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    let marker = format!("      - name: {name}\n");
    let (_, remainder) = job
        .split_once(&marker)
        .ok_or_else(|| format!("missing workflow step {name}"))?;
    Ok(remainder
        .split_once("\n      - name:")
        .map_or(remainder, |(step, _)| step))
}

fn step_env_value<'a>(step: &'a str, key: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    let line = step
        .lines()
        .find_map(|line| line.trim_start().strip_prefix(key)?.strip_prefix(": "))
        .ok_or("missing step environment value")?;
    Ok(line.trim())
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
fn scale_set_routes_amd64_image_jobs_with_container_and_required_inventory() -> TestResult {
    let repo = schema_two_image_repo("scale-set", "linux_amd64")?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = required_file(&tree, ".github/workflows/ci.yml")?;
    let image = job_body(ci, "image-demo-app-amd64")?;
    assert!(
        image.contains("runs-on: [velnor, ubuntu-26.04-scale-set, verification-worker]"),
        "{image}"
    );
    assert!(image.contains("container:\n      image: ghcr.io/team/job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"), "{image}");
    assert!(image.contains("contents: read"), "{image}");
    assert!(
        !image.contains("packages:") && !image.contains("id-token: write"),
        "{image}"
    );
    assert!(image.contains("Build OCI image"), "{image}");
    let required = job_body(ci, "required")?;
    assert!(required.contains("- image-demo-app-amd64"), "{required}");
    assert!(!ci.contains("image-demo-app-amd64__"), "{ci}");
    Ok(())
}

#[test]
fn both_mode_keeps_each_native_image_job_hosted() -> TestResult {
    let repo = schema_two_image_repo("both", "linux_amd64, linux_arm64")?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = required_file(&tree, ".github/workflows/ci.yml")?;
    for (id, label) in [
        ("image-demo-app-amd64", "ubuntu-26.04"),
        ("image-demo-app-arm64", "ubuntu-26.04-arm"),
    ] {
        let image = job_body(ci, id)?;
        assert!(image.contains(&format!("runs-on: {label}\n")), "{image}");
        assert!(!image.contains("container:"), "{image}");
        assert!(
            !image.contains("__hosted") && !image.contains("__local"),
            "{image}"
        );
    }
    let required = job_body(ci, "required")?;
    assert!(required.contains("- image-demo-app-amd64"), "{required}");
    assert!(required.contains("- image-demo-app-arm64"), "{required}");
    Ok(())
}

#[test]
fn scale_set_rejects_arm64_image_jobs() -> TestResult {
    let repo = schema_two_image_repo("scale-set", "linux_arm64")?;
    let error = render_staged_tree(&prepare(repo.path())?).expect_err("amd64 Scale Set only");
    assert!(
        error
            .to_string()
            .contains("unsupported_workload_for_linux_amd64_scale_set"),
        "{error}"
    );
    Ok(())
}

fn schema_two_image_repo(
    mode: &str,
    platforms: &str,
) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let platforms = platforms
        .split(',')
        .map(str::trim)
        .map(|platform| format!("\"{platform}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let config = format!(
        "schema = 2\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\
[[images]]\nid = \"demo-app\"\nrepository = \"ghcr.io/acme/demo-app\"\ndockerfile = \"svc/Dockerfile\"\n\
context = \"svc\"\nplatforms = [{platforms}]\n\
[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\nmode = \"{mode}\"\n\
[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n\
[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"ubuntu-26.04-scale-set\", \"velnor\", \"verification-worker\"]\nplatform = \"linux/amd64\"\n\
job_container_image = \"ghcr.io/team/job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n"
    );
    let repo = make_repo(&config)?;
    write_dockerfile(repo.path(), &pinned())?;
    Ok(repo)
}

#[test]
fn hosted_mode_keeps_image_job_hosted_when_override_is_configured() -> TestResult {
    let body = "schema = 2\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\
[[images]]\nid = \"demo-app\"\nrepository = \"ghcr.io/acme/demo-app\"\ndockerfile = \"svc/Dockerfile\"\n\
context = \"svc\"\nplatforms = [\"linux_amd64\"]\n\
[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\nmode = \"hosted\"\n\
[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n\
[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"ubuntu-26.04-scale-set\", \"velnor\", \"verification-worker\"]\nplatform = \"linux/amd64\"\n\
[execution.overrides.\"image-demo-app-amd64\"]\nprofile = \"local\"\nrole = \"verification\"\n";
    let repo = make_repo(body)?;
    write_dockerfile(repo.path(), &pinned())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = required_file(&tree, ".github/workflows/ci.yml")?;
    let image = job_body(ci, "image-demo-app-amd64")?;
    assert!(image.contains("runs-on: ubuntu-26.04\n"), "{image}");
    assert!(!image.contains("container:"), "{image}");
    Ok(())
}

const CARGO_IMAGE: &str = "[[images]]\nid = \"demo-app\"\nrepository = \"ghcr.io/acme/demo-app\"\n\
dockerfile = \"Dockerfile\"\nplatforms = [\"linux_amd64\"]\n\
[images.prebuilt]\nkind = \"cargo_bin\"\npackage = \"demo\"\nbin = \"demo\"\nstage = \"dist/demo\"\n";

fn cargo_repo(dockerfile_tail: &str) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let body = format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n{CARGO_IMAGE}"
    );
    let repo = make_repo(&body)?;
    let root = repo.path();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[[bin]]\nname = \"demo\"\npath = \"src/main.rs\"\n",
    )?;
    fs::write(root.join("src/main.rs"), "fn main() {}\n")?;
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )?;
    fs::write(
        root.join("Dockerfile"),
        format!("FROM debian:12@{DIGEST}\n{dockerfile_tail}"),
    )?;
    add_origin(root)?;
    git(&["add", "."], root)?;
    Ok(repo)
}

#[test]
fn cargo_bin_jobs_build_offline_and_stage_before_the_docker_build() -> TestResult {
    let repo = cargo_repo("COPY dist/demo /usr/local/bin/demo\n")?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = required_file(&tree, ".github/workflows/ci.yml")?;
    let job = job_body(ci, "image-demo-app-amd64")?;
    assert!(job.contains("runs-on: ubuntu-26.04\n"), "{job}");
    assert!(job.contains("- plan"), "{job}");
    assert!(job.contains("Setup Mise"), "{job}");
    assert!(
        job.contains("cargo build --release --locked --offline"),
        "{job}"
    );
    assert!(job.contains("--package demo --bin demo"), "{job}");
    let fetch = named_step(job, "Fetch Cargo sources")?;
    let build_step = named_step(job, "Build release binary")?;
    for (native, mise) in [
        ("CARGO_HOME", "MISE_CARGO_HOME"),
        ("RUSTUP_HOME", "MISE_RUSTUP_HOME"),
    ] {
        assert_eq!(
            step_env_value(fetch, native)?,
            step_env_value(build_step, native)?
        );
        assert_eq!(
            step_env_value(fetch, mise)?,
            step_env_value(build_step, mise)?
        );
    }
    for step in [fetch, build_step] {
        for key in [
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
            "ACTIONS_RUNTIME_TOKEN",
        ] {
            assert!(
                step_env_value(step, key).is_err(),
                "unexpected token environment {key}: {step}"
            );
        }
    }
    assert!(
        job.contains("CARGO_TARGET_DIR: ${{ runner.temp }}/velnor/image-target/demo-app"),
        "{job}"
    );
    let build = job.find("Build release binary").ok_or("build step")?;
    let stage = job.find("Stage release binary").ok_or("stage step")?;
    let docker = job.find("Build OCI image").ok_or("docker step")?;
    assert!(build < stage && stage < docker, "{job}");
    assert!(
        !job.contains("packages:") && !job.contains("id-token: write"),
        "{job}"
    );
    Ok(())
}

#[test]
fn cargo_bin_rejects_uncopied_stage_missing_package_and_existing_stage() -> TestResult {
    let repo = cargo_repo("COPY README /readme\n")?;
    let error = prepare(repo.path()).expect_err("stage not copied");
    assert!(
        error
            .to_string()
            .contains("image_prebuilt_stage_not_copied"),
        "{error}"
    );

    let repo = cargo_repo("COPY dist/demo /usr/local/bin/demo\n")?;
    let config = fs::read_to_string(repo.path().join(".velnor/config.toml"))?;
    fs::write(
        repo.path().join(".velnor/config.toml"),
        config.replace("package = \"demo\"", "package = \"nope\""),
    )?;
    let error = prepare(repo.path()).expect_err("missing package");
    assert!(
        error
            .to_string()
            .contains("image_prebuilt_package_not_discovered"),
        "{error}"
    );

    let repo = cargo_repo("COPY dist/demo /usr/local/bin/demo\n")?;
    fs::create_dir_all(repo.path().join("dist"))?;
    fs::write(repo.path().join("dist/demo"), "old")?;
    let error = prepare(repo.path()).expect_err("stage exists");
    assert!(
        error.to_string().contains("image_prebuilt_stage_exists"),
        "{error}"
    );
    Ok(())
}

#[test]
fn cargo_bin_on_arm64_fails_until_the_tools_cache_supports_it() -> TestResult {
    let repo = cargo_repo("COPY dist/demo /usr/local/bin/demo\n")?;
    let config = fs::read_to_string(repo.path().join(".velnor/config.toml"))?;
    fs::write(
        repo.path().join(".velnor/config.toml"),
        config.replace(
            "platforms = [\"linux_amd64\"]",
            "platforms = [\"linux_amd64\", \"linux_arm64\"]",
        ),
    )?;
    let error = prepare(repo.path()).expect_err("arm64 prebuilt unsupported");
    assert!(
        error
            .to_string()
            .contains("image_prebuilt_platform_unsupported:linux_arm64"),
        "{error}"
    );
    Ok(())
}
