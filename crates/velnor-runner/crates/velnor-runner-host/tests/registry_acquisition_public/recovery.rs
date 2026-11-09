// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[path = "recovery/mismatch.rs"]
mod mismatch;

#[tokio::test]
async fn pruned_completed_image_is_pulled_again_with_new_attempt() -> TestResult {
    let temp = tempfile::tempdir()?;
    let socket = temp.path().join("docker.sock");
    let path = temp.path().join("journal.db");
    let journal = Journal::open(&path).await?;
    journal.bind_engine(ENGINE_ID).await?;
    let (image, manifest, config, source) = fixture()?;
    let gate = Arc::new((Mutex::new(true), Condvar::new()));
    let mut plan = pull_plan(true, &image, &config, &gate)?;
    let mut second = pull_plan(true, &image, &config, &gate)?;
    plan.inspect.append(&mut second.inspect);
    plan.pull.append(&mut second.pull);
    let peer = Peer::bind(&socket, plan)?;
    let endpoint = socket.to_str().ok_or("socket path")?;
    for _ in 0..2 {
        let projection = RegistryProjection::from_oci(
            &image,
            ImageKind::ResourceProbe,
            &source,
            &manifest,
            &config,
        )?;
        let spec = RegistryImageSpecification::new(
            &selection()?,
            &image,
            ImageKind::ResourceProbe,
            &source,
            &"e".repeat(64),
            projection,
        )?;
        let result = acquire_registry_image(&journal, endpoint, spec).await?;
        assert_eq!(result.runtime_id(), image.config_digest);
    }
    let conn = turso::Builder::new_local(path.to_str().ok_or("journal path")?)
        .build()
        .await?
        .connect()?;
    let mut rows = conn
        .query(
            "SELECT attempt,state FROM image_import_registry ORDER BY attempt",
            (),
        )
        .await?;
    for expected in 0..2 {
        let row = rows.next().await?.ok_or("missing attempt")?;
        assert_eq!(row.get::<i64>(0)?, expected);
        assert_eq!(row.get::<String>(1)?, "completed");
    }
    assert!(rows.next().await?.is_none());
    assert_eq!(
        peer.requests()?
            .iter()
            .filter(|request| request.method == "POST")
            .count(),
        2
    );
    Ok(())
}

fn specification() -> TestResult<RegistryImageSpecification> {
    let (image, manifest, config, source) = fixture()?;
    let projection = RegistryProjection::from_oci(
        &image,
        ImageKind::ResourceProbe,
        &source,
        &manifest,
        &config,
    )?;
    Ok(RegistryImageSpecification::new(
        &selection()?,
        &image,
        ImageKind::ResourceProbe,
        &source,
        &"e".repeat(64),
        projection,
    )?)
}

#[tokio::test]
async fn interrupted_pull_child() -> TestResult {
    let Some(directory) = std::env::var_os("VELNOR_REGISTRY_CRASH_FIXTURE") else {
        return Ok(());
    };
    let directory = std::path::PathBuf::from(directory);
    let journal = Journal::open(&directory.join("journal.db")).await?;
    let socket = directory.join("docker.sock");
    acquire_registry_image(
        &journal,
        socket.to_str().ok_or("socket path")?,
        specification()?,
    )
    .await?;
    Err("child unexpectedly completed".into())
}

