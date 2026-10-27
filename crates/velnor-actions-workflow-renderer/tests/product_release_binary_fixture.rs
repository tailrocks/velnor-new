// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::from_str;
use tempfile::TempDir;
use velnor_actions_contract::{RoutingWorkflow, Step, StepKind};
use velnor_actions_workflow_renderer::render::RenderedFile;
use velnor_actions_workflow_renderer::schema2::{
    ProductReleasePins, Schema2WorkflowRequest, render_schema2_workflows,
};
use velnor_actions_workflow_renderer::setup::MiseSetup;

use super::product_release_binary_hash_fixture::{install_shasum_double, sha256};

pub(crate) const SOURCE_SHA: &str = "a97f3c482da31b6f91f83e28c4dca0217e5b6930";
pub(crate) const HELPER_SUFFIX: &[u8] = b"helper payload\n";

const BINARY_WORKFLOW: &str = ".github/workflows/product-release-binary.yml";
const BUILD_STEP: &str = "Build velnor-host with MBX";
const VERIFY_STEP: &str = "Verify Mach-O architecture";
const CHECKSUM_STEP: &str = "Checksum built bytes";
const HELPER_NAME: &str = "velnor-runner-attestation-helper";
const HOST_NAME: &str = "velnor-host";
const HELPER_PACKAGE: &str = "velnor-runner-attestation";
const HELPER_BIN: &str = "velnor-runner-attestation-helper";
const HOST_PACKAGE: &str = "velnor-runner-cli";
const TARGET_DIR: &str = "crates/velnor-runner/target/release";

#[derive(Clone, Copy)]
pub(crate) enum BuildMode {
    Success,
    HelperFailure,
    MissingHelper,
    HostFailure,
    TamperedHelper,
    MissingSource,
    InitialHelperHashFailure,
    CopiedHelperHashFailure,
}

impl BuildMode {
    fn label(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::HelperFailure => "helper-failure",
            Self::MissingHelper => "missing-helper",
            Self::HostFailure => "host-failure",
            Self::TamperedHelper => "tampered-helper",
            Self::MissingSource => "missing-source",
            Self::InitialHelperHashFailure => "initial-helper-hash-failure",
            Self::CopiedHelperHashFailure => "copied-helper-hash-failure",
        }
    }

    fn tool_value(self) -> &'static str {
        match self {
            Self::MissingSource => "success",
            _ => self.label(),
        }
    }

    fn shasum_failure(self) -> &'static str {
        match self {
            Self::InitialHelperHashFailure => "initial-helper",
            Self::CopiedHelperHashFailure => "copied-helper",
            _ => "none",
        }
    }
}

pub(crate) struct BinaryReleaseFixture {
    _root: TempDir,
    workspace: PathBuf,
    fake_bin: PathBuf,
    target_dir: PathBuf,
    build_log: PathBuf,
    build_script: String,
    verify_script: String,
    checksum_script: String,
    evidence_dir: Option<PathBuf>,
    real_shasum: PathBuf,
    shasum_log: PathBuf,
    mode: BuildMode,
}

