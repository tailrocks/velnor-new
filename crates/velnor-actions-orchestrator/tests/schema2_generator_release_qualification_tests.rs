//! Release-candidate qualification graph assertions.

use std::error::Error;

struct Target<'a> {
    job: &'a str,
    runner: &'a str,
    build: &'a str,
    triple: &'a str,
    artifact: &'a str,
    binary: String,
    sidecar: String,
    provenance: String,
}

/// Require exact artifact binding, behavior qualification, and fail-closed fan-in.
pub(super) fn assert_qualification_jobs(body: &str) -> Result<(), Box<dyn Error>> {
    let version = env!("CARGO_PKG_VERSION");
    let targets = vec![
        Target {
            job: "qualify-linux-x64",
            runner: "runs-on: ubuntu-26.04\n",
            build: "build-linux-x64",
            triple: "x86_64-unknown-linux-gnu",
            artifact: "generator-linux-x64-assets",
            binary: format!("velnor-actions-{version}-x86_64-unknown-linux-gnu"),
            sidecar: format!("velnor-actions-{version}-x86_64-unknown-linux-gnu.sha256"),
            provenance: format!(
                "velnor-actions-{version}-x86_64-unknown-linux-gnu.provenance.json"
            ),
        },
        Target {
            job: "qualify-macos-arm64",
            runner: "runs-on: macos-15\n",
            build: "build-macos-arm64",
            triple: "aarch64-apple-darwin",
            artifact: "generator-macos-arm64-assets",
            binary: format!("velnor-actions-{version}-aarch64-apple-darwin"),
            sidecar: format!("velnor-actions-{version}-aarch64-apple-darwin.sha256"),
            provenance: format!("velnor-actions-{version}-aarch64-apple-darwin.provenance.json"),
        },
        Target {
            job: "qualify-macos-x64",
            runner: "runs-on: macos-15-intel\n",
            build: "build-macos-x64",
            triple: "x86_64-apple-darwin",
            artifact: "generator-macos-x64-assets",
            binary: format!("velnor-actions-{version}-x86_64-apple-darwin"),
            sidecar: format!("velnor-actions-{version}-x86_64-apple-darwin.sha256"),
            provenance: format!("velnor-actions-{version}-x86_64-apple-darwin.provenance.json"),
        },
    ];
    for target in &targets {
        assert_target_job(body, target)?;
    }
    assert_publisher_gates(body)?;
    Ok(())
}

fn assert_target_job(body: &str, target: &Target<'_>) -> Result<(), Box<dyn Error>> {
    let job = super::super::job_body(body, target.job)?;
    assert!(job.contains(target.runner), "{}", target.job);
    for dependency in ["release-gate", target.build, "prepare-manifest"] {
        assert!(job.contains(&format!("- {dependency}\n")), "{job}");
        assert!(
            job.contains(&format!("needs.{dependency}.result == 'success'")),
            "{job}"
        );
    }
    assert_step_order(job)?;
    assert_artifact_binding(job, target);
    assert_manifest_binding(job, target);
    assert_native_runner(job, target)?;
    assert_behavior_corpus(job);
    Ok(())
}

fn assert_step_order(job: &str) -> Result<(), Box<dyn Error>> {
    let candidate = job
        .find("name: Download exact candidate artifact")
        .ok_or("candidate download missing")?;
    let manifest = job
        .find("name: Download attested release manifest")
        .ok_or("manifest download missing")?;
    let digest = job
        .find("name: Bind downloaded candidate to manifest digest")
        .ok_or("digest binding missing")?;
    let qualify = job
        .find("name: Qualify exact downloaded candidate")
        .ok_or("candidate qualification missing")?;
    assert!(
        candidate < manifest && manifest < digest && digest < qualify,
        "{job}"
    );
    Ok(())
}

fn assert_artifact_binding(job: &str, target: &Target<'_>) {
    assert!(job.contains(&format!("name: {}", target.artifact)), "{job}");
    assert!(job.contains("name: generator-release-manifest"), "{job}");
    assert!(job.contains("path: assets"), "{job}");
    assert!(job.contains("path: release-manifest"), "{job}");
    assert!(job.contains(&target.binary), "{job}");
    assert!(job.contains(&target.sidecar), "{job}");
    assert!(job.contains(&target.provenance), "{job}");
}

