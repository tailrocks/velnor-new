// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeSet,
    error::Error,
    fs,
    path::Path,
    process::{Command, Output},
};

use velnor_actions_contract::RoutingWorkflow;
use velnor_actions_workflow_renderer::schema2::{Schema2WorkflowRequest, render_schema2_workflows};

mod assets;

const REJECTION: &str = "external attestation rejection: passive subject is not authenticated";

#[test]
fn public_completed_generator_integrity_reaches_rejecting_attestation_boundary()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let output = fixture.execute()?;
    assert_rejected(&fixture, &output)?;
    assert_eq!(output.status.code(), Some(17));
    let stdout = String::from_utf8_lossy(&output.stdout);
    for (target, _) in assets::TARGETS {
        assert!(
            stdout.contains(&format!("{}: OK", assets::binary_name(target))),
            "owned integrity verifier omitted {target}: {stdout}"
        );
    }
    assert!(String::from_utf8_lossy(&output.stderr).contains(REJECTION));
    Ok(())
}

#[test]
fn public_completed_generator_rejects_corrupt_downloaded_integrity() -> Result<(), Box<dyn Error>> {
    for kind in ["binary", "source", "toolchain", "manifest"] {
        let fixture = Fixture::new()?;
        assets::corrupt(&fixture.root.path().join("assets"), kind)?;
        let output = fixture.execute()?;
        assert_rejected(&fixture, &output)?;
        assert_eq!(
            output.status.code(),
            Some(1),
            "unexpected integrity rejection for {kind}: {output:?}"
        );
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains(REJECTION),
            "corrupt {kind} reached external attestation boundary"
        );
        if kind == "binary" {
            assert!(String::from_utf8_lossy(&output.stdout).contains(": FAILED"));
        }
        if kind == "manifest" {
            assert!(String::from_utf8_lossy(&output.stdout).contains("differ"));
        }
    }
    Ok(())
}

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        fs::create_dir(root.path().join("checkout"))?;
        fs::create_dir(root.path().join("assets"))?;
        assets::create(&root.path().join("assets"))?;
        fs::write(
            root.path().join("gh-transport.sh"),
            include_str!("completed_generator_verification/gh_transport.sh"),
        )?;
        let run = reconcile_script(root.path())?;
        fs::write(root.path().join("reconcile.sh"), run)?;
        Ok(Self { root })
    }

    fn execute(&self) -> Result<Output, Box<dyn Error>> {
        let root = self.root.path();
        let response = assets::release_response(&root.join("assets"))?;
        fs::write(root.join("releases.json"), serde_json::to_vec(&response)?)?;
        Ok(Command::new("bash")
            .arg(root.join("reconcile.sh"))
            .current_dir(root.join("checkout"))
            .env_clear()
            .env("PATH", std::env::var_os("PATH").ok_or("PATH missing")?)
            .env("HOME", root)
            .env("LC_ALL", "C")
            .env("GH_TOKEN", "unverified-transport-fixture-token")
            .env("VELNOR_SOURCE_SHA", assets::SOURCE)
            .env("VELNOR_WORKFLOW_AUTHORITY_SHA", assets::SOURCE)
            .env("GITHUB_SHA", assets::SOURCE)
            .env("GITHUB_WORKFLOW_SHA", assets::SOURCE)
            .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
            .env("GITHUB_OUTPUT", root.join("output"))
            .env("FIXTURE_RELEASES", root.join("releases.json"))
            .env("FIXTURE_ASSETS", root.join("assets"))
            .output()?)
    }
}

fn reconcile_script(root: &Path) -> Result<String, Box<dyn Error>> {
    let mut pins = render_pins()?;
    pins.gh_argv = vec![
        "bash".to_owned(),
        root.join("gh-transport.sh")
            .to_str()
            .ok_or("nonutf8 fixture path")?
            .to_owned(),
    ];
    let request = Schema2WorkflowRequest {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from([RoutingWorkflow::GeneratorRelease]),
        mbx_qualification: None,
        mise_pin_qualification: None,
        product_release: Some(pins),
    };
    let files = render_schema2_workflows(&request)?;
    let workflow = files
        .iter()
        .find(|file| file.path == ".github/workflows/product-release.yml")
        .ok_or("missing rendered product release workflow")?;
    let section = workflow
        .bytes
        .split("\n  prepare-generator:\n")
        .nth(1)
        .ok_or("missing generator prepare job")?;
    let job = section
        .split("\n  release-generator:\n")
        .next()
        .ok_or("missing generator job body")?;
    let reconcile = job
        .split("- name: Reconcile immutable family release\n")
        .nth(1)
        .ok_or("missing generator reconcile step")?;
    let run = reconcile
        .lines()
        .find_map(|line| line.strip_prefix("        run: "))
        .ok_or("missing generator reconcile script")?;
    Ok(serde_json::from_str(run)?)
}

fn assert_rejected(fixture: &Fixture, output: &Output) -> Result<(), Box<dyn Error>> {
    assert!(
        !output.status.success(),
        "passive fixtures must never complete authenticated reconciliation"
    );
    let output_path = fixture.root.path().join("output");
    let state = if output_path.exists() {
        fs::read_to_string(output_path)?
    } else {
        String::new()
    };
    assert!(!state.lines().any(|line| line == "action=complete"));
    assert_eq!(
        fs::read_dir(fixture.root.path().join("checkout"))?.count(),
        0,
        "reconciliation changed the empty checkout"
    );
    Ok(())
}

fn render_pins() -> Result<velnor_actions_workflow_renderer::ProductReleasePins, Box<dyn Error>> {
    use velnor_actions_mise::{MiseInstall, PinnedTool, ToolCatalog};
    use velnor_actions_workflow_renderer::verifier_builders::{
        jactionlint_verifier_argv, verify_rust_argv,
    };
    let catalog = ToolCatalog::pinned();
    let install = MiseInstall::new(vec![PinnedTool::Gh])?
        .argv(&catalog)
        .into_iter()
        .map(|value| {
            value
                .into_string()
                .map_err(|_| "nonutf8 native install argument")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut pins = super::pins();
    for argv in [
        &mut pins.install_gate_tools_argv,
        &mut pins.install_build_tools_argv,
        &mut pins.install_qualify_tools_argv,
        &mut pins.install_runner_build_tools_argv,
        &mut pins.install_gh_argv,
        &mut pins.build_argv,
        &mut pins.intel_build_argv,
        &mut pins.install_intel_target_argv,
        &mut pins.install_resource_probe_target_argv,
        &mut pins.runner_build_argv,
        &mut pins.resource_probe_build_argv,
        &mut pins.install_resource_probe_release_tools_argv,
        &mut pins.resource_probe_release_build_argv,
        &mut pins.resource_probe_release_test_argv,
        &mut pins.lint_argv,
    ] {
        install.clone_into(argv);
    }
    pins.verify_rust_argv = verify_rust_argv(&catalog);
    pins.verify_lint_argv = jactionlint_verifier_argv(&catalog);
    Ok(pins)
}