impl BinaryReleaseFixture {
    pub(crate) fn new(mode: BuildMode) -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let fake_bin = root.path().join("bin");
        let fake_mbx = fake_bin.join("mbx");
        let target_dir = workspace.join(TARGET_DIR);
        let build_log = root.path().join("build.log");
        let shasum_log = root.path().join("shasum.log");
        let evidence_dir = std::env::var_os("VELNOR_PR167_PRODUCER_EVIDENCE_DIR")
            .map(|path| PathBuf::from(path).join(mode.label()));
        fs::create_dir_all(&workspace)?;
        fs::create_dir_all(&fake_bin)?;
        let real_shasum = install_shasum_double(&fake_bin)?;
        if let Some(evidence_dir) = &evidence_dir {
            fs::create_dir_all(evidence_dir)?;
        }
        fs::write(&fake_mbx, FAKE_MBX)?;
        let mut permissions = fs::metadata(&fake_mbx)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_mbx, permissions)?;
        fs::write(&build_log, "")?;
        fs::write(&shasum_log, "")?;
        copy_release_inputs(&workspace)?;

        let request = Schema2WorkflowRequest {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            hosted_label: "ubuntu-26.04".to_owned(),
            scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
            workflows: BTreeSet::from([RoutingWorkflow::MacosBinaryRelease]),
            mbx_qualification: None,
            mise_pin_qualification: None,
            product_release: Some(pins()),
        };
        let files = render_schema2_workflows(&request)?;
        let workflow = rendered_file(&files, BINARY_WORKFLOW)?;
        let build_script = extract_step(workflow, BUILD_STEP)?;
        let verify_script = extract_step(workflow, VERIFY_STEP)?;
        let checksum_script = extract_step(workflow, CHECKSUM_STEP)?;
        if let Some(evidence_dir) = &evidence_dir {
            fs::write(evidence_dir.join("product-release-binary.yml"), workflow)?;
            fs::write(evidence_dir.join("build.sh"), &build_script)?;
            fs::write(evidence_dir.join("verify.sh"), &verify_script)?;
            fs::write(evidence_dir.join("checksum.sh"), &checksum_script)?;
        }

        Ok(Self {
            _root: root,
            workspace,
            fake_bin,
            target_dir,
            build_log,
            build_script,
            verify_script,
            checksum_script,
            evidence_dir,
            real_shasum,
            shasum_log,
            mode,
        })
    }

    pub(crate) fn execute_build(&self, source_sha: Option<&str>) -> Result<Output, Box<dyn Error>> {
        self.execute("build", &self.build_script, source_sha)
    }

    pub(crate) fn execute_verify(&self) -> Result<Output, Box<dyn Error>> {
        self.execute("verify", &self.verify_script, Some(SOURCE_SHA))
    }

    pub(crate) fn execute_checksum(&self) -> Result<Output, Box<dyn Error>> {
        self.execute("checksum", &self.checksum_script, Some(SOURCE_SHA))
    }

    fn execute(
        &self,
        label: &str,
        script: &str,
        source_sha: Option<&str>,
    ) -> Result<Output, Box<dyn Error>> {
        let original_path = std::env::var("PATH")?;
        let path = format!("{}:{original_path}", self.fake_bin.display());
        let mut command = Command::new("bash");
        command
            .args(["--noprofile", "--norc", "-e", "-c"])
            .arg(script)
            .current_dir(&self.workspace)
            .env("PATH", path)
            .env("BUILD_MODE", self.mode.tool_value())
            .env("BUILD_LOG", &self.build_log)
            .env("BUILD_OUTPUT_DIR", &self.target_dir)
            .env("REAL_SHASUM", &self.real_shasum)
            .env("SHASUM_FAILURE_AT", self.mode.shasum_failure())
            .env("SHASUM_LOG", &self.shasum_log)
            .env_remove("VELNOR_SOURCE_SHA");
        if let Some(source_sha) = source_sha {
            command.env("VELNOR_SOURCE_SHA", source_sha);
        }
        let output = command.output()?;
        self.save_evidence(label, &output)?;
        Ok(output)
    }

    fn save_evidence(&self, label: &str, output: &Output) -> Result<(), Box<dyn Error>> {
        let Some(evidence_dir) = &self.evidence_dir else {
            return Ok(());
        };
        fs::write(
            evidence_dir.join(format!("{label}.status")),
            format!("{:?}\n", output.status),
        )?;
        fs::write(evidence_dir.join(format!("{label}.stdout")), &output.stdout)?;
        fs::write(evidence_dir.join(format!("{label}.stderr")), &output.stderr)?;
        fs::write(
            evidence_dir.join(format!("{label}.build-log")),
            self.build_log()?,
        )?;
        fs::write(
            evidence_dir.join(format!("{label}-shasum.log")),
            self.shasum_receipt()?,
        )?;
        let snapshot = evidence_dir.join(format!("{label}-files"));
        fs::create_dir_all(&snapshot)?;
        let names = ["LICENSE", "NOTICE", HOST_NAME, HELPER_NAME, "SHA256SUMS"];
        let mut hashes = String::new();
        for name in names {
            let path = self.workspace.join(name);
            if path.is_file() {
                let bytes = fs::read(&path)?;
                writeln!(hashes, "{}  {name}", sha256(&bytes))?;
                fs::copy(path, snapshot.join(name))?;
            }
        }
        for name in [HOST_NAME, HELPER_NAME] {
            let source = self.target_dir.join(name);
            if source.is_file() {
                let bytes = fs::read(&source)?;
                writeln!(hashes, "{}  target/{name}", sha256(&bytes))?;
                fs::copy(source, snapshot.join(format!("target-{name}")))?;
            }
        }
        fs::write(evidence_dir.join(format!("{label}.sha256")), hashes)?;
        Ok(())
    }

    pub(crate) fn build_log(&self) -> Result<String, Box<dyn Error>> {
        Ok(fs::read_to_string(&self.build_log)?)
    }

    pub(crate) fn shasum_receipt(&self) -> Result<String, Box<dyn Error>> {
        Ok(fs::read_to_string(&self.shasum_log)?)
    }

    pub(crate) fn workspace(&self) -> &Path {
        &self.workspace
    }
}

fn copy_release_inputs(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for name in ["LICENSE", "NOTICE"] {
        fs::copy(repository.join(name), workspace.join(name))?;
    }
    Ok(())
}

fn rendered_file<'a>(files: &'a [RenderedFile], path: &str) -> Result<&'a str, Box<dyn Error>> {
    let file = files
        .iter()
        .find(|file| file.path == path)
        .ok_or("binary release workflow is missing")?;
    Ok(&file.bytes)
}

