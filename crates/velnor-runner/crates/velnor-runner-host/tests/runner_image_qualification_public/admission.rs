// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn public_qualifier_records_all_three_candidate_identities() -> TestResult<()> {
    let git = GitFixture::new()?;
    let api = DockerApiFixture::new(ApiPlan::new(git.source_sha()))?;
    let output_dir = tempfile::tempdir()?;
    let result_path = output_dir.path().join("qualification.json");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-resource-probe-release"))
        .arg("qualify-runner-images")
        .arg(git.root())
        .arg(git.source_sha())
        .arg(digest_id('a'))
        .arg(digest_id('b'))
        .arg(digest_id('c'))
        .arg(api.socket())
        .arg(&result_path)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&fs::read(result_path)?)?;
    assert_eq!(
        result["schema"],
        "velnor.runner-image-runtime-qualification.v1"
    );
    assert_eq!(result["source_commit"], git.source_sha());
    let lock = fs::read(git.root().join(".velnor/runner.lock"))?;
    assert_eq!(
        result["runner_lock_sha256"],
        sha2::Sha256::digest(lock)
            .iter()
            .try_fold(String::new(), |mut text, byte| {
                write!(&mut text, "{byte:02x}")?;
                Ok::<_, std::fmt::Error>(text)
            })?
    );
    assert_eq!(result["images"]["runner"]["runtime_id"], digest_id('a'));
    assert_eq!(result["images"]["runner"]["identity_kind"], "config");
    assert_eq!(result["images"]["dind"]["runtime_id"], digest_id('b'));
    assert_eq!(result["images"]["dind"]["identity_kind"], "config");
    assert_eq!(
        result["images"]["resource_probe"]["runtime_id"],
        digest_id('c')
    );
    assert_eq!(
        result["images"]["resource_probe"]["identity_kind"],
        "config"
    );
    Ok(())
}

#[test]
fn public_qualifier_rejects_probe_identity_mismatches_without_result() -> TestResult<()> {
    for fault in [
        ImageFault::Id,
        ImageFault::Platform,
        ImageFault::Source,
        ImageFault::User,
        ImageFault::Entrypoint,
        ImageFault::WorkingDir,
        ImageFault::Reinspection,
    ] {
        let git = GitFixture::new()?;
        let mut plan = ApiPlan::new(git.source_sha());
        plan.image_fault = Some((ImageRole::ResourceProbe, fault));
        let api = DockerApiFixture::new(plan)?;
        let result_dir = tempfile::tempdir()?;
        let result = result_dir.path().join("qualification.json");
        let output =
            std::process::Command::new(env!("CARGO_BIN_EXE_velnor-resource-probe-release"))
                .arg("qualify-runner-images")
                .arg(git.root())
                .arg(git.source_sha())
                .arg(digest_id('a'))
                .arg(digest_id('b'))
                .arg(digest_id('c'))
                .arg(api.socket())
                .arg(&result)
                .output()?;
        assert!(
            !output.status.success(),
            "accepted probe mismatch {fault:?}"
        );
        assert!(
            !result.exists(),
            "wrote result for probe mismatch {fault:?}"
        );
        assert!(
            api.snapshot()?.fault_reached,
            "fault not reached: {fault:?}"
        );
    }
    Ok(())
}

#[test]
fn public_qualifier_rejects_malformed_probe_output_without_result() -> TestResult<()> {
    let git = GitFixture::new()?;
    let mut plan = ApiPlan::new(git.source_sha());
    plan.malformed_resource_probe = true;
    let api = DockerApiFixture::new(plan)?;
    let result_dir = tempfile::tempdir()?;
    let result = result_dir.path().join("qualification.json");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-resource-probe-release"))
        .arg("qualify-runner-images")
        .arg(git.root())
        .arg(git.source_sha())
        .arg(digest_id('a'))
        .arg(digest_id('b'))
        .arg(digest_id('c'))
        .arg(api.socket())
        .arg(&result)
        .output()?;
    assert!(!output.status.success());
    assert!(!result.exists());
    Ok(())
}

#[test]
fn public_qualifier_rejects_changed_lock_without_result() -> TestResult<()> {
    let git = GitFixture::new()?;
    fs::write(git.root().join(".velnor/runner.lock"), b"changed")?;
    let api = DockerApiFixture::new(ApiPlan::new(git.source_sha()))?;
    let result_dir = tempfile::tempdir()?;
    let result = result_dir.path().join("qualification.json");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-resource-probe-release"))
        .arg("qualify-runner-images")
        .arg(git.root())
        .arg(git.source_sha())
        .arg(digest_id('a'))
        .arg(digest_id('b'))
        .arg(digest_id('c'))
        .arg(api.socket())
        .arg(&result)
        .output()?;
    assert!(!output.status.success());
    assert!(!result.exists());
    assert_eq!(api.snapshot()?.api_requests, 0);
    Ok(())
}

