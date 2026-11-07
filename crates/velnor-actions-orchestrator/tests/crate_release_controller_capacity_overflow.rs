// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn workspace_with_unpublished_dependency_passes_capacity_before_coordination()
-> Result<(), Box<dyn std::error::Error>> {
    let (temp, intent, mut request, mut github, registry) = make_workspace_order_fixture()?;
    let index = PublishedIndex::start()?;
    let cargo_home = tempfile::tempdir()?;
    fs::write(
        cargo_home.path().join("config.toml"),
        format!(
            "[source.crates-io]\nreplace-with='fixture'\n[source.fixture]\nregistry='sparse+{}/'\n",
            index.url
        ),
    )?;
    let manifest = temp.path().join("a/Cargo.toml");
    let text = fs::read_to_string(&manifest)?;
    fs::write(
        &manifest,
        format!("{text}\n[dependencies]\npercent-encoding='=2.3.2'\n"),
    )?;
    let output = Command::new(env!("CARGO"))
        .args(["generate-lockfile"])
        .current_dir(temp.path())
        .env("CARGO_HOME", cargo_home.path())
        .env_remove("CARGO_NET_OFFLINE")
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    assert!(
        fs::read_to_string(temp.path().join("Cargo.lock"))?
            .contains("9b4f627cb1b25917193a259e49bdad08f671f8d9708acfd5fe0a8c1455d87220")
    );
    let sha = commit_paths(temp.path())?;
    request.source_sha = sha.clone();
    request.workflow_sha = sha.clone();
    github.0.sha = sha.clone();
    github.0.policy_sha = sha;
    super::FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = Some(index.url.clone()));
    let github = workspace_github(github);
    let registry = PublishedRegistry(registry);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, temp.path())?;
    let (identity, mut receipt) = fixture_journal_plan(temp.path(), &intent, &request, &admission)?;
    set_workspace_journal_packages(&mut receipt, &admission);
    let journal = JournalRegistryFake::default();
    seed_started_record(&journal, &identity, &receipt)?;
    journal.write_calls.set(0);
    let input = CrateReleaseObservation {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: test_git(),
        journal: &journal,
        identity: &identity,
        journal_receipt: &receipt,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: temp.path(),
        sequence: 1,
    };
    let calls = Cell::new(0);
    let result = coordinate_release(
        &input,
        |_| {
            calls.set(calls.get() + 1);
            Err(crate::internal::internal("capacity_fixture_stop"))
        },
        |_| panic!("unexpected upload"),
        || {},
    );
    assert!(result.is_err());
    assert_eq!(calls.get(), 1);
    assert_eq!(journal.write_calls.get(), 0);
    assert_eq!(
        index.downloads.load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    super::FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = None);
    Ok(())
}

pub(super) struct PublishedRegistry<R>(pub(super) R);

