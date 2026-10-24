use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    error::HostError,
    journal::EngineContext,
    launch::{
        CompletionWorker, bootstrap::NativeCleanupRuntime, open_connection_session,
        scoped_runtime::RunnerReadyRegistry,
    },
    launch_blocking::ListenFault,
    scale_set::{ConnectionSetup, ensure_connection_scale_set},
    worker::guest_image::write_guest_image,
};

use super::super::state::{
    PreparedConnection, PreparedSetup, SessionCredentials, session_assigned_total,
};

use super::{PrepareConnectionRequest, bind_connection, connection_pat};

struct ConnectionAuthority {
    setup: ConnectionSetup,
    captured_at_unix_ms: u64,
    github_pat: crate::listen::Secret,
}

pub(super) async fn prepare_connection(
    request: PrepareConnectionRequest<'_>,
) -> Result<PreparedConnection, ListenFault> {
    let authority = prepare_connection_authority(&request).await?;
    finish_connection_preparation(request, authority).await
}

async fn prepare_connection_authority(
    request: &PrepareConnectionRequest<'_>,
) -> Result<ConnectionAuthority, ListenFault> {
    bind_connection(
        request.journal,
        request.binding,
        request.config,
        request.selected_engine,
    )
    .await?;
    let credential = crate::keychain::load_secret(
        request.binding.credential_ref.service(),
        request.binding.credential_ref.account(),
    )?;
    let pat = connection_pat(&credential)?;
    let github_pat = crate::listen::Secret::new(pat);
    let setup = ensure_connection_scale_set(pat, request.binding, request.journal).await?;
    let captured_at_unix_ms = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HostError::Identity)?
            .as_millis(),
    )
    .map_err(|_| HostError::Identity)?;
    drop(credential);
    let stored = setup
        .scoped
        .scale_set()
        .await?
        .ok_or(HostError::Ownership)?;
    let source_binding = setup.scoped.source_authorization_binding().await?;
    if stored.id != setup.set.id
        || stored.name != setup.set.name
        || stored.ownership != setup.set.ownership
        || source_binding.scale_set_id() != setup.set.id
    {
        return Err(HostError::Ownership.into());
    }
    Ok(ConnectionAuthority {
        setup,
        captured_at_unix_ms,
        github_pat,
    })
}

async fn finish_connection_preparation(
    request: PrepareConnectionRequest<'_>,
    authority: ConnectionAuthority,
) -> Result<PreparedConnection, ListenFault> {
    let guest_image = write_guest_image(request.state, request.binding, request.worker_images)?;
    guest_image.runner_bind(request.binding, request.worker_images)?;
    let native_cleanup = native_cleanup_runtime(
        request.docker,
        request.journal,
        request.selected_engine,
        request.worker_images,
        request.options.release_read_secret,
    )?;
    let completion = start_completion_worker(&authority.setup, &request, native_cleanup)?;
    let PrepareConnectionRequest {
        binding, options, ..
    } = request;
    let ConnectionAuthority {
        setup:
            ConnectionSetup {
                set,
                groups,
                mut link,
                scoped,
            },
        github_pat,
        captured_at_unix_ms,
    } = authority;
    let (session, session_intent, _header_allocation) = open_connection_session(
        &mut link,
        set.id,
        &scoped,
        options.requested_header_capacity,
    )
    .await?;
    let assigned_total = session_assigned_total(session.statistics());
    let set_id = set.id;
    Ok(PreparedConnection {
        binding: binding.clone(),
        scope: scoped,
        set_id,
        setup: PreparedSetup {
            set,
            groups,
            captured_at_unix_ms,
        },
        session_intent,
        assigned_total,
        poll_cursor: 0,
        poll_retry_at: None,
        guest_image,
        completion: Some(completion),
        credentials: Some(SessionCredentials {
            link,
            session,
            github_pat,
        }),
        runner_ready: RunnerReadyRegistry::new(),
        ready_intents: BTreeMap::new(),
        authorizing: BTreeMap::new(),
        pending_bootstrap: BTreeMap::new(),
        unkeyed_bootstrap: Vec::new(),
        authorized: BTreeMap::new(),
        pending_authorization: BTreeMap::new(),
        unkeyed_authorization: Vec::new(),
        pending_acquired_recovery: BTreeMap::new(),
        unkeyed_acquired_recovery: Vec::new(),
    })
}

fn native_cleanup_runtime(
    docker: &bollard::Docker,
    journal: &crate::journal::Journal,
    selected_engine: &crate::launch::SelectedEngineContext,
    worker_images: &crate::launch::VerifiedWorkerImages,
    release_read_secret: &Arc<crate::launch::ReleaseReadSecret>,
) -> Result<NativeCleanupRuntime, ListenFault> {
    Ok(NativeCleanupRuntime::new(
        docker.clone(),
        journal.clone(),
        EngineContext {
            engine_id: selected_engine.engine_id().to_owned(),
            endpoint: selected_engine.endpoint().to_owned(),
            platform: selected_engine.platform().to_owned(),
        },
        selected_engine.clone(),
        worker_images.clone(),
        Arc::clone(release_read_secret),
    )?)
}

fn start_completion_worker(
    setup: &ConnectionSetup,
    request: &PrepareConnectionRequest<'_>,
    native_cleanup: NativeCleanupRuntime,
) -> Result<CompletionWorker, ListenFault> {
    Ok(CompletionWorker::start_scoped(
        setup.scoped.clone(),
        native_cleanup,
        request.docker.clone(),
        request.selected_engine.clone(),
        setup.link.base(),
        setup.link.token().to_owned(),
        request.state,
    )?)
}