fn extract_step(workflow: &str, name: &str) -> Result<String, Box<dyn Error>> {
    let marker = format!("name: {name}");
    let step = workflow
        .split_once(&marker)
        .map(|(_, remainder)| remainder)
        .ok_or("release workflow step is missing")?;
    let encoded = step
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("run: "))
        .ok_or("release workflow run script is missing")?;
    Ok(from_str(encoded)?)
}

fn pins() -> ProductReleasePins {
    let setup = MiseSetup {
        uses: "jdx/mise-action@f1f6d8a6c2a1f6a79d5216a1d7e3f1b0e4d7c1a9".to_owned(),
        version: "2026.10.19".to_owned(),
        sha256: "c".repeat(64),
    };
    let verify_lint_script =
        velnor_actions_contract::config::jactionlint_verifier::jactionlint_verifier_script(
            "2.83.1",
            velnor_actions_workflow_renderer::VERIFY_PATH,
        );
    ProductReleasePins {
        linux_x86_64_setup: setup.clone(),
        macos_arm64_setup: setup.clone(),
        macos_x86_64_setup: setup,
        install_gate_tools_argv: argv(),
        install_build_tools_argv: argv(),
        install_qualify_tools_argv: argv(),
        qualify_mbx_action_steps: std::array::from_fn(|_| fixture_step()),
        install_runner_build_tools_argv: argv(),
        install_gh_argv: argv(),
        build_argv: argv(),
        intel_build_argv: argv(),
        install_intel_target_argv: argv(),
        install_resource_probe_target_argv: argv(),
        runner_build_argv: build_argv(HOST_PACKAGE, HOST_NAME),
        attestation_helper_build_argv: build_argv(HELPER_PACKAGE, HELPER_BIN),
        resource_probe_build_argv: argv(),
        install_resource_probe_release_tools_argv: argv(),
        resource_probe_release_build_argv: argv(),
        resource_probe_release_test_argv: argv(),
        lint_argv: argv(),
        verify_lint_argv: vec!["sh".to_owned(), "-c".to_owned(), verify_lint_script],
        gh_argv: argv(),
        rust_version: "1.99.1".to_owned(),
        verify_rust_argv: vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!(
                "{} true",
                velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude()
            ),
        ],
        mr_boxington_version: "1.28.0".to_owned(),
        gh_version: "2.83.1".to_owned(),
    }
}

fn build_argv(package: &str, binary: &str) -> Vec<String> {
    [
        "mbx",
        "build",
        "--locked",
        "--manifest-path",
        "crates/velnor-runner/Cargo.toml",
        "--release",
        "--package",
        package,
        "--bin",
        binary,
    ]
    .map(str::to_owned)
    .to_vec()
}

fn argv() -> Vec<String> {
    vec!["true".to_owned()]
}

fn fixture_step() -> Step {
    Step {
        name: "fixture".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    }
}

const FAKE_MBX: &str = r#"#!/bin/sh
set -eu
case " $* " in
  *" --package velnor-runner-attestation "*)
    printf 'helper\n' >> "$BUILD_LOG"
    case "$BUILD_MODE" in missing-helper) exit 0 ;; esac
    mkdir -p "$BUILD_OUTPUT_DIR"
    printf '\317\372\355\376\014\000\000\001' > "$BUILD_OUTPUT_DIR/velnor-runner-attestation-helper"
    dd if=/dev/zero bs=24 count=1 2>/dev/null >> "$BUILD_OUTPUT_DIR/velnor-runner-attestation-helper"
    printf '%s\n' 'helper payload' >> "$BUILD_OUTPUT_DIR/velnor-runner-attestation-helper"
    case "$BUILD_MODE" in helper-failure) exit 31 ;; esac
    ;;
  *" --package velnor-runner-cli "*)
    printf 'host source=%s helper=%s\n' "${VELNOR_RUNNER_BUILD_SOURCE_SHA-unbound}" "${VELNOR_RUNNER_BUILD_HELPER_SHA256-unbound}" >> "$BUILD_LOG"
    mkdir -p "$BUILD_OUTPUT_DIR"
    printf '\317\372\355\376\014\000\000\001' > "$BUILD_OUTPUT_DIR/velnor-host"
    dd if=/dev/zero bs=24 count=1 2>/dev/null >> "$BUILD_OUTPUT_DIR/velnor-host"
    printf 'source=%s\nhelper=%s\n' "${VELNOR_RUNNER_BUILD_SOURCE_SHA-unbound}" "${VELNOR_RUNNER_BUILD_HELPER_SHA256-unbound}" >> "$BUILD_OUTPUT_DIR/velnor-host"
    if [ "$BUILD_MODE" = tampered-helper ]; then
      printf '%s\n' 'changed helper bytes' > "$BUILD_OUTPUT_DIR/velnor-runner-attestation-helper"
    fi
    case "$BUILD_MODE" in host-failure) exit 32 ;; esac
    ;;
  *) exit 33 ;;
esac
"#;
