// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_github::{
    AckScope, AcquireOutcome, EncodedJit, ParsedBatch, QueueSession, RefreshGate, ScaleSetByName,
    ScaleSetCreate, ScaleSetFound, WireError, ack, acquire, create_runner_scale_set,
    create_session, delete_session, get_runner_by_name, get_runner_scale_set, jit, jit_request,
    list_runner_groups, list_scale_set_runners, refresh_session, remove_runner,
};
use zeroize::Zeroizing;

use crate::config::RepositoryConnection;
use crate::error::{HostError, RustRunnerFailure};
use crate::https::HttpsTransport;
use crate::launch_blocking::ListenFault;
use crate::listen::{
    OWNER_NAME, PollPathResult, absolute_https, map_listen, poll_path_with, queue_path,
};
use crate::scale_set::{EnsureError, create_labels, map_session, open_binding_admin};

#[derive(Clone)]
pub(super) struct Admin {
    token: Zeroizing<String>,
    base: String,
}

#[derive(Clone)]
pub(super) struct QueueAccess {
    token: Zeroizing<String>,
    origin: String,
    path: String,
}

pub(super) enum Acquisition {
    NotAcquired,
    Acquired { jit: EncodedJit },
}

impl QueueAccess {
    pub(super) fn of(session: &QueueSession, admin: &Admin) -> Result<Self, ListenFault> {
        let url = &session.message_queue_url;
        let (origin, path) = if let Some(path) = queue_path(&admin.base, url) {
            (admin.base.clone(), path.to_owned())
        } else {
            let absolute = absolute_https(url).ok_or(EnsureError::Unexpected {
                status: 0,
                step: "queue-path",
            })?;
            (absolute.origin, absolute.path)
        };
        Ok(Self {
            token: Zeroizing::new(session.token().to_owned()),
            origin,
            path,
        })
    }
}

pub(super) fn open_scale_set(
    pat: &str,
    binding: &RepositoryConnection,
) -> Result<(Admin, i64), ListenFault> {
    let (mut transport, connection) = open_binding_admin(pat, binding)?;
    let admin = Admin {
        token: Zeroizing::new(connection.expose_token().to_owned()),
        base: connection.expose_url().to_owned(),
    };
    let groups = list_runner_groups(&mut transport, &admin.token)
        .map_err(|error| map_session(error, &transport))?;
    let group = binding
        .select_runner_group(&groups)
        .map_err(|_| EnsureError::Rejected)?;
    let (name, labels) = (binding.scale_set.name(), binding.scale_set.labels());
    let found = get_runner_scale_set(
        &mut transport,
        &ScaleSetByName {
            runner_group_id: group,
            name,
            expected_labels: labels,
            admin_token: &admin.token,
        },
    )
    .map_err(|error| map_session(error, &transport))?;
    let id = match found {
        ScaleSetFound::Found(view) => view.id,
        ScaleSetFound::NotFound => {
            create_runner_scale_set(
                &mut transport,
                &ScaleSetCreate {
                    name,
                    runner_group_id: group,
                    labels: &create_labels(labels),
                    admin_token: &admin.token,
                },
            )
            .map_err(|error| map_session(error, &transport))?
            .id
        }
    };
    Ok((admin, id))
}

pub(super) fn renew_admin(pat: &str, binding: &RepositoryConnection) -> Result<Admin, ListenFault> {
    let (_, connection) = open_binding_admin(pat, binding)?;
    Ok(Admin {
        token: Zeroizing::new(connection.expose_token().to_owned()),
        base: connection.expose_url().to_owned(),
    })
}

pub(super) fn open_session(admin: &Admin, scale_set_id: i64) -> Result<QueueSession, ListenFault> {
    let mut transport = HttpsTransport::new(&admin.base)?;
    Ok(
        create_session(&mut transport, scale_set_id, OWNER_NAME, &admin.token)
            .map_err(map_listen)?,
    )
}

