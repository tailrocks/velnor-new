use super::{ConsumerBinaryReleaseSpec, render_consumer_binary_release};
use crate::workflow_routing::WorkflowRoutingPolicy;
use crate::{HelperProvenance, provision_acquire_step, setup::MiseSetup};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};
use velnor_actions_contract::{
    PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlan, PackageReleasePlanProduct,
    package_release::{PackageReleaseChannel, PackageReleasePayloadFormat},
};

fn install_argv(tools: &[&str]) -> Vec<String> {
    let mut command = vec![
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "install".to_owned(),
    ];
    command.extend(tools.iter().map(|tool| (*tool).to_owned()));
    command
}

fn exec_argv(tool: &str, program: &str, args: &[&str]) -> Vec<String> {
    let mut command = vec![
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "exec".to_owned(),
        tool.to_owned(),
        "--".to_owned(),
        program.to_owned(),
    ];
    command.extend(args.iter().map(|arg| (*arg).to_owned()));
    command
}

fn spec() -> ConsumerBinaryReleaseSpec {
    let linux_setup = MiseSetup {
        uses: "jdx/mise-action@9b200bcecbfd6303e69a4a261b2e09e3757440a7".to_owned(),
        version: "2026.10.13".to_owned(),
        sha256: "94c45523c7b6760b7da704f16051171ca32e8447a849a02266a439a4455e9a12".to_owned(),
    };
    ConsumerBinaryReleaseSpec {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        plan: PackageReleasePlan {
            schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
            channel: PackageReleaseChannel::Stable,
            repository: "example/repo-scan".to_owned(),
            default_branch: "stable".to_owned(),
            base_version: "0.4.3".to_owned(),
            products: vec![PackageReleasePlanProduct {
                id: "repo-scan".to_owned(),
                package: "repo-scan".to_owned(),
                bins: vec!["repo-scan".to_owned()],
                targets: vec!["aarch64-apple-darwin".to_owned()],
                format: PackageReleasePayloadFormat::RawBinary,
            }],
            companions: Vec::new(),
        },
        repository: "example/repo-scan".to_owned(),
        default_branch: "stable".to_owned(),
        manifest_path: "Cargo.toml".to_owned(),
        package: "repo-scan".to_owned(),
        bin: "repo-scan".to_owned(),
        linux_setup,
        macos_setup: MiseSetup {
            uses: "jdx/mise-action@9b200bcecbfd6303e69a4a261b2e09e3757440a7".to_owned(),
            version: "2026.10.13".to_owned(),
            sha256: "92fd6898dc213808d1c81f0bead9668dbea036e5946c94c0e82554603205a3aa".to_owned(),
        },
        install_tools_argv: install_argv(&["rust@1.98.1", "gh@2.102.0"]),
        install_gh_argv: install_argv(&["gh@2.102.0"]),
        install_rust_argv: install_argv(&["rust@1.98.1"]),
        metadata_argv: exec_argv("rust@1.98.1", "cargo", &["metadata", "--format-version", "1", "--locked", "--no-deps", "--manifest-path", "Cargo.toml"]),
        add_target_argv: exec_argv("rust@1.98.1", "rustup", &["target", "add", "--toolchain", "1.98.1-aarch64-apple-darwin", "aarch64-apple-darwin"]),
        build_argv: exec_argv("rust@1.98.1", "cargo", &["build", "--release", "--locked", "--manifest-path", "Cargo.toml", "--package", "repo-scan", "--bin", "repo-scan", "--target", "aarch64-apple-darwin"]),
        gh_argv: exec_argv("gh@2.102.0", "gh", &[]),
        linux_acquire: provision_acquire_step(
            &HelperProvenance::ReleaseAsset {
                url: "https://github.com/tailrocks/velnor/releases/download/v0.1.12/velnor-actions-0.1.12-x86_64-unknown-linux-gnu".to_owned(),
                sha256: "a".repeat(64),
                commit: "b".repeat(40),
            },
            vec![
                "sh".to_owned(),
                "-c".to_owned(),
                r#"curl -fsSL "$VELNOR_ASSET_URL" -o "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.12" && printf '%s  %s\n' "$VELNOR_ASSET_SHA256" "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.12" | sha256sum -c -"#.to_owned(),
            ],
        ).expect("verified helper acquisition"),
    }
}

#[test]
fn stable_renderer_uses_common_receipt_ops_and_preserves_manual_approval_boundary() {
    let rendered = render_consumer_binary_release(&spec()).expect("workflow renders");
    let yaml = rendered.bytes;
    assert_eq!(rendered.path, ".github/workflows/binary-release.yml");
    assert!(yaml.contains("workflow_dispatch: {}"));
    assert!(!yaml.contains("push:"));
    assert!(yaml.contains("consumer-binary-release-${{ github.repository }}"));
    assert!(yaml.contains("raw-binary"));
    assert!(yaml.contains("sign-package-release-v1"));
    assert!(yaml.contains("publish-package-release-v1"));
    assert!(yaml.contains("consumer-binary-release"));
    assert!(yaml.contains("IMMUTABILITY_READ_TOKEN"));
    assert!(yaml.contains("attestations: write"));
    assert!(yaml.contains("contents: write"));
    assert!(!yaml.contains("actions: write"));
    assert!(!yaml.contains("gh release create"));
    assert!(!yaml.contains("gh release edit"));
    assert!(!yaml.contains("make_latest=true"));
    assert!(yaml.contains("aarch64-apple-darwin"));
    assert!(yaml.contains("CARGO_TARGET_DIR"));
}