#[tokio::test]
async fn public_recovery_retains_unproven_worker_images() -> TestResult<()> {
    use velnor_runner_host::{HostConfig, Journal, Outcome, connect_unix, launch_once};
    let worker = format!("w{}", "a".repeat(32));
    let expected_image = digest_id('d');
    for case in ["matching", "wrong-image", "missing-binding", "wrong-engine"] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("journal.db");
        let journal = Journal::open(&path).await?;
        let engine = if case == "wrong-engine" {
            "different-engine"
        } else {
            "outer-daemon-id"
        };
        journal.bind_engine(engine).await?;
        let id = journal.begin("launch", "recovery-test").await?;
        journal.bind_worker_volume(id, &worker).await?;
        journal.bind_worker(id, Some(&"f".repeat(64)), None).await?;
        journal.finish(id, Outcome::Done).await?;
        if case != "missing-binding" {
            let database = turso::Builder::new_local(path.to_str().ok_or("path")?)
                .build()
                .await?;
            let connection = database.connect()?;
            connection.execute(
                "INSERT INTO worker_image_bindings (intent_id, engine_id, docker_root, runner_image, dind_image) VALUES (?1, ?2, ?3, ?4, ?5)",
                (id, engine.to_owned(), "/var/lib/docker".to_owned(), expected_image.clone(), digest_id('b')),
            ).await?;
        }
        let mut plan = ApiPlan::new(&"a".repeat(40));
        plan.recovery_image = Some(if case == "wrong-image" {
            digest_id('e')
        } else {
            expected_image.clone()
        });
        let api = DockerApiFixture::new(plan)?;
        let docker = connect_unix(api.socket().to_str().ok_or("socket")?)?;
        let config = HostConfig::parse(&format!(
            "schema = 1\n[github]\nrepository = \"owner/repo\"\nscale_set_name = \"ubuntu-26.04-scale-set\"\ncredential_ref = \"keychain:velnor/token\"\n[host]\nmax_jobs = 1\n[host.resources]\nrunner_cpu_millicores = 1000\nrunner_memory_bytes = 1073741824\ndind_cpu_millicores = 1000\ndind_memory_bytes = 1073741824\n[docker]\ncontext = \"fixture\"\nplatform = \"linux/amd64\"\nendpoint = \"unix://{}\"\n",
            api.socket().display()
        ))?;
        let result = launch_once("", "", "", &docker, &journal, config.resource_budget()?).await;
        let snapshot = api.snapshot()?;
        let exists = snapshot
            .containers
            .iter()
            .any(|container| container.name == format!("{worker}-runner"));
        let row = journal
            .rows()
            .await?
            .into_iter()
            .find(|row| row.id == id)
            .ok_or("intent")?;
        assert_eq!(
            exists,
            case != "matching",
            "container retention for {case}, result {result:?}, requests {}",
            snapshot.api_requests
        );
        assert_eq!(
            row.cleanup_proven,
            case == "matching",
            "recovery state for {case}"
        );
    }
    Ok(())
}

#[test]
fn public_qualifier_records_classic_and_manifest_store_identities() -> TestResult<()> {
    for manifest_store in [false, true] {
        let git = GitFixture::new()?;
        let mut plan = ApiPlan::new(git.source_sha());
        plan.manifest_store = manifest_store.then_some(());
        let api = DockerApiFixture::new(plan)?;
        let result_dir = tempfile::tempdir()?;
        let result = result_dir.path().join("qualification.json");
        let output =
            std::process::Command::new(env!("CARGO_BIN_EXE_velnor-resource-probe-release"))
                .arg("qualify-runner-images")
                .arg(git.root())
                .arg(git.source_sha())
                .arg(digest_id('a'))
                .arg(digest_id('b'))
                .arg(digest_id('c'))
                .arg(api.socket())
                .arg(&result)
                .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&fs::read(result)?)?;
        let kind = if manifest_store {
            "platform_manifest"
        } else {
            "config"
        };
        for (role, digit) in [("runner", 'a'), ("dind", 'b'), ("resource_probe", 'c')] {
            assert_eq!(value["images"][role]["runtime_id"], digest_id(digit));
            assert_eq!(value["images"][role]["identity_kind"], kind);
        }
    }
    Ok(())
}

