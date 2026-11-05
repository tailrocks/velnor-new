// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Digest, Github, JournalRegistryFake, Registry, Sha256, fixture_journal_plan, json,
    make_fixture, prepare_crate_release, test_git, test_hex,
};

pub(super) struct SparseFixture {
    pub(super) url: String,
    pub(super) stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(super) thread: Option<std::thread::JoinHandle<()>>,
    pub(super) index_visible: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(super) index_hits: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub(super) download_hits: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
impl SparseFixture {
    pub(super) fn start(name: &str, archive: Vec<u8>) -> Result<Self, Box<dyn std::error::Error>> {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!("http://{}", listener.local_addr()?);
        let url_server = url.clone();
        let name = name.to_owned();
        let checksum = test_hex(&Sha256::digest(&archive));
        let stop = Arc::new(AtomicBool::new(false));
        let stop_server = stop.clone();
        let index_visible = Arc::new(AtomicBool::new(false));
        let index_server = index_visible.clone();
        let index_hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let index_hits_server = index_hits.clone();
        let download_hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let download_hits_server = download_hits.clone();
        let thread = std::thread::spawn(move || {
            let mut workers = Vec::new();
            while !stop_server.load(Ordering::Relaxed) {
                let Ok((stream, _)) = listener.accept() else {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    continue;
                };
                let request = SparseRequest {
                    name: name.clone(),
                    url: url_server.clone(),
                    checksum: checksum.clone(),
                    archive: archive.clone(),
                    visible: index_server.clone(),
                    index_hits: index_hits_server.clone(),
                    download_hits: download_hits_server.clone(),
                    stop: stop_server.clone(),
                };
                workers.push(std::thread::spawn(move || {
                    serve_sparse_request(stream, request);
                }));
            }
            for worker in workers {
                worker.join().expect("fixture connection");
            }
        });
        Ok(Self {
            url,
            stop,
            thread: Some(thread),
            index_visible,
            index_hits,
            download_hits,
        })
    }
}
pub(super) struct SparseRequest {
    pub(super) name: String,
    pub(super) url: String,
    pub(super) checksum: String,
    pub(super) archive: Vec<u8>,
    pub(super) visible: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(super) index_hits: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub(super) download_hits: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub(super) stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

pub(super) fn serve_sparse_request(mut stream: std::net::TcpStream, fixture: SparseRequest) {
    use std::io::{Read, Write};
    use std::sync::atomic::Ordering;
    stream
        .set_read_timeout(Some(std::time::Duration::from_millis(100)))
        .expect("set read timeout");
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    while !bytes.windows(4).any(|window| window == b"\r\n\r\n") && bytes.len() < 16 * 1024 {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => bytes.extend_from_slice(&chunk[..count]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) && !fixture.stop.load(Ordering::Relaxed) => {}
            Err(_) => break,
        }
    }
    let request = String::from_utf8_lossy(&bytes);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("");
    if path.is_empty() {
        return;
    }
    let name = &fixture.name;
    let (status, body) = if path == "/config.json" {
        (
            "200 OK",
            json!({"dl":format!("{}/download/{{crate}}/{{version}}", fixture.url),"api":null})
                .to_string()
                .into_bytes(),
        )
    } else if path == format!("/{}/{}/{}", &name[..2], &name[2..4], name)
        || path == format!("/{}/{}/{}", &name[..2], &name[2..4], name.replace('_', "-"))
    {
        fixture.index_hits.fetch_add(1, Ordering::SeqCst);
        if fixture.visible.load(Ordering::SeqCst) {
            ("200 OK", format!("{}\n",json!({"name":name,"vers":"0.1.1","deps":[],"cksum":fixture.checksum,"features":{"pty":[],"base":[],"default":["base"]},"yanked":false})).into_bytes())
        } else {
            ("404 Not Found", Vec::new())
        }
    } else if path == format!("/download/{name}/0.1.1") {
        fixture.download_hits.fetch_add(1, Ordering::SeqCst);
        ("200 OK", fixture.archive)
    } else {
        ("404 Not Found", Vec::new())
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .expect("write response header");
    stream.write_all(&body).expect("write response body");
}

impl Drop for SparseFixture {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().expect("join fixture server");
        }
    }
}

#[test]
pub(super) fn admitted_start_derives_receipt_and_rechecks_authority()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{JournalAuthentication, JournalStage, inspect_journal};
    use crate::package_release_signer::{
        AdmittedStart, fixture_attested_generator, record_admitted_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let journal = JournalRegistryFake::default();
    let generator = fixture_attested_generator();
    let input = AdmittedStart {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: test_git(),
        journal: &journal,
        github: &github,
        registry: &registry,
        intent: &intent,
        request: &request,
        admission: &admission,
        source: root,
        workflow_root: root,
        generator: &generator,
    };
    github.approved.set(false);
    assert!(record_admitted_start(&input).is_err());
    assert!(journal.refs.borrow().is_empty());
    github.approved.set(true);
    github.owner_match.set(false);
    assert!(record_admitted_start(&input).is_err());
    assert!(journal.refs.borrow().is_empty());
    github.owner_match.set(true);
    let mut wrong = intent.clone();
    wrong.authentication = velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    let bad = AdmittedStart {
        intent: &wrong,
        ..input
    };
    assert!(record_admitted_start(&bad).is_err());
    assert!(journal.refs.borrow().is_empty());
    record_admitted_start(&input)?;
    let (identity, _) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let recorded =
        inspect_journal(&journal, &identity, JournalStage::Started)?.ok_or("missing start")?;
    assert_eq!(
        recorded.receipt.authentication,
        JournalAuthentication::TrustedPublishing
    );
    assert_eq!(
        recorded.receipt.expected_tools.generator_sha256,
        "1".repeat(64)
    );
    assert_eq!(recorded.receipt.packages.len(), 1);
    record_admitted_start(&input)?;
    assert_eq!(journal.refs.borrow().len(), 1);
    Ok(())
}

pub(super) fn reject_invalid_metadata_prerequisites(
    input: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) {
    use crate::crate_release_controller::{CrateReleaseObservation, repair_verified_metadata};
    use crate::crate_release_journal::JournalMutationKind;
    let github = input.github;
    let registry = input.registry;
    assert!(repair_verified_metadata(input, "demo", JournalMutationKind::CreateRelease).is_err());
    github.approved.set(false);
    assert!(repair_verified_metadata(input, "demo", JournalMutationKind::CreateTag).is_err());
    github.approved.set(true);
    github.owner_match.set(false);
    assert!(repair_verified_metadata(input, "demo", JournalMutationKind::CreateTag).is_err());
    github.owner_match.set(true);
    registry.archive.borrow_mut().push(0);
    assert!(repair_verified_metadata(input, "demo", JournalMutationKind::CreateTag).is_err());
    registry.archive.borrow_mut().pop();
    github.tag_exists.set(true);
    github.tag_wrong.set(true);
    assert!(repair_verified_metadata(input, "demo", JournalMutationKind::CreateTag).is_err());
    github.tag_wrong.set(false);
    github.tag_exists.set(false);
    let invalid_sequence = CrateReleaseObservation {
        sequence: 1000,
        ..*input
    };
    assert!(
        repair_verified_metadata(&invalid_sequence, "demo", JournalMutationKind::CreateTag)
            .is_err()
    );
}
