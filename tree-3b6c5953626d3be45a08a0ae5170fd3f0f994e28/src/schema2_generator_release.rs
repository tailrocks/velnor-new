//! Protected generator release: Linux x64 and macOS arm64/x64 assets.
//!
//! The SHA-named release uses the workspace version and leaves version tags
//! untouched. Publishing requires protected `main`, same-SHA CI, and review.

use crate::RenderError;
use crate::runs_on::runs_on_yaml;
use crate::yaml::Yaml;

use super::Schema2WorkflowRequest;
use super::features::{CHECKOUT_USES, base, finish};

#[path = "schema2_generator_release_steps.rs"]
mod release_steps;
#[path = "schema2_generator_release_scripts.rs"]
mod scripts;

/// Default arm64 macOS runner. Intel output uses the pinned Rust target here.
const MACOS_RUNS_ON: &str = "macos-15";
/// Pinned `jdx/mise-action` commit (v5.0.0).
const MISE_USES: &str = "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5";
/// Pinned Mise binary release.
const MISE_VERSION: &str = "2026.9.18";
/// Pinned provenance action commit (v4.2.2).
const ATTEST_USES: &str =
    "actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8";
const LINUX_ARTIFACT: &str = "generator-linux-x64-assets";
const MACOS_ARM_ARTIFACT: &str = "generator-macos-arm64-assets";
const MACOS_X64_ARTIFACT: &str = "generator-macos-x64-assets";
const LINUX_DIR: &str = "linux-assets";
const MACOS_ARM_DIR: &str = "macos-arm64-assets";
const MACOS_X64_DIR: &str = "macos-x64-assets";
const ASSET_DIR: &str = "assets";
const LINUX_TARGET: &str = "x86_64-unknown-linux-gnu";
const MACOS_ARM_TARGET: &str = "aarch64-apple-darwin";
const MACOS_X64_TARGET: &str = "x86_64-apple-darwin";

struct AssetNames {
    linux_bin: String,
    linux_sum: String,
    macos_arm_bin: String,
    macos_arm_sum: String,
    macos_x64_bin: String,
    macos_x64_sum: String,
}

impl AssetNames {
    fn for_version(version: &str) -> Self {
        let prefix = format!("velnor-actions-{version}");
        Self {
            linux_bin: format!("{prefix}-{LINUX_TARGET}"),
            linux_sum: format!("{prefix}-{LINUX_TARGET}.sha256"),
            macos_arm_bin: format!("{prefix}-{MACOS_ARM_TARGET}"),
            macos_arm_sum: format!("{prefix}-{MACOS_ARM_TARGET}.sha256"),
            macos_x64_bin: format!("{prefix}-{MACOS_X64_TARGET}"),
            macos_x64_sum: format!("{prefix}-{MACOS_X64_TARGET}.sha256"),
        }
    }
}

/// Render a main-only release gate, three builds, attestations, and publisher.
///
/// # Errors
///
/// Illegal hosted or macOS labels fail.
pub(super) fn generator_release(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let hosted = runs_on_yaml(&request.hosted_label)?;
    let macos = runs_on_yaml(MACOS_RUNS_ON)?;
    let assets = AssetNames::for_version(&request.version);
    let mut jobs = vec![release_gate_job(hosted.clone())];
    jobs.extend(build_jobs(hosted.clone(), macos, &assets));
    jobs.extend(attest_jobs(hosted.clone(), &assets));
    jobs.push(prepare_manifest_job(
        hosted.clone(),
        &request.version,
        &assets,
    ));
    jobs.push(publish_job(hosted, &request.version, &assets));
    Ok(document(jobs))
}