#[test]
fn public_qualifier_rejects_wrong_or_changed_manifest_store_descriptor() -> TestResult<()> {
    for role in [ImageRole::Runner, ImageRole::Dind, ImageRole::ResourceProbe] {
        for fault in [
            ImageFault::DescriptorDigest,
            ImageFault::DescriptorPlatform,
            ImageFault::DescriptorReinspection,
        ] {
            let git = GitFixture::new()?;
            let mut plan = ApiPlan::new(git.source_sha());
            plan.manifest_store = Some(());
            plan.image_fault = Some((role, fault));
            let api = DockerApiFixture::new(plan)?;
            let result_dir = tempfile::tempdir()?;
            let result = result_dir.path().join("qualification.json");
            let output =
                std::process::Command::new(env!("CARGO_BIN_EXE_velnor-resource-probe-release"))
                    .arg("qualify-runner-images")
                    .arg(git.root())
                    .arg(git.source_sha())
                    .arg(digest_id('a'))
                    .arg(digest_id('b'))
                    .arg(digest_id('c'))
                    .arg(api.socket())
                    .arg(&result)
                    .output()?;
            assert!(!output.status.success(), "accepted {role:?} {fault:?}");
            assert!(!result.exists());
            assert!(api.snapshot()?.fault_reached);
        }
    }
    Ok(())
}

#[tokio::test]
async fn public_operator_cleanup_uses_recorded_image_and_engine() -> TestResult<()> {
    if std::env::var_os("VELNOR_OPERATOR_TEST_CHILD").is_some() {
        return operator_cleanup_child().await;
    }
    for case in ["matching", "wrong-image", "missing-binding", "wrong-engine"] {
        operator_cleanup_case(case).await?;
    }
    Ok(())
}

async fn operator_cleanup_child() -> TestResult<()> {
    let socket = std::env::var("VELNOR_TEST_SOCKET")?;
    let journal = std::env::var("VELNOR_TEST_JOURNAL")?;
    let intent = std::env::var("VELNOR_TEST_INTENT")?.parse()?;
    let expected = std::env::var("VELNOR_TEST_EXPECTED")?;
    let docker = velnor_runner_host::connect_unix(&socket)?;
    let result =
        velnor_runner_host::remove_recorded(&docker, std::path::Path::new(&journal), intent).await;
    if expected == "matching" {
        assert_eq!(result?, velnor_runner_host::DeleteDecision::Delete);
    } else {
        assert!(result.is_err() || result? != velnor_runner_host::DeleteDecision::Delete);
    }
    Ok(())
}

async fn operator_cleanup_case(case: &str) -> TestResult<()> {
    use velnor_runner_host::{Journal, Outcome};
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("journal.db");
    let home = directory.path().join("home");
    fs::create_dir(&home)?;
    let journal = Journal::open(&path).await?;
    let worker = format!("w{}", "a".repeat(32));
    let expected_image = digest_id('d');
    let engine = if case == "wrong-engine" {
        "different-engine"
    } else {
        "outer-daemon-id"
    };
    journal.bind_engine(engine).await?;
    let id = journal.begin("launch", "operator-test").await?;
    journal.bind_worker_volume(id, &worker).await?;
    journal.bind_worker(id, Some(&"f".repeat(64)), None).await?;
    journal.finish(id, Outcome::Done).await?;
    if case != "missing-binding" {
        let database = turso::Builder::new_local(path.to_str().ok_or("path")?)
            .build()
            .await?;
        let connection = database.connect()?;
        connection.execute(
            "INSERT INTO worker_image_bindings (intent_id, engine_id, docker_root, runner_image, dind_image) VALUES (?1, ?2, ?3, ?4, ?5)",
            (id, engine.to_owned(), "/var/lib/docker".to_owned(), expected_image.clone(), digest_id('b')),
        ).await?;
    }
    let mut plan = ApiPlan::new(&"a".repeat(40));
    plan.recovery_image = Some(if case == "wrong-image" {
        digest_id('e')
    } else {
        expected_image
    });
    let api = DockerApiFixture::new(plan)?;
    let output = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "admission::public_operator_cleanup_uses_recorded_image_and_engine",
            "--nocapture",
        ])
        .env("HOME", &home)
        .env("VELNOR_OPERATOR_TEST_CHILD", "1")
        .env("VELNOR_TEST_SOCKET", api.socket())
        .env("VELNOR_TEST_JOURNAL", &path)
        .env("VELNOR_TEST_INTENT", id.to_string())
        .env("VELNOR_TEST_EXPECTED", case)
        .output()?;
    assert!(
        output.status.success(),
        "{case}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let exists = api
        .snapshot()?
        .containers
        .iter()
        .any(|container| container.name == format!("{worker}-runner"));
    let row = journal
        .rows()
        .await?
        .into_iter()
        .find(|row| row.id == id)
        .ok_or("intent")?;
    assert_eq!(exists, case != "matching", "container retention for {case}");
    assert_eq!(
        row.cleanup_proven,
        case == "matching",
        "recovery state for {case}"
    );
    Ok(())
}