fn assert_manifest_binding(job: &str, target: &Target<'_>) {
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        job.contains(&format!("--arg target \\\"{}\\\"", target.triple)),
        "{job}"
    );
    assert!(job.contains(".targets | length == 3"), "{job}");
    assert!(job.contains("test(\\\"^[0-9a-f]{64}$\\\")"), "{job}");
    assert!(job.contains("releases/download/"), "{job}");
    assert!(
        job.contains(&format!("--arg tag \\\"v{version}\\\"")),
        "{job}"
    );
    assert!(
        job.contains("test \\\"$actual\\\" = \\\"$expected\\\""),
        "{job}"
    );
    assert!(job.contains("GITHUB_SHA"), "{job}");
    assert!(job.contains("git rev-parse HEAD"), "{job}");
    assert!(job.contains(".version == $version"), "{job}");
    assert!(job.contains(".repository == $repository"), "{job}");
    assert!(job.contains(".commit == $commit"), "{job}");
    assert!(job.contains("tailrocks/velnor-new"), "{job}");
    assert!(job.contains("VELNOR_RELEASE_MANIFEST_SHA256"), "{job}");
    assert!(job.contains("GITHUB_WORKFLOW_SHA"), "{job}");
    assert!(job.contains("provenance"), "{job}");
    assert!(!job.contains("generator-$GITHUB_SHA"), "{job}");
}

fn assert_native_runner(job: &str, target: &Target<'_>) -> Result<(), Box<dyn Error>> {
    let make_candidate_executable = job
        .find("chmod +x \\\"$candidate\\\"")
        .ok_or("candidate executable-mode normalization missing")?;
    let run_candidate = job
        .find("./\\\"$candidate\\\" --version")
        .ok_or("candidate version check missing")?;
    assert!(make_candidate_executable < run_candidate, "{job}");
    let native_host = match target.triple {
        "x86_64-unknown-linux-gnu" => "Linux:x86_64",
        "aarch64-apple-darwin" => "macOS:arm64",
        "x86_64-apple-darwin" => "macOS:x86_64",
        _ => return Err("unexpected qualification target".into()),
    };
    assert!(job.contains("uname -m"), "{job}");
    assert!(job.contains(native_host), "{job}");
    assert!(job.contains("catalog.rs"), "{job}");
    for tool in [
        "rust@$rust_version",
        "actionlint@$actionlint_version",
        "shellcheck@$shellcheck_version",
        "zizmor@$zizmor_version",
    ] {
        assert!(job.contains(tool), "{job}");
    }
    if target.triple.contains("apple-darwin") {
        assert!(job.contains("lipo -archs"), "{job}");
        assert!(job.contains("sysctl.proc_translated"), "{job}");
    }
    Ok(())
}

fn assert_behavior_corpus(job: &str) {
    assert!(
        job.contains("mise --no-config --no-env --no-hooks exec \\\"rust@"),
        "{job}"
    );
    for fixture in ["minimal-cargo", "multi-crate", "ignored-stack"] {
        assert!(job.contains(fixture), "{job}");
    }
    for fixture in ["malformed", "malformed-ignored"] {
        assert!(job.contains(fixture), "{job}");
    }
    assert!(job.contains("expected/ci.yml"), "{job}");
    assert!(job.contains("expected/actionlint.yaml"), "{job}");
    assert!(job.contains("expected/plan.txt"), "{job}");
    assert!(job.contains("expected/plan.stderr.txt"), "{job}");
    assert!(
        job.contains("scripts/capture-opentofu-goldens.sh check-release"),
        "{job}"
    );
    assert!(job.contains("generate --output-dir"), "{job}");
    assert!(job.contains("malformed_manifest:Cargo.toml:"), "{job}");
    assert!(!job.contains("cargo build"), "{job}");
    assert!(!job.contains("mbx build"), "{job}");
    assert!(!job.contains("rustup target add"), "{job}");
}

fn assert_publisher_gates(body: &str) -> Result<(), Box<dyn Error>> {
    let publish = super::super::job_body(body, "publish-generator")?;
    for job in [
        "qualify-linux-x64",
        "qualify-macos-arm64",
        "qualify-macos-x64",
    ] {
        assert!(publish.contains(&format!("- {job}\n")), "{publish}");
        assert!(
            publish.contains(&format!("needs.{job}.result == 'success'")),
            "{publish}"
        );
    }
    assert!(
        publish.contains("environment: generator-release\n"),
        "{publish}"
    );
    assert!(publish.contains("verify_release_environment"), "{publish}");
    assert!(publish.contains("verify_same_sha_ci"), "{publish}");
    Ok(())
}