fn build_jobs(hosted: Yaml, macos: Yaml, assets: &AssetNames) -> Vec<(String, Yaml)> {
    vec![
        build_job(
            "build-linux-x64",
            "Build Linux x86_64 velnor-actions",
            hosted,
            BuildSpec {
                target: LINUX_TARGET,
                cross_compile: false,
                os: "linux",
                asset: &assets.linux_bin,
                sidecar: &assets.linux_sum,
                sum_command: "sha256sum",
                artifact: LINUX_ARTIFACT,
            },
        ),
        build_job(
            "build-macos-arm64",
            "Build macOS arm64 velnor-actions",
            macos.clone(),
            BuildSpec {
                target: MACOS_ARM_TARGET,
                cross_compile: false,
                os: "macos",
                asset: &assets.macos_arm_bin,
                sidecar: &assets.macos_arm_sum,
                sum_command: "shasum -a 256",
                artifact: MACOS_ARM_ARTIFACT,
            },
        ),
        build_job(
            "build-macos-x64",
            "Build macOS x86_64 velnor-actions",
            macos,
            BuildSpec {
                target: MACOS_X64_TARGET,
                cross_compile: true,
                os: "macos",
                asset: &assets.macos_x64_bin,
                sidecar: &assets.macos_x64_sum,
                sum_command: "shasum -a 256",
                artifact: MACOS_X64_ARTIFACT,
            },
        ),
    ]
}

#[derive(Clone, Copy)]
struct BuildSpec<'a> {
    target: &'a str,
    cross_compile: bool,
    os: &'a str,
    asset: &'a str,
    sidecar: &'a str,
    sum_command: &'a str,
    artifact: &'a str,
}

fn build_job(id: &str, name: &str, runs_on: Yaml, spec: BuildSpec<'_>) -> (String, Yaml) {
    let files = [spec.asset, spec.sidecar];
    let mut steps = vec![release_steps::checkout_step(), release_steps::mise_step()];
    steps.push(release_steps::bash_run_step(
        "Install catalog tools and verify MBX",
        &scripts::install_tools(spec.os),
    ));
    if spec.cross_compile {
        steps.push(release_steps::bash_run_step(
            "Install pinned Intel Rust target",
            &scripts::install_rust_target(spec.target),
        ));
    }
    steps.push(release_steps::bash_run_step(
        "Build with MBX and checksum target",
        &scripts::build(
            spec.target,
            spec.cross_compile,
            spec.os,
            spec.asset,
            spec.sidecar,
            spec.sum_command,
        ),
    ));
    steps.push(release_steps::upload_step(name, spec.artifact, &files));
    let fields = with_if(
        with_needs(
            with_permissions(base(name, runs_on, 120), release_steps::build_permissions()),
            &["release-gate"],
        ),
        "needs.release-gate.result == 'success'",
    );
    finish(id, fields, steps)
}

fn attest_jobs(hosted: Yaml, assets: &AssetNames) -> Vec<(String, Yaml)> {
    vec![
        attest_job(
            "attest-linux-x64",
            "Attest Linux x86_64 velnor-actions",
            hosted.clone(),
            "build-linux-x64",
            LINUX_ARTIFACT,
            &[&assets.linux_bin, &assets.linux_sum],
        ),
        attest_job(
            "attest-macos-arm64",
            "Attest macOS arm64 velnor-actions",
            hosted.clone(),
            "build-macos-arm64",
            MACOS_ARM_ARTIFACT,
            &[&assets.macos_arm_bin, &assets.macos_arm_sum],
        ),
        attest_job(
            "attest-macos-x64",
            "Attest macOS x86_64 velnor-actions",
            hosted,
            "build-macos-x64",
            MACOS_X64_ARTIFACT,
            &[&assets.macos_x64_bin, &assets.macos_x64_sum],
        ),
    ]
}

fn attest_job(
    id: &str,
    name: &str,
    runs_on: Yaml,
    build: &str,
    artifact: &str,
    files: &[&str],
) -> (String, Yaml) {
    let job_name = format!("needs.{build}.result == 'success'");
    let fields = with_if(
        with_needs(
            with_permissions(base(name, runs_on, 20), release_steps::attest_permissions()),
            &["release-gate", build],
        ),
        &format!("needs.release-gate.result == 'success' && {job_name}"),
    );
    finish(
        id,
        fields,
        vec![
            release_steps::download_step("Download built assets", artifact, ASSET_DIR),
            release_steps::attest_step(&release_steps::subject_list(files)),
        ],
    )
}

fn release_gate_job(hosted: Yaml) -> (String, Yaml) {
    let fields = with_if(
        with_permissions(
            base("Release eligibility gate", hosted, 15),
            release_steps::gate_permissions(),
        ),
        "github.event_name == 'workflow_dispatch' && github.repository == 'tailrocks/velnor-new' && github.ref == 'refs/heads/main' && github.ref_protected",
    );
    finish(
        "release-gate",
        fields,
        vec![
            release_steps::checkout_step(),
            release_steps::mise_step(),
            release_steps::token_bash_run_step(
                "Require protected main, same-SHA CI, and environment",
                &scripts::release_gate(),
            ),
        ],
    )
}