impl<R: crate::crate_release_registry::CrateRegistryRead>
    crate::crate_release_registry::CrateRegistryRead for PublishedRegistry<R>
{
    fn get(
        &self,
        path: &str,
    ) -> Result<crate::crate_release_registry::CrateRegistryResponse, crate::OrchestratorError>
    {
        if path == "/api/v1/crates/percent-encoding" {
            return Ok(crate::crate_release_registry::CrateRegistryResponse { status: 200, body: br#"{"crate":{"id":"percent-encoding","name":"percent-encoding","versions":[1]},"versions":[{"id":1,"crate":"percent-encoding","num":"2.3.2","yanked":false}]}"#.to_vec() });
        }
        crate::crate_release_registry::CrateRegistryRead::get(&self.0, path)
    }
}

#[test]
fn oversized_observed_record_is_rejected_before_coordination_mutates()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (mut intent, mut request, mut github, registry) = make_fixture(root)?;
    add_tracked_paths(root, "", 0, 29_000)?;
    let sha = commit_paths(root)?;
    request.source_sha = sha.clone();
    request.recovery_sha = Some(sha.clone());
    intent.recovery.as_mut().ok_or("recovery")?.source_sha = sha.clone();
    github.sha = sha;
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)
        .map_err(|error| format!("admission: {error:?}"))?;
    let (identity, receipt) = fixture_journal_plan(root, &intent, &request, &admission)
        .map_err(|error| format!("journal inventory: {error:?}"))?;
    let files = package_files(root, "demo").map_err(|error| format!("package list: {error:?}"))?;
    verify_archive_limits(root, "demo", "0.1.1", &files)
        .map_err(|error| format!("archive: {error:?}"))?;
    assert!(fixture_stage_record_bytes(&receipt, None)?.len() < LIMIT);
    assert!(
        intent_size(
            &receipt,
            &receipt.packages[0].name,
            &receipt.packages[0].version
        )? < LIMIT
    );
    assert!(observed_size(&receipt, 0, files, feature_map(root, "Cargo.toml")?)? > LIMIT);
    let journal = JournalRegistryFake::default();
    seed_started_record(&journal, &identity, &receipt)?;
    journal.write_calls.set(0);
    let input = CrateReleaseObservation {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: test_git(),
        journal: &journal,
        identity: &identity,
        journal_receipt: &receipt,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    assert_capacity_stops_before_callbacks(&input, &journal);
    assert_eq!(github.writes.get(), 0);
    Ok(())
}

#[test]
fn oversized_aggregate_completion_is_rejected_before_coordination_mutates()
-> Result<(), Box<dyn std::error::Error>> {
    let (temp, intent, mut request, mut github, registry) = make_workspace_order_fixture()?;
    let root = temp.path();
    add_tracked_paths(root, "a", 0, 14_500)?;
    add_tracked_paths(root, "b", 14_500, 14_500)?;
    let sha = commit_paths(root)?;
    request.source_sha = sha.clone();
    request.workflow_sha = sha.clone();
    github.0.sha = sha.clone();
    github.0.policy_sha = sha;
    let github = workspace_github(github);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, mut receipt) = fixture_journal_plan(root, &intent, &request, &admission)?;
    set_workspace_journal_packages(&mut receipt, &admission);
    let files = receipt
        .packages
        .iter()
        .map(|package| package_files(root, &package.name))
        .collect::<Result<Vec<_>, _>>()?;
    assert!(fixture_stage_record_bytes(&receipt, None)?.len() < LIMIT);
    let mut published = Vec::new();
    for (index, package) in receipt.packages.iter().enumerate() {
        assert!(intent_size(&receipt, &package.name, &package.version)? < LIMIT);
        assert!(
            observed_size(
                &receipt,
                index,
                files[index].clone(),
                feature_map(root, &package.manifest)?
            )? < LIMIT
        );
        let JournalOwnership::Existing { owners } = &package.owners else {
            return Err("owners".into());
        };
        published.push(JournalPublishedPackage {
            name: package.name.clone(),
            version: package.version.clone(),
            owners: owners.clone(),
            registry_checksum: "f".repeat(64),
            downloaded_checksum: "f".repeat(64),
            tag_target: identity.source_sha.clone(),
            release_id: 1,
            release_url: format!(
                "https://github.com/{}/releases/tag/{}",
                identity.repository, package.tag
            ),
            files: files[index].clone(),
            features: feature_map(root, &package.manifest)?,
            operations: vec![JournalOperation::ObservedPresent],
        });
    }
    let completion = JournalCompletion {
        packages: published,
        post_publication_file_hashes: receipt.expected_tools.clone(),
    };
    assert!(fixture_stage_record_bytes(&receipt, Some(completion))?.len() > LIMIT);
    let journal = JournalRegistryFake::default();
    seed_started_record(&journal, &identity, &receipt)?;
    journal.write_calls.set(0);
    let input = CrateReleaseObservation {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: test_git(),
        journal: &journal,
        identity: &identity,
        journal_receipt: &receipt,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    assert_capacity_stops_before_callbacks(&input, &journal);
    assert!(github.writes.borrow().is_empty());
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn signed_start_rejects_oversized_observation_without_journal_writes()
-> Result<(), Box<dyn std::error::Error>> {
    const TEST: &str = crate::libtest_case::name(concat!(
        module_path!(),
        "::signed_start_rejects_oversized_observation_without_journal_writes"
    ));
    if std::env::var_os("VELNOR_GH_FIXTURE_WORKER").is_none() {
        return crate::release::package_release_signer::image_transport_tests::dispatch(TEST);
    }
    crate::release::package_release_signer::image_transport_tests::validate(TEST)?;
    let source = tempfile::tempdir()?;
    let root = source.path();
    let (mut intent, mut request, mut github, registry) = make_fixture(root)?;
    add_tracked_paths(root, "", 0, 29_000)?;
    let sha = commit_paths(root)?;
    intent.generator_version = "0.1.5".into();
    intent.recovery.as_mut().ok_or("recovery")?.source_sha = sha.clone();
    request.source_sha = sha.clone();
    request.recovery_sha = Some(sha.clone());
    github.sha = sha;
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let journal = JournalRegistryFake::default();
    let attestation = tempfile::tempdir()?;
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/attestation_fixtures");
    let binary = "velnor-actions-0.1.5-x86_64-unknown-linux-gnu";
    for name in [
        "release-manifest.json",
        "release-manifest.json.intoto.jsonl",
        "velnor-actions-0.1.5-x86_64-unknown-linux-gnu.intoto.jsonl",
    ] {
        fs::copy(fixtures.join(name), attestation.path().join(name))?;
    }
    let mut decoder = flate2::read::GzDecoder::new(fs::File::open(fixtures.join("binary.gz"))?);
    std::io::copy(
        &mut decoder,
        &mut fs::File::create(attestation.path().join(binary))?,
    )?;
    let deadline = velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(180))?;
    let (_, _, generator) = crate::release::package_release_signer::verified_generator_fixture(
        attestation.path(),
        deadline,
    )?;
    let input = crate::release::package_release_signer::AdmittedStart {
        journal: &journal,
        github: &github,
        registry: &registry,
        intent: &intent,
        request: &request,
        admission: &admission,
        source: root,
        git: test_git(),
        source_execution: &crate::UnprivilegedSourceExecution,
        workflow_root: root,
        generator: &generator,
    };
    assert!(crate::release::package_release_signer::record_admitted_start(&input).is_err());
    assert_eq!(journal.write_calls.get(), 0);
    assert!(journal.refs.borrow().is_empty());
    assert_eq!(github.writes.get(), 0);
    crate::release::package_release_signer::image_transport_tests::record_success(TEST)?;
    Ok(())
}

pub(super) struct PublishedIndex {
    pub(super) url: String,
    done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(super) downloads: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl PublishedIndex {
    pub(super) fn start() -> Result<Self, Box<dyn std::error::Error>> {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        };
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!("http://{}", listener.local_addr()?);
        let done = Arc::new(AtomicBool::new(false));
        let downloads = Arc::new(AtomicUsize::new(0));
        let stop = done.clone();
        let counts = downloads.clone();
        let origin = url.clone();
        let thread = std::thread::spawn(move || {
            use std::io::{Read, Write};
            while !stop.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                    continue;
                };
                if stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .is_err()
                {
                    continue;
                }
                let mut input = [0u8; 4096];
                let Ok(n) = stream.read(&mut input) else {
                    continue;
                };
                let request = String::from_utf8_lossy(&input[..n]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("");
                let body = match path {
                    "/config.json" => format!("{{\"dl\":\"{origin}/download/{{crate}}/{{version}}\",\"api\":null}}"),
                    "/pe/rc/percent-encoding" => "{\"name\":\"percent-encoding\",\"vers\":\"2.3.2\",\"deps\":[],\"cksum\":\"9b4f627cb1b25917193a259e49bdad08f671f8d9708acfd5fe0a8c1455d87220\",\"features\":{\"alloc\":[],\"default\":[\"std\"],\"std\":[\"alloc\"]},\"yanked\":false,\"rust_version\":\"1.51\",\"pubtime\":\"2025-08-21T08:46:50Z\"}\n".to_owned(),
                    _ => { counts.fetch_add(1, Ordering::Relaxed); String::new() },
                };
                let status = if body.is_empty() {
                    "404 Not Found"
                } else {
                    "200 OK"
                };
                if let Err(error) = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                ) {
                    eprintln!("published index response failed: {error}");
                }
            }
        });
        Ok(Self {
            url,
            done,
            downloads,
            thread: Some(thread),
        })
    }
}

impl Drop for PublishedIndex {
    fn drop(&mut self) {
        self.done.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            eprintln!("published index thread failed");
        }
    }
}
