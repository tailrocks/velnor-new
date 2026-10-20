use std::{collections::BTreeMap, sync::Arc};

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

use super::super::state::{PreparedConnection, SessionCredentials, session_assigned_total};

use super::{PrepareConnectionRequest, bind_connection, connection_pat};

struct ConnectionAuthority {
    setup: ConnectionSetup,
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
    Ok(ConnectionAuthority { setup, github_pat })
}

async fn finish_connection_preparation(
    request: PrepareConnectionRequest<'_>,
    authority: ConnectionAuthority,
) -> Result<PreparedConnection, ListenFault> {
    let PrepareConnectionRequest {
        state,
        journal,
        binding,
        selected_engine,
        worker_images,
        docker,
        options,
        ..
    } = request;
    let ConnectionAuthority {
        setup:
            ConnectionSetup {
                set,
                mut link,
                scoped,
            },
        github_pat,
    } = authority;
    let guest_image = write_guest_image(state, binding, worker_images)?;
    guest_image.runner_bind(binding, worker_images)?;
    let native_cleanup = NativeCleanupRuntime::new(
        docker.clone(),
        journal.clone(),
        EngineContext {
            engine_id: selected_engine.engine_id().to_owned(),
            endpoint: selected_engine.endpoint().to_owned(),
            platform: selected_engine.platform().to_owned(),
        },
        selected_engine.clone(),
        worker_images.clone(),
        Arc::clone(options.release_read_secret),
    )?;
    let completion = CompletionWorker::start_scoped(
        scoped.clone(),
        native_cleanup,
        docker.clone(),
        selected_engine.clone(),
        link.base(),
        link.token().to_owned(),
        state,
    )?;
    let (session, session_intent, _header_allocation) = open_connection_session(
        &mut link,
        set.id,
        &scoped,
        options.requested_header_capacity,
    )
    .await?;
    let assigned_total = session_assigned_total(session.statistics());
    Ok(PreparedConnection {
        binding: binding.clone(),
        scope: scoped,
        set_id: set.id,
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