fn prepare_manifest_job(hosted: Yaml, version: &str, assets: &AssetNames) -> (String, Yaml) {
    let needs = [
        "release-gate",
        "attest-linux-x64",
        "attest-macos-arm64",
        "attest-macos-x64",
    ];
    let needs_success = needs
        .iter()
        .map(|job| format!("needs.{job}.result == 'success'"))
        .collect::<Vec<_>>()
        .join(" && ");
    let fields = with_if(
        with_needs(
            with_permissions(
                base("Prepare and attest release manifest", hosted, 30),
                release_steps::manifest_permissions(),
            ),
            &needs,
        ),
        &needs_success,
    );
    let manifest = format!("release-manifest/velnor-actions-release-manifest-{version}.json");
    finish(
        "prepare-manifest",
        fields,
        vec![
            release_steps::checkout_step(),
            release_steps::mise_step(),
            release_steps::download_step("Download Linux x86_64 assets", LINUX_ARTIFACT, LINUX_DIR),
            release_steps::download_step(
                "Download macOS arm64 assets",
                MACOS_ARM_ARTIFACT,
                MACOS_ARM_DIR,
            ),
            release_steps::download_step(
                "Download macOS x86_64 assets",
                MACOS_X64_ARTIFACT,
                MACOS_X64_DIR,
            ),
            release_steps::token_bash_run_step(
                "Validate assets and create canonical manifest",
                &scripts::prepare_manifest(version, assets),
            ),
            release_steps::attest_step(&manifest),
            release_steps::upload_step(
                "Upload attested release manifest",
                "generator-release-manifest",
                &[&manifest],
            ),
        ],
    )
}

fn publish_job(hosted: Yaml, version: &str, assets: &AssetNames) -> (String, Yaml) {
    let needs = ["release-gate", "prepare-manifest"];
    let needs_success = needs
        .iter()
        .map(|job| format!("needs.{job}.result == 'success'"))
        .collect::<Vec<_>>()
        .join(" && ");
    let mut fields = base("Publish velnor-actions release", hosted, 30);
    fields.push(("environment".to_owned(), Yaml::str("generator-release")));
    let fields = with_if(
        with_needs(
            with_permissions(fields, release_steps::publish_permissions()),
            &needs,
        ),
        &needs_success,
    );
    finish(
        "publish-generator",
        fields,
        vec![
            release_steps::checkout_step(),
            release_steps::mise_step(),
            release_steps::download_step("Download Linux x86_64 assets", LINUX_ARTIFACT, LINUX_DIR),
            release_steps::download_step(
                "Download macOS arm64 assets",
                MACOS_ARM_ARTIFACT,
                MACOS_ARM_DIR,
            ),
            release_steps::download_step(
                "Download macOS x86_64 assets",
                MACOS_X64_ARTIFACT,
                MACOS_X64_DIR,
            ),
            release_steps::download_step(
                "Download attested release manifest",
                "generator-release-manifest",
                "release-manifest",
            ),
            release_steps::publish_bash_step(&scripts::publish(version, assets)),
        ],
    )
}

fn with_permissions(mut fields: Vec<(String, Yaml)>, permissions: Yaml) -> Vec<(String, Yaml)> {
    fields.push(("permissions".to_owned(), permissions));
    fields
}

fn with_needs(mut fields: Vec<(String, Yaml)>, jobs: &[&str]) -> Vec<(String, Yaml)> {
    fields.push((
        "needs".to_owned(),
        Yaml::Seq(jobs.iter().copied().map(Yaml::str).collect()),
    ));
    fields
}

fn with_if(mut fields: Vec<(String, Yaml)>, condition: &str) -> Vec<(String, Yaml)> {
    fields.insert(1, ("if".to_owned(), Yaml::str(condition)));
    fields
}

fn document(jobs: Vec<(String, Yaml)>) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Generator release")),
        (
            "on".to_owned(),
            Yaml::Map(vec![("workflow_dispatch".to_owned(), Yaml::Map(vec![]))]),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ])
}
