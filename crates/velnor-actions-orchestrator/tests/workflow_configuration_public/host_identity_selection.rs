// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use super::scoped_workspace::{TestResult, commit, generate_lockfile, plan, put};
use super::workflow_configuration_fixture::make_repo;

const HOST: &str = "velnor-runner-host";
const RUNNER_CLI: &str = "velnor-runner-cli";
const ACTIONS_CLI: &str = "velnor-actions-cli";
const SDK: &str = "velnor-actions-mise";

#[test]
fn unrelated_sdk_changes_do_not_force_runner_host_or_its_cli() -> TestResult {
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\n[[stacks.rust.inputs]]\nglobs = [\"crates/velnor-actions-mise/**\"]\npackages = [\"velnor-actions-cli\"]\ncomplete = true\n";
    let repo = make_repo(config)?;
    let root = repo.path();
    put(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/velnor-actions-cli\", \"crates/velnor-actions-mise\"]\nexclude = [\"crates/velnor-runner\"]\nresolver = \"3\"\n",
    )?;
    for name in [ACTIONS_CLI, SDK] {
        let path = format!("crates/{name}");
        put(
            root,
            &format!("{path}/Cargo.toml"),
            &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )?;
        put(root, &format!("{path}/src/lib.rs"), "pub fn fixture() {}\n")?;
    }
    copy_runner_manifests(root)?;
    put(root, "unowned/fixture.txt", "base\n")?;
    generate_lockfile(root)?;
    let base = commit(root, "baseline")?;

    put(
        root,
        "crates/velnor-actions-mise/src/lib.rs",
        "pub fn changed() {}\n",
    )?;
    let sdk_head = commit(root, "sdk change")?;
    let sdk = plan(root, "pull_request", Some(&base), &sdk_head)?;
    let jobs = sdk.jobs()?;
    for name in [SDK, ACTIONS_CLI] {
        assert_eq!(
            jobs.get(&format!("rust-{name}")).map(String::as_str),
            Some("run"),
            "{name}"
        );
    }
    for name in [HOST, RUNNER_CLI] {
        assert_eq!(
            jobs.get(&format!("rust-{name}")).map(String::as_str),
            Some("not_affected"),
            "{name}: {:?}",
            sdk.warnings()
        );
    }

    put(
        root,
        "crates/velnor-runner/crates/velnor-runner-host/src/lib.rs",
        "pub fn changed() {}\n",
    )?;
    let host_head = commit(root, "host change")?;
    let host = plan(root, "pull_request", Some(&sdk_head), &host_head)?;
    let jobs = host.jobs()?;
    for name in [HOST, RUNNER_CLI] {
        assert_eq!(
            jobs.get(&format!("rust-{name}")).map(String::as_str),
            Some("run"),
            "{name}"
        );
    }

    put(root, "unowned/fixture.txt", "changed\n")?;
    let unowned_head = commit(root, "unowned change")?;
    let unowned = plan(root, "pull_request", Some(&host_head), &unowned_head)?;
    assert!(unowned.with_disposition("not_affected")?.is_empty());
    assert!(
        unowned
            .warnings()
            .iter()
            .any(|warning| warning == "unclassified_files:all_changed")
    );
    Ok(())
}

fn copy_runner_manifests(root: &Path) -> TestResult {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../velnor-runner");
    let target = "crates/velnor-runner";
    for name in ["Cargo.toml", "Cargo.lock"] {
        put(
            root,
            &format!("{target}/{name}"),
            &fs::read_to_string(source.join(name))?,
        )?;
    }
    for name in [
        "velnor-resource-probe",
        "velnor-runner-attestation",
        RUNNER_CLI,
        "velnor-runner-core",
        "velnor-runner-github",
        HOST,
    ] {
        let path = format!("crates/{name}");
        put(
            root,
            &format!("{target}/{path}/Cargo.toml"),
            &fs::read_to_string(source.join(&path).join("Cargo.toml"))?,
        )?;
        put(
            root,
            &format!("{target}/{path}/src/lib.rs"),
            "pub fn fixture() {}\n",
        )?;
        put(
            root,
            &format!("{target}/{path}/src/main.rs"),
            "fn main() {}\n",
        )?;
    }
    for path in [
        "crates/velnor-resource-probe/tests/job_hook_public.rs",
        "crates/velnor-resource-probe/src/bin/velnor_storage_init.rs",
        "crates/velnor-resource-probe/src/bin/velnor_job_start_hook.rs",
        "crates/velnor-runner-github/tests/poll_statistics.rs",
        "crates/velnor-runner-host/tests/resource_probe_release_public.rs",
        "crates/velnor-runner-host/tests/runner_image_manifest_public.rs",
        "crates/velnor-runner-host/tests/runner_image_qualification_public.rs",
        "crates/velnor-runner-host/tests/scaleset_installation_public.rs",
        "crates/velnor-runner-host/src/bin/velnor_resource_probe_release.rs",
    ] {
        put(root, &format!("{target}/{path}"), "fn main() {}\n")?;
    }
    let build = source.join("crates/velnor-runner-host/build.rs");
    if build.is_file() {
        put(
            root,
            &format!("{target}/crates/velnor-runner-host/build.rs"),
            &fs::read_to_string(build)?,
        )?;
    }
    Ok(())
}
