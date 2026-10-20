use std::{mem::size_of_val, path::PathBuf};

use bollard::Docker;

use crate::{
    config::{
        ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
        SourcePolicyMode, WorkflowEvent,
    },
    journal::{EngineContext, Journal},
    launch::{
        SelectedEngineContext, resource_probe::IntakeCompositionProvider,
        steps::ScopedProvisioningOutcome,
    },
};

#[tokio::test]
async fn shared_scoped_prepare_future_stays_small_and_preserves_no_start() {
    let path = journal_path();
    let socket_path = socket_path();
    let journal = Journal::open(&path)
        .await
        .expect("the future-size journal must open");
    let binding = repository_binding();
    let scoped = journal
        .connection_scope(&binding)
        .await
        .expect("the test connection scope must bind");
    let images = crate::launch::verified_images::test_worker_images_for(
        "future-size-engine",
        &"a".repeat(64),
        "aarch64",
        "linux/amd64",
    );
    let engine = SelectedEngineContext::bind(
        &EngineContext {
            engine_id: "future-size-engine".to_owned(),
            endpoint: "unix:///var/run/docker.sock".to_owned(),
            platform: "linux/amd64".to_owned(),
        },
        &images,
    )
    .expect("the test engine must bind to its image identities");
    let provider = IntakeCompositionProvider::new(&engine, images.source_sha());
    let _socket = std::os::unix::net::UnixListener::bind(&socket_path)
        .expect("a local unused Docker socket must bind");
    let docker = Docker::connect_with_socket(
        socket_path.to_str().expect("socket path must be UTF-8"),
        120,
        &bollard::API_DEFAULT_VERSION,
    )
    .expect("a lazy Docker client must be constructible");
    let future = super::prepare_and_bind_scoped_with_provider(
        ScopedProvisioningOutcome::NoStart,
        super::scoped_prepare_context(&docker, &scoped, &journal, &provider, &engine, &images),
    );
    let size = size_of_val(&future);
    eprintln!("shared_scoped_prepare_future_bytes={size}");
    assert!(size <= 2 * size_of::<usize>());
    let outcome = future
        .await
        .expect("NoStart must not use Docker, journal, or provider");
    assert!(matches!(outcome, super::ScopedRuntimeOutcome::NoStart));
    drop(scoped);
    drop(journal);
    std::fs::remove_file(path).expect("the test journal must be removable");
    std::fs::remove_file(socket_path).expect("the test socket must be removable");
}

fn journal_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "velnor-scoped-future-size-{}.db",
        std::process::id()
    ))
}

fn socket_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "velnor-scoped-future-size-{}.sock",
        std::process::id()
    ))
}

fn repository_binding() -> RepositoryConnection {
    let labels = ["velnor".to_owned(), "integration".to_owned()];
    let selector =
        ScaleSetSelector::try_new("integration", &labels).expect("the test selector must be valid");
    let policy = SourcePolicy::try_new(
        SourcePolicyMode::Repository,
        Vec::new(),
        Vec::new(),
        vec![WorkflowEvent::Push],
        Vec::new(),
    )
    .expect("the test source policy must be valid");
    RepositoryConnection::try_new(
        ConnectionId::parse("scoped-future-size").expect("test ID must be valid"),
        101,
        "owner/repository".to_owned(),
        KeychainReference::parse("keychain:velnor/scoped-future-size")
            .expect("test keychain reference must be valid"),
        selector,
        policy,
    )
    .expect("test repository binding must be valid")
}