async fn leave_requested(
    temp: &tempfile::TempDir,
    image: &ImageEntry,
    config: &[u8],
) -> TestResult {
    let path = temp.path().join("journal.db");
    let journal = Journal::open(&path).await?;
    journal.bind_engine(ENGINE_ID).await?;
    drop(journal);
    let socket = temp.path().join("docker.sock");
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let peer = Peer::bind(&socket, pull_plan(true, image, config, &gate)?)?;
    let mut child = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", "recovery::interrupted_pull_child", "--nocapture"])
        .env("VELNOR_REGISTRY_CRASH_FIXTURE", temp.path())
        .spawn()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !peer
        .requests()?
        .iter()
        .any(|request| request.method == "POST")
    {
        if std::time::Instant::now() >= deadline || child.try_wait()?.is_some() {
            *gate.0.lock().map_err(|_| "gate")? = true;
            gate.1.notify_all();
            drop(child.kill());
            drop(child.wait());
            return Err("child did not start pull".into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    child.kill()?;
    child.wait()?;
    *gate.0.lock().map_err(|_| "gate")? = true;
    gate.1.notify_all();
    drop(peer);
    std::fs::remove_file(socket)?;
    Ok(())
}

#[tokio::test]
async fn interrupted_claim_reconciles_verified_local_image_without_pull() -> TestResult {
    let temp = tempfile::tempdir()?;
    let (image, _, config, _) = fixture()?;
    leave_requested(&temp, &image, &config).await?;
    let socket = temp.path().join("docker.sock");
    let gate = Arc::new((Mutex::new(true), Condvar::new()));
    let mut plan = pull_plan(true, &image, &config, &gate)?;
    plan.inspect.pop_front();
    plan.pull.clear();
    let peer = Peer::bind(&socket, plan)?;
    let path = temp.path().join("journal.db");
    let journal = Journal::open(&path).await?;
    let image_result = acquire_registry_image(
        &journal,
        socket.to_str().ok_or("socket path")?,
        specification()?,
    )
    .await?;
    assert_eq!(image_result.runtime_id(), image.config_digest);
    assert!(
        !peer
            .requests()?
            .iter()
            .any(|request| request.method == "POST")
    );
    let conn = turso::Builder::new_local(path.to_str().ok_or("journal path")?)
        .build()
        .await?
        .connect()?;
    let mut rows = conn
        .query("SELECT attempt,state FROM image_import_registry", ())
        .await?;
    let row = rows.next().await?.ok_or("missing attempt")?;
    assert_eq!(row.get::<i64>(0)?, 0);
    assert_eq!(row.get::<String>(1)?, "completed");
    assert!(rows.next().await?.is_none());
    Ok(())
}

#[tokio::test]
async fn interrupted_requested_pull_is_retried_after_process_crash() -> TestResult {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("journal.db");
    let socket = temp.path().join("docker.sock");
    let (image, _, config, _) = fixture()?;
    leave_requested(&temp, &image, &config).await?;
    let mut wrong_plan = Plan::default();
    wrong_plan.info.push_back(info_response("other-engine"));
    let wrong_peer = Peer::bind(&socket, wrong_plan)?;
    let journal = Journal::open(&path).await?;
    assert!(
        acquire_registry_image(
            &journal,
            socket.to_str().ok_or("socket path")?,
            specification()?
        )
        .await
        .is_err()
    );
    assert!(
        !wrong_peer
            .requests()?
            .iter()
            .any(|request| request.method == "POST")
    );
    drop(wrong_peer);
    std::fs::remove_file(&socket)?;
    let retry_gate = Arc::new((Mutex::new(true), Condvar::new()));
    let retry_peer = Peer::bind(&socket, pull_plan(true, &image, &config, &retry_gate)?)?;
    let result = acquire_registry_image(
        &journal,
        socket.to_str().ok_or("socket path")?,
        specification()?,
    )
    .await?;
    assert_eq!(result.runtime_id(), image.config_digest);
    let conn = turso::Builder::new_local(path.to_str().ok_or("journal path")?)
        .build()
        .await?
        .connect()?;
    let mut rows = conn
        .query("SELECT attempt,state FROM image_import_registry", ())
        .await?;
    let row = rows.next().await?.ok_or("missing attempt")?;
    assert_eq!(row.get::<i64>(0)?, 0);
    assert_eq!(row.get::<String>(1)?, "completed");
    assert!(rows.next().await?.is_none());
    assert_eq!(
        retry_peer
            .requests()?
            .iter()
            .filter(|request| request.method == "POST")
            .count(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn cancelled_caller_does_not_release_an_active_pull_claim() -> TestResult {
    let temp = tempfile::tempdir()?;
    let socket = temp.path().join("docker.sock");
    let path = temp.path().join("journal.db");
    let journal = Journal::open(&path).await?;
    journal.bind_engine(ENGINE_ID).await?;
    let (image, _, config, _) = fixture()?;
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let peer = Peer::bind(&socket, pull_plan(true, &image, &config, &gate)?)?;
    let endpoint = socket.to_str().ok_or("socket path")?.to_owned();
    let owned_journal = journal.clone();
    let owned_endpoint = endpoint.clone();
    let spec = specification()?;
    let owner =
        tokio::spawn(
            async move { acquire_registry_image(&owned_journal, &owned_endpoint, spec).await },
        );
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !peer
        .requests()?
        .iter()
        .any(|request| request.method == "POST")
    {
        if std::time::Instant::now() >= deadline || owner.is_finished() {
            *gate.0.lock().map_err(|_| "gate")? = true;
            gate.1.notify_all();
            return Err("owner did not start pull".into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    owner.abort();
    let contender = acquire_registry_image(&journal, &endpoint, specification()?).await;
    *gate.0.lock().map_err(|_| "gate")? = true;
    gate.1.notify_all();
    assert!(contender.is_err());
    let conn = turso::Builder::new_local(path.to_str().ok_or("journal path")?)
        .build()
        .await?
        .connect()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let mut rows = conn
            .query("SELECT state FROM image_import_registry", ())
            .await?;
        let row = rows.next().await?.ok_or("missing claim")?;
        let state: String = row.get(0)?;
        if state == "completed" {
            break;
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!("claim did not settle: {state}").into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        peer.requests()?
            .iter()
            .filter(|request| request.method == "POST")
            .count(),
        1
    );
    Ok(())
}