#[test]
fn stable_renderer_rejects_tarball_plan_without_changing_public_package_config() {
    let mut invalid = spec();
    invalid.plan.products[0].format = PackageReleasePayloadFormat::TarGz;
    assert!(render_consumer_binary_release(&invalid).is_err());
}

#[test]
fn stable_renderer_rejects_divergent_branch_authorities() {
    let mut invalid = spec();
    invalid.plan.default_branch = "other-main".to_owned();
    assert!(render_consumer_binary_release(&invalid).is_err());
}

#[test]
fn executable_stable_build_emits_raw_handoff_for_package_name_distinct_from_bin() {
    let scratch = Scratch::new();
    let (workspace, source_sha, metadata, runner_temp) = stable_fixture_workspace(&scratch);
    let output = scratch.path().join("identity.out");
    let identity = super::scripts::identity("cat \"$METADATA_FILE\"");
    let mut environment = stable_identity_environment(&workspace, &output, &metadata);
    let result = run(&identity, &workspace, &environment, None);
    assert!(
        result.status.success(),
        "identity: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        fs::read_to_string(&output)
            .expect("identity output")
            .contains("tag=demo-package-v0.4.3")
    );

    let fake_bin = fake_file_tool(&scratch);
    environment.extend(stable_build_environment(&source_sha, &runner_temp));
    let build = super::scripts::build(
        &identity,
        "true",
        "mkdir -p \"$CARGO_TARGET_DIR/$TARGET_TRIPLE/release\"; printf raw-binary > \"$CARGO_TARGET_DIR/$TARGET_TRIPLE/release/$BINARY_NAME\"",
    );
    let result = run(&build, &workspace, &environment, Some(&fake_bin));
    assert!(
        result.status.success(),
        "build: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_raw_handoff(&runner_temp, &source_sha);
    assert_locked_metadata_mismatch_rejected(&workspace, environment, &fake_bin);
}

fn stable_fixture_workspace(scratch: &Scratch) -> (PathBuf, String, PathBuf, PathBuf) {
    let workspace = scratch.path().join("workspace");
    fs::create_dir_all(workspace.join("src")).expect("workspace");
    fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"demo-package\"\nversion = \"0.4.3\"\nedition = \"2021\"\n\n[[bin]]\nname = \"demo-cli\"\npath = \"src/main.rs\"\n",
    )
    .expect("manifest");
    fs::write(workspace.join("src/main.rs"), "fn main() {}\n").expect("source");
    fs::write(
        workspace.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"demo-package\"\nversion = \"0.4.3\"\n",
    )
    .expect("lockfile");
    git(&workspace, &["init", "-q"]);
    git(
        &workspace,
        &["config", "user.email", "test@example.invalid"],
    );
    git(&workspace, &["config", "user.name", "Velnor tests"]);
    git(&workspace, &["add", "Cargo.toml", "Cargo.lock", "src"]);
    git(&workspace, &["commit", "-q", "-m", "fixture"]);
    let source_sha = git_output(&workspace, &["rev-parse", "HEAD"]);
    let canonical_workspace = fs::canonicalize(&workspace).expect("canonical workspace");
    let metadata = scratch.path().join("metadata.json");
    fs::write(
        &metadata,
        format!(
            r#"{{"packages":[{{"name":"demo-package","version":"0.4.3","id":"path+file:///workspace#demo-package@0.4.3","manifest_path":"{}/Cargo.toml","targets":[{{"name":"demo-cli","kind":["bin"],"required-features":[]}}]}}],"workspace_members":["path+file:///workspace#demo-package@0.4.3"],"workspace_root":"{}"}}"#,
            canonical_workspace.display(),
            canonical_workspace.display()
        ),
    )
    .expect("metadata");
    let runner_temp = scratch.path().join("runner");
    fs::create_dir(&runner_temp).expect("runner temp");
    (workspace, source_sha, metadata, runner_temp)
}

fn stable_identity_environment(
    workspace: &Path,
    output: &Path,
    metadata: &Path,
) -> Vec<(&'static str, String)> {
    vec![
        ("GITHUB_WORKSPACE", workspace.display().to_string()),
        ("GITHUB_OUTPUT", output.display().to_string()),
        ("MANIFEST_PATH", "Cargo.toml".to_owned()),
        ("PACKAGE_NAME", "demo-package".to_owned()),
        ("BINARY_NAME", "demo-cli".to_owned()),
        ("EXPECTED_VERSION", "0.4.3".to_owned()),
        ("METADATA_FILE", metadata.display().to_string()),
    ]
}

