// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::Cell;
use std::path::Path;
use std::time::{Duration, Instant};

use velnor_runner_github::{
    AckScope, AcquireOutcome, EncodedJit, JitResult, ParsedBatch, QueueSession, RefreshGate,
    RunnerReference, ScaleSetById, ScaleSetByName, ScaleSetCreate, ScaleSetFound, SessionError,
    WireError, ack, acquire, create_runner_scale_set, create_session, delete_session,
    get_runner_scale_set, get_runner_scale_set_by_id, jit, jit_request, list_runner_groups,
    list_scale_set_runners, refresh_session, remove_runner, sole_grant,
};

use zeroize::Zeroizing;

use super::owned;
use crate::config::{ConnectionId, RepositoryConnection};
use crate::error::{HostError, RustRunnerFailure};
use crate::https::HttpsTransport;
use crate::launch_blocking::ListenFault;
use crate::listen::{
    OWNER_NAME, PollPathResult, absolute_https, map_listen, poll_path_with, queue_path,
};
use crate::scale_set::{EnsureError, create_labels, map_session, open_binding_admin};

pub(super) const ACQUIRE_BUDGET: Duration = Duration::from_secs(30);
const JIT_ATTEMPTS: u32 = 3;
const JIT_GAP: Duration = Duration::from_secs(1);

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
    RefreshRequired,
    Acquired { jit: EncodedJit },
}

pub(super) enum Acked {
    Done,
    RefreshRequired,
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
    state: &Path,
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
    let shared = binding.organization.is_some();
    let id = match found {
        ScaleSetFound::Found(view) => {
            if shared {
                adopt_shared(state, &binding.connection_id, view.id)?;
            }
            view.id
        }
        ScaleSetFound::NotFound => {
            if shared {
                owned::record_intent(state, &binding.connection_id)?;
            }
            let created = match create_runner_scale_set(
                &mut transport,
                &ScaleSetCreate {
                    name,
                    runner_group_id: group,
                    labels: &create_labels(labels),
                    admin_token: &admin.token,
                },
            ) {
                Ok(view) => view.id,
                Err(error) => {
                    let error = map_session(error, &transport);
                    if shared && refused(&error) {
                        owned::drop_intent(state, &binding.connection_id)?;
                    }
                    return Err(error.into());
                }
            };
            let reread = get_runner_scale_set_by_id(
                &mut transport,
                &ScaleSetById {
                    scale_set_id: created,
                    name,
                    expected_labels: labels,
                    admin_token: &admin.token,
                },
            )
            .map_err(|error| map_session(error, &transport))?;
            if reread.id != created || !reread.runner_setting.disable_update {
                return Err(EnsureError::Uncertain.into());
            }
            if shared {
                owned::record_created(state, &binding.connection_id, created)?;
            }
            created
        }
    };
    Ok((admin, id))
}

fn refused(error: &EnsureError) -> bool {
    matches!(error, EnsureError::Unexpected { status, .. } if (400..500).contains(status))
}

fn adopt_shared(
    state: &Path,
    connection: &ConnectionId,
    scale_set_id: i64,
) -> Result<(), ListenFault> {
    match owned::created(state, connection)? {
        Some(recorded) if recorded == scale_set_id => Ok(()),
        None if owned::intended(state, connection)? => {
            Ok(owned::record_created(state, connection, scale_set_id)?)
        }
        _ => Err(EnsureError::Uncertain.into()),
    }
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

pub(super) fn remove_runner_named(
    admin: &Admin,
    scale_set_id: i64,
    name: &str,
) -> Result<(), ListenFault> {
    let mut transport = HttpsTransport::new(&admin.base)?;
    let named = named_runners(&mut transport, admin, scale_set_id, name)?;
    if named.is_empty() {
        return Ok(());
    }
    for runner in &named {
        remove_runner(&mut transport, runner.id, &admin.token).map_err(map_listen)?;
    }
    if named_runners(&mut transport, admin, scale_set_id, name)?.is_empty() {
        Ok(())
    } else {
        Err(HostError::RustRunner(RustRunnerFailure::RunnerStillRegistered).into())
    }
}

fn named_runners(
    transport: &mut HttpsTransport,
    admin: &Admin,
    scale_set_id: i64,
    name: &str,
) -> Result<Vec<RunnerReference>, ListenFault> {
    let mut runners =
        list_scale_set_runners(transport, scale_set_id, &admin.token).map_err(map_listen)?;
    runners.retain(|runner| runner.name == name);
    Ok(runners)
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
    assigned: bool,
) -> Result<Acquisition, ListenFault> {
    let mut transport =
        HttpsTransport::with_deadline(&admin.base, Instant::now() + ACQUIRE_BUDGET)?;
    if !assigned {
        let gate = RefreshGate::new();
        let refresh_requested = Cell::new(false);
        let outcome = acquire(
            &mut transport,
            scale_set_id,
            &[request_id],
            &[],
            &queue.token,
            &gate,
            || {
                refresh_requested.set(true);
                Err(WireError::RefreshExhausted)
            },
        );
        let Some(outcome) = refresh_or_fail(outcome, &refresh_requested)? else {
            return Ok(Acquisition::RefreshRequired);
        };
        match outcome {
            AcquireOutcome::Acquired(ids) if ids.is_empty() => return Ok(Acquisition::NotAcquired),
            AcquireOutcome::Acquired(ids) if sole_grant(&ids, request_id) => {}
            _ => return Err(EnsureError::Uncertain.into()),
        }
    }
    let request = jit_request(runner_name).map_err(|_| EnsureError::Rejected)?;
    let result = generate_jit(&mut transport, scale_set_id, &admin.token, &request)?;
    let runner = &result.runner;
    if runner.id <= 0 || runner.runner_scale_set_id != scale_set_id || runner.name != runner_name {
        return Err(EnsureError::Malformed.into());
    }
    Ok(Acquisition::Acquired {
        jit: result.encoded_jit_config,
    })
}

fn generate_jit(
    transport: &mut HttpsTransport,
    scale_set_id: i64,
    token: &str,
    request: &[u8],
) -> Result<JitResult, ListenFault> {
    let mut attempt = 1;
    loop {
        match jit(transport, scale_set_id, token, request) {
            Ok(result) => return Ok(result),
            Err(error) if attempt < JIT_ATTEMPTS => {
                eprintln!("rust-runner jit attempt {attempt} held: {error}");
                attempt += 1;
                std::thread::sleep(JIT_GAP);
            }
            Err(error) => return Err(map_listen(error).into()),
        }
    }
}

pub(super) fn acknowledge(queue: &QueueAccess, batch: &ParsedBatch) -> Result<Acked, ListenFault> {
    let mut transport = HttpsTransport::new(&queue.origin)?;
    let gate = RefreshGate::new();
    let refresh_requested = Cell::new(false);
    let acked = ack(
        &mut transport,
        &queue.path,
        batch,
        &AckScope {
            replay_safe: true,
            sole_unacquired_offer: false,
            queue_token: &queue.token,
        },
        &gate,
        || {
            refresh_requested.set(true);
            Err(WireError::RefreshExhausted)
        },
    );
    Ok(match refresh_or_fail(acked, &refresh_requested)? {
        Some(_) => Acked::Done,
        None => Acked::RefreshRequired,
    })
}

fn refresh_or_fail<T>(
    result: Result<T, SessionError>,
    refresh_requested: &Cell<bool>,
) -> Result<Option<T>, ListenFault> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(SessionError::Wire(WireError::RefreshExhausted)) if refresh_requested.get() => Ok(None),
        Err(error) => Err(map_listen(error).into()),
    }
}