pub(super) fn refresh(
    pat: &str,
    binding: &RepositoryConnection,
    admin: &Admin,
    scale_set_id: i64,
    session_id: &str,
) -> Result<(Admin, QueueSession), ListenFault> {
    let mut transport = HttpsTransport::new(&admin.base)?;
    if let Ok(session) = refresh_session(&mut transport, scale_set_id, session_id, &admin.token) {
        return Ok((admin.clone(), session));
    }
    let renewed = renew_admin(pat, binding)?;
    let mut transport = HttpsTransport::new(&renewed.base)?;
    let session = refresh_session(&mut transport, scale_set_id, session_id, &renewed.token)
        .map_err(map_listen)?;
    Ok((renewed, session))
}

pub(super) fn close_session(
    admin: &Admin,
    scale_set_id: i64,
    session_id: &str,
) -> Result<(), ListenFault> {
    let mut transport = HttpsTransport::new(&admin.base)?;
    delete_session(&mut transport, scale_set_id, session_id, &admin.token)
        .map(drop)
        .map_err(|error| map_listen(error).into())
}

pub(super) fn clear_runners(admin: &Admin, scale_set_id: i64) -> Result<(), ListenFault> {
    let mut transport = HttpsTransport::new(&admin.base)?;
    let registered =
        list_scale_set_runners(&mut transport, scale_set_id, &admin.token).map_err(map_listen)?;
    let mut refused = false;
    for runner in &registered {
        refused |= remove_runner(&mut transport, runner.id, &admin.token).is_err();
    }
    if refused {
        return Err(HostError::RustRunner(RustRunnerFailure::RunnerRemovalRefused).into());
    }
    if !registered.is_empty()
        && !list_scale_set_runners(&mut transport, scale_set_id, &admin.token)
            .map_err(map_listen)?
            .is_empty()
    {
        return Err(HostError::RustRunner(RustRunnerFailure::RunnerStillRegistered).into());
    }
    Ok(())
}

pub(super) fn remove_runner_named(admin: &Admin, name: &str) -> Result<(), ListenFault> {
    let mut transport = HttpsTransport::new(&admin.base)?;
    let Some(runner) =
        get_runner_by_name(&mut transport, name, &admin.token).map_err(map_listen)?
    else {
        return Ok(());
    };
    remove_runner(&mut transport, runner.id, &admin.token).map_err(map_listen)?;
    if get_runner_by_name(&mut transport, name, &admin.token)
        .map_err(map_listen)?
        .is_some()
    {
        return Err(HostError::RustRunner(RustRunnerFailure::RunnerStillRegistered).into());
    }
    Ok(())
}

pub(super) fn poll(
    queue: &QueueAccess,
    capacity: u32,
    cursor: i64,
) -> Result<PollPathResult, ListenFault> {
    let mut transport = HttpsTransport::new(&queue.origin)?;
    Ok(poll_path_with(
        &mut transport,
        &queue.token,
        &queue.path,
        capacity,
        cursor,
    )?)
}

pub(super) fn acquire_and_jit(
    admin: &Admin,
    queue: &QueueAccess,
    scale_set_id: i64,
    request_id: i64,
    runner_name: &str,
) -> Result<Acquisition, ListenFault> {
    let mut transport = HttpsTransport::new(&admin.base)?;
    let gate = RefreshGate::new();
    let outcome = acquire(
        &mut transport,
        scale_set_id,
        &[request_id],
        &[],
        &queue.token,
        &gate,
        || Err(WireError::RefreshExhausted),
    )
    .map_err(map_listen)?;
    let AcquireOutcome::Acquired(ids) = outcome else {
        return Ok(Acquisition::NotAcquired);
    };
    if !ids.contains(&request_id) {
        return Ok(Acquisition::NotAcquired);
    }
    let request = jit_request(runner_name).map_err(|_| EnsureError::Rejected)?;
    let result = jit(&mut transport, scale_set_id, &admin.token, &request).map_err(map_listen)?;
    Ok(Acquisition::Acquired {
        jit: result.encoded_jit_config,
    })
}

pub(super) fn acknowledge(queue: &QueueAccess, batch: &ParsedBatch) -> Result<(), ListenFault> {
    let mut transport = HttpsTransport::new(&queue.origin)?;
    let gate = RefreshGate::new();
    ack(
        &mut transport,
        &queue.path,
        batch,
        &AckScope {
            replay_safe: true,
            sole_unacquired_offer: false,
            queue_token: &queue.token,
        },
        &gate,
        || Err(WireError::RefreshExhausted),
    )
    .map(drop)
    .map_err(|error| map_listen(error).into())
}