fn fake_file_tool(scratch: &Scratch) -> PathBuf {
    let fake_bin = scratch.path().join("bin");
    fs::create_dir(&fake_bin).expect("fake bin");
    let fake_file = fake_bin.join("file");
    fs::write(
        &fake_file,
        "#!/bin/sh\nprintf 'Mach-O 64-bit executable arm64\\n'\n",
    )
    .expect("fake file");
    fs::set_permissions(&fake_file, fs::Permissions::from_mode(0o755)).expect("executable");
    fake_bin
}

fn stable_build_environment(source_sha: &str, runner_temp: &Path) -> Vec<(&'static str, String)> {
    vec![
        ("GITHUB_SHA", source_sha.to_owned()),
        ("GITHUB_REF", "refs/heads/stable".to_owned()),
        ("GITHUB_RUN_ID", "12".to_owned()),
        ("GITHUB_RUN_ATTEMPT", "1".to_owned()),
        ("RUNNER_TEMP", runner_temp.display().to_string()),
        ("TARGET_TRIPLE", "aarch64-apple-darwin".to_owned()),
        ("EXPECTED_TAG", "demo-package-v0.4.3".to_owned()),
    ]
}

fn assert_raw_handoff(runner_temp: &Path, source_sha: &str) {
    let producer = runner_temp.join("velnor/preview-output");
    let payload = producer.join("demo-cli-aarch64-apple-darwin");
    assert_eq!(fs::read(&payload).expect("raw payload"), b"raw-binary");
    let manifest = velnor_actions_contract::parse_strict_json(
        &fs::read_to_string(producer.join("package-output.json")).expect("handoff"),
    )
    .expect("handoff JSON");
    assert_eq!(manifest["schema"], "velnor.package-output.v1");
    assert_eq!(manifest["source_commit"], source_sha);
    assert_eq!(manifest["source_ref"], "refs/heads/stable");
    assert_eq!(manifest["version"], "0.4.3");
    assert_eq!(manifest["payloads"][0]["product_id"], "demo-package");
    assert_eq!(manifest["payloads"][0]["format"], "raw-binary");
    assert_eq!(
        manifest["payloads"][0]["name"],
        "demo-cli-aarch64-apple-darwin"
    );
    assert_eq!(
        manifest["sboms"][0]["payload_name"],
        manifest["payloads"][0]["name"]
    );
    assert_eq!(
        manifest["sboms"][0]["sha256"].as_str().map(str::len),
        Some(64)
    );
}

fn assert_locked_metadata_mismatch_rejected(
    workspace: &Path,
    mut environment: Vec<(&'static str, String)>,
    fake_bin: &Path,
) {
    fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"demo-package\"\nversion = \"0.4.4\"\nedition = \"2021\"\n\n[[bin]]\nname = \"demo-cli\"\npath = \"src/main.rs\"\n",
    )
    .expect("introduce an uncommitted lockfile mismatch");
    let locked_identity = super::scripts::identity(
        "cargo metadata --format-version 1 --locked --no-deps --manifest-path Cargo.toml",
    );
    let failed_output = workspace.join("locked-mismatch.out");
    for (key, value) in &mut environment {
        if *key == "GITHUB_OUTPUT" {
            *value = failed_output.display().to_string();
        }
    }
    let result = run(&locked_identity, workspace, &environment, Some(fake_bin));
    assert!(
        !result.status.success(),
        "--locked metadata must reject stale Cargo.lock"
    );
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "velnor-stable-build-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("scratch directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn run(script: &str, cwd: &Path, env: &[(&str, String)], path_prefix: Option<&Path>) -> Output {
    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg(script)
        .current_dir(cwd)
        .envs(env.iter().cloned());
    if let Some(prefix) = path_prefix {
        let path = std::env::var_os("PATH").unwrap_or_default();
        command.env(
            "PATH",
            format!("{}:{}", prefix.display(), path.to_string_lossy()),
        );
    }
    command.output().expect("run generated shell")
}

fn git(cwd: &Path, args: &[&str]) {
    let result = crate::schema2::git_fixture::command(cwd)
        .expect("fixture git command")
        .args(args)
        .output()
        .expect("git invocation");
    assert!(
        result.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn git_output(cwd: &Path, args: &[&str]) -> String {
    let result = crate::schema2::git_fixture::command(cwd)
        .expect("fixture git command")
        .args(args)
        .output()
        .expect("git invocation");
    assert!(result.status.success(), "git {args:?}");
    String::from_utf8(result.stdout)
        .expect("git output")
        .trim()
        .to_owned()
}

#[test]
fn scale_set_rejects_the_native_macos_binary_build_job() {
    let policy = WorkflowRoutingPolicy::ScaleSet(
        crate::schema2::Schema2WorkflowRequest::canonical_scale_set().expect("selector"),
    );
    let error = super::render_consumer_binary_release_with_routing(&spec(), &policy)
        .expect_err("macOS binary build cannot run on a Linux Scale Set");
    assert!(
        error.to_string().contains(
            "unsupported_darwin_workload:.github/workflows/binary-release.yml:build-binary:macos-15"
        ),
        "{error}"
    );
}
