// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(unix)]
#[path = "crate_release_runtime_tests/continuation.rs"]
mod continuation;
#[cfg(unix)]
#[path = "crate_release_runtime_tests/crate_release_runtime_fixture_responses.rs"]
mod responses;
#[cfg(unix)]
#[path = "crate_release_runtime_tests/crate_release_runtime_fixture_transport.rs"]
mod transport;
#[cfg(unix)]
pub(super) use responses::test_github_admission_response;
#[cfg(unix)]
pub(super) use transport::read_test_request;
#[cfg(unix)]
use transport::{run_test_transport_child, serve_test_transport};

#[cfg(unix)]
#[test]
fn test_only_dispatch_and_loopback_transport() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    if std::env::var_os("VELNOR_TEST_CONTROLLER_SIGNED_CHILD").is_some() {
        return crate::crate_release_controller::tests::signed_history::run_child();
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    if std::env::var_os("VELNOR_TEST_DIRECT_HISTORY_CHILD").is_some() {
        return continuation::verify_direct_history_child();
    }
    if std::env::var_os("VELNOR_TEST_TRANSPORT_CHILD").is_some() {
        return run_test_transport_child();
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    if std::env::var_os("VELNOR_TEST_SAME_BASE_SIGNED").is_none() {
        return crate::release::package_release_signer::image_transport_tests::dispatch(
            "release::package_release_signer::attestation::image_transport_tests::same_base_history_rejects_forged_tool",
        );
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let origin = format!("http://{}", listener.local_addr()?);
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_done = std::sync::Arc::clone(&done);
    let server = std::thread::spawn(move || serve_test_transport(&listener, &server_done));
    let (spec, rust) = crate::release::release_steps::tests::fixture()?;
    let workflow = velnor_actions_workflow_renderer::release_tree::render_release_workflow(
        &spec,
        &velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext {
            generator_version: env!("CARGO_PKG_VERSION").to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
        },
        &rust,
    )?;
    let workflow: serde_json::Value = serde_saphyr::from_str(&workflow)?;
    let steps = workflow["jobs"]["release-publish"]["steps"]
        .as_array()
        .ok_or("publisher steps")?;
    let names = steps
        .iter()
        .filter_map(|step| step["name"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(names.first(), Some(&"Acquire Velnor"));
    assert_eq!(names.last(), Some(&"Release journal-coordinate"));
    assert!(names.contains(&"Release verify-admission"));
    let output = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            crate::libtest_case::name(concat!(
                module_path!(),
                "::test_only_dispatch_and_loopback_transport"
            )),
        ])
        .env("VELNOR_TEST_TRANSPORT_CHILD", "1")
        .env("VELNOR_TEST_GITHUB_ORIGIN", &origin)
        .env("VELNOR_TEST_REGISTRY_ORIGIN", &origin)
        .env_remove("GITHUB_WORKSPACE")
        .output()?;
    done.store(true, std::sync::atomic::Ordering::Release);
    server
        .join()
        .map_err(|_| "fixture server panicked")?
        .map_err(|error| format!("fixture server: {error}"))?;
    assert!(
        output.status.success(),
        "fixture child failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    crate::libtest_case::assert_one_passed(&output);
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn direct_history_accepts_reviewed_ubuntu26_tool_update() -> Result<(), Box<dyn std::error::Error>>
{
    let output = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            crate::libtest_case::name(concat!(
                module_path!(),
                "::test_only_dispatch_and_loopback_transport"
            )),
        ])
        .env("VELNOR_TEST_UPDATED_HISTORY", "1")
        .output()?;
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    crate::libtest_case::assert_one_passed(&output);
    Ok(())
}

#[test]
fn workflow_lock_requires_exact_literal_group_and_no_cancellation() {
    let group = "release-crates-io-owner/repo";
    let valid = format!(
        "name: release\nconcurrency:\n  group: {group}\n  cancel-in-progress: false\n  queue: max\njobs: {{}}\n"
    );
    assert!(super::validate_workflow_concurrency(valid.as_bytes(), group).is_ok());
    for bad in [
        valid.replace("  queue: max\n", ""),
        valid.replace("queue: max", "queue: single"),
        valid.replace(group, "release-crates-io-owner/repo-other"),
        valid.replace("cancel-in-progress: false", "cancel-in-progress: true"),
        valid.replace("concurrency:", "other:"),
        format!("{valid}concurrency:\n  group: {group}\n  cancel-in-progress: false\n"),
        valid.replace(&format!("  group: {group}"), "  group: *other"),
        valid.replace("concurrency:", "concurrency: &value"),
        valid.replace(
            "  cancel-in-progress: false",
            "  cancel-in-progress: false\n  group: other",
        ),
        valid.replace(
            "  cancel-in-progress: false",
            "  cancel-in-progress: false\n  cancel-in-progress: true",
        ),
        valid.replace(
            "  cancel-in-progress: false",
            "  cancel-in-progress: false\n  <<: {}",
        ),
        valid.replace("cancel-in-progress: false", "cancel-in-progress: 'false'"),
        format!("base: &base {{extra: value}}\n<<: *base\n{valid}"),
        valid
            .replace("concurrency:", "base: &base {extra: value}\nconcurrency:")
            .replace(
                "  cancel-in-progress: false",
                "  cancel-in-progress: false\n  <<: *base",
            ),
    ] {
        assert!(super::validate_workflow_concurrency(bad.as_bytes(), group).is_err());
    }
}

fn reject_unreviewed_bootstrap(
    root: &std::path::Path,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    writes: &std::cell::Cell<usize>,
) {
    use super::{load_bootstrap, start_journal_with};
    assert!(
        start_journal_with(
            root,
            intent,
            b"different reviewed manifest",
            |_| Ok(()),
            |()| {
                writes.set(writes.get() + 1);
                Ok(())
            }
        )
        .is_err()
    );
    assert_eq!(writes.get(), 1);
    let mut wrong = intent.clone();
    wrong.generator_source_sha = "a".repeat(40);
    assert!(load_bootstrap(root, &wrong).is_err());
    wrong = intent.clone();
    wrong.generator_manifest_sha256 = "b".repeat(64);
    assert!(load_bootstrap(root, &wrong).is_err());
}

#[test]
fn journal_start_dispatcher_rejects_tampered_inputs_before_recording()
-> Result<(), Box<dyn std::error::Error>> {
    use super::{load_bootstrap, start_journal_with};
    use std::{cell::Cell, fs};
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let mut intent: velnor_actions_contract::config::CrateReleaseIntent = serde_json::from_value(
        serde_json::json!({"schema":1,"repository":"example/demo","default_branch":"main","environment":"release","required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":139_017,"rust_version":"1.97.1","generator_version":"0.1.5","generator_source_sha":"f1041f322c54cd7edfdef06afd307799eeaa801a","generator_manifest_sha256":"e38d047150f9e0347ec2160fd65bb7f026842e6f416e456990cf3b7ccd284c52","release_plz_version":"0.4.3","runner":"ubuntu-26.04","authentication":"trusted-publishing","manifest_path":"Cargo.toml","pr_write_paths":["CHANGELOG.md","Cargo.lock","Cargo.toml"],"registry":"crates-io","packages":["demo"],"publish_features":{"demo":[]},"tag_template":"{{ package }}-v{{ version }}","recovery":null}),
    )?;
    let binary = b"fixture-binary";
    let mut manifest: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../tests/attestation_fixtures/release-manifest.json"
    ))?;
    manifest["targets"][0]["sha256"] =
        serde_json::json!(crate::planner::generator::sha256_hex(binary));
    let manifest = serde_json::to_vec(&manifest)?;
    intent.generator_manifest_sha256 = crate::planner::generator::sha256_hex(&manifest);
    let name = "velnor-actions-0.1.5-x86_64-unknown-linux-gnu";
    fs::write(root.join("release-manifest.json"), &manifest)?;
    fs::write(
        root.join("release-manifest.json.intoto.jsonl"),
        b"manifest-signature",
    )?;
    fs::write(root.join(name), binary)?;
    fs::write(
        root.join(format!("{name}.intoto.jsonl")),
        b"binary-signature",
    )?;
    let writes = Cell::new(0);
    let dispatch = || {
        start_journal_with(
            root,
            &intent,
            &manifest,
            |files| {
                if files.bytes != manifest
                    || fs::read(&files.manifest_bundle).ok().as_deref()
                        != Some(b"manifest-signature")
                    || fs::read(&files.binary_bundle).ok().as_deref() != Some(b"binary-signature")
                {
                    return Err(crate::internal::internal("fixture_attestation_failed"));
                }
                Ok(())
            },
            |()| {
                writes.set(writes.get() + 1);
                Ok(())
            },
        )
    };
    dispatch()?;
    assert_eq!(writes.get(), 1);
    reject_unreviewed_bootstrap(root, &intent, &writes);
    fs::write(root.join(name), b"tampered")?;
    assert!(dispatch().is_err());
    fs::write(root.join(name), binary)?;
    fs::write(
        root.join("release-manifest.json"),
        [manifest.as_slice(), b" "].concat(),
    )?;
    assert!(dispatch().is_err());
    fs::write(root.join("release-manifest.json"), &manifest)?;
    fs::write(root.join("release-manifest.json.intoto.jsonl"), b"tampered")?;
    assert!(dispatch().is_err());
    fs::write(
        root.join("release-manifest.json.intoto.jsonl"),
        b"manifest-signature",
    )?;
    fs::write(root.join(format!("{name}.intoto.jsonl")), b"tampered")?;
    assert!(dispatch().is_err());
    assert_eq!(writes.get(), 1);
    assert!(load_bootstrap(root, &intent).is_ok());
    assert!(super::run_crate_release("journal-start").is_err());
    Ok(())
}
#[test]
fn release_intent_comes_from_the_requested_workflow_commit()
-> Result<(), Box<dyn std::error::Error>> {
    use super::{read_intent_at_workflow, read_manifest_at_workflow};
    use std::{fs, path::Path, process::Command};
    fn test_git() -> crate::crate_release_git::ReleaseGit {
        crate::crate_release_git::ReleaseGit::host_fixture()
    }
    fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
        let output = Command::new("/usr/bin/git")
            .current_dir(root)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()?;
        if !output.status.success() {
            return Err("git fixture failed".into());
        }
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    }
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    git(root, &["init", "-q"])?;
    git(root, &["config", "user.name", "Fixture"])?;
    git(root, &["config", "user.email", "fixture@example.invalid"])?;
    git(root, &["config", "commit.gpgsign", "false"])?;
    fs::create_dir(root.join(".github"))?;
    fs::create_dir(root.join(".velnor"))?;
    fs::create_dir(root.join(".github/workflows"))?;
    let old_workflow = root.join(".github/workflows/not-release.yml");
    let workflow = root.join(".github/workflows/release.yml");
    fs::write(&old_workflow, b"legacy workflow")?;
    let path = root.join(".github/velnor-release-intent.toml");
    let manifest_path = root.join(".velnor/release-manifest.json");
    let make = |version: &str| {
        format!(
            "schema=1\nrepository='example/demo'\ndefault_branch='main'\nenvironment='release'\nrequired_workflow_path='.github/workflows/ci.yml'\ntrusted_reviewer_id=139017\nrust_version='1.97.1'\ngenerator_version='{version}'\ngenerator_source_sha='f1041f322c54cd7edfdef06afd307799eeaa801a'\ngenerator_manifest_sha256='e38d047150f9e0347ec2160fd65bb7f026842e6f416e456990cf3b7ccd284c52'\nrelease_plz_version='0.4.3'\nrunner='ubuntu-26.04'\nauthentication='trusted-publishing'\nmanifest_path='Cargo.toml'\npr_write_paths=['CHANGELOG.md','Cargo.lock','Cargo.toml']\nregistry='crates-io'\npackages=['demo']\npublish_features={{demo=[]}}\ntag_template='{{{{ package }}}}-v{{{{ version }}}}'\n"
        )
    };
    fs::write(&path, make("0.1.5"))?;
    fs::write(&manifest_path, b"first manifest")?;
    git(root, &["add", "."])?;
    git(root, &["commit", "-qm", "first"])?;
    let first = git(root, &["rev-parse", "HEAD"])?;
    fs::remove_file(&old_workflow)?;
    fs::write(&workflow, b"canonical workflow")?;
    fs::write(&path, make("0.1.6"))?;
    fs::write(&manifest_path, b"second manifest")?;
    git(root, &["add", "."])?;
    git(root, &["commit", "-qm", "second"])?;
    let second = git(root, &["rev-parse", "HEAD"])?;
    fs::write(&workflow, b"dirty workflow")?;
    assert!(super::read_workflow_at(root, &first, &test_git()).is_err());
    assert_eq!(
        super::read_workflow_at(root, &second, &test_git())?,
        b"canonical workflow"
    );
    fs::write(&path, make("0.1.7"))?;
    fs::write(&manifest_path, b"dirty manifest")?;
    assert_eq!(
        read_manifest_at_workflow(root, &first, &test_git())?,
        b"first manifest"
    );
    assert_eq!(
        read_manifest_at_workflow(root, &second, &test_git())?,
        b"second manifest"
    );
    assert!(read_manifest_at_workflow(root, "bad", &test_git()).is_err());
    assert!(read_manifest_at_workflow(root, &"0".repeat(40), &test_git()).is_err());
    assert_eq!(
        read_intent_at_workflow(root, &first, &test_git())?.generator_version,
        "0.1.5"
    );
    assert_eq!(
        read_intent_at_workflow(root, &second, &test_git())?.generator_version,
        "0.1.6"
    );
    assert!(read_intent_at_workflow(root, "bad", &test_git()).is_err());
    assert!(read_intent_at_workflow(root, &"0".repeat(40), &test_git()).is_err());
    Ok(())
}
#[path = "crate_release_runtime_tests/polling.rs"]
mod polling;

#[test]
fn native_fixture_evidence_binds_observed_inputs() -> Result<(), Box<dyn std::error::Error>> {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native_candidate/test_evidence.py");
    let status = std::process::Command::new("python3")
        .arg("-B")
        .arg(script)
        .status()?;
    assert!(status.success());
    Ok(())
}
