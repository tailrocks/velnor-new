//! Run bounded control requests in a same-binary child.

use super::{
    AsyncReadExt, AsyncWriteExt, ChildWait, ConnectionId, Duration, ExitCode, HostError,
    INTERNAL_FLAG, Instant, OsStr, Path, PathBuf, REAP_RESERVE, Read, RepositoryConnection,
    StdInstant, Stdio, Write, bounded_wait, reactivate_connection_if_drained, stop_and_reap,
    timeout_at, wait_for_connection_detached, wait_for_connection_drain, wait_for_drain,
    write_readiness,
};
use tokio::process::Command;

const CONTROL_PARENT_FLAG: &str = "--internal-control-request";
const CONTROL_LEAF_FLAG: &str = "--internal-control-leaf";
const MAX_CONTROL_BYTES: u64 = 65_536;
const MAX_CONTROL_RESULT: u64 = 16;

/// Run the private child operation when its exact argument shape is present.
pub(crate) fn run_internal() -> Option<ExitCode> {
    let mut args = std::env::args_os();
    let _program = args.next()?;
    match args.next()?.as_os_str() {
        flag if flag == OsStr::new(INTERNAL_FLAG) => {
            let Some(state) = args.next() else {
                return Some(ExitCode::from(2));
            };
            if args.next().is_some() {
                return Some(ExitCode::from(2));
            }
            Some(write_readiness(&PathBuf::from(state)))
        }
        flag if flag == OsStr::new(CONTROL_PARENT_FLAG) => {
            if args.next().is_some() {
                return Some(ExitCode::from(2));
            }
            Some(run_control_parent())
        }
        flag if flag == OsStr::new(CONTROL_LEAF_FLAG) => {
            if args.next().is_some() {
                return Some(ExitCode::from(2));
            }
            Some(run_control_leaf())
        }
        _ => None,
    }
}

pub(crate) fn wait_for_drain_until(state: &Path, deadline: StdInstant) -> Result<bool, HostError> {
    run_control_job(control_job("drain", state, None, None, deadline)?, deadline)
}

pub(crate) fn wait_for_connection_drain_until(
    state: &Path,
    connection_id: &ConnectionId,
    deadline: StdInstant,
) -> Result<bool, HostError> {
    run_control_job(
        control_job(
            "connection_drain",
            state,
            Some(serde_json::Value::String(connection_id.as_str().to_owned())),
            None,
            deadline,
        )?,
        deadline,
    )
}

pub(crate) fn wait_for_connection_detached_until(
    state: &Path,
    binding: &RepositoryConnection,
    deadline: StdInstant,
) -> Result<bool, HostError> {
    let binding = serde_json::to_value(binding).map_err(|_| HostError::Config)?;
    run_control_job(
        control_job("detached", state, None, Some(binding), deadline)?,
        deadline,
    )
}

pub(crate) fn reactivate_connection_until(
    state: &Path,
    binding: &RepositoryConnection,
    deadline: StdInstant,
) -> Result<bool, HostError> {
    let binding = serde_json::to_value(binding).map_err(|_| HostError::Config)?;
    run_control_job(
        control_job("reactivate", state, None, Some(binding), deadline)?,
        deadline,
    )
}

fn control_job(
    operation: &str,
    state: &Path,
    connection_id: Option<serde_json::Value>,
    binding: Option<serde_json::Value>,
    deadline: StdInstant,
) -> Result<Vec<u8>, HostError> {
    let remaining = deadline.saturating_duration_since(StdInstant::now());
    if remaining.is_zero() {
        return Ok(br#"{"operation":"expired"}"#.to_vec());
    }
    let milliseconds =
        u64::try_from(remaining.as_millis().max(1)).map_err(|_| HostError::Config)?;
    let mut object = serde_json::Map::new();
    object.insert("operation".to_owned(), operation.into());
    object.insert(
        "state".to_owned(),
        serde_json::to_value(state).map_err(|_| HostError::Path)?,
    );
    object.insert("timeout_ms".to_owned(), milliseconds.into());
    if let Some(connection_id) = connection_id {
        object.insert("connection_id".to_owned(), connection_id);
    }
    if let Some(binding) = binding {
        object.insert("binding".to_owned(), binding);
    }
    serde_json::to_vec(&serde_json::Value::Object(object)).map_err(|_| HostError::Config)
}

fn run_control_job(request: Vec<u8>, deadline: StdInstant) -> Result<bool, HostError> {
    if request == br#"{"operation":"expired"}"# {
        return Ok(false);
    }
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .enable_io()
        .build()
    else {
        return Err(HostError::Lock);
    };
    match runtime.block_on(run_control_child(request, Instant::from_std(deadline))) {
        Ok(Some(value)) => Ok(value),
        Ok(None) => Ok(false),
        Err(()) => Err(HostError::Lock),
    }
}

async fn run_control_child(request: Vec<u8>, deadline: Instant) -> Result<Option<bool>, ()> {
    let now = Instant::now();
    let remaining = deadline.saturating_duration_since(now);
    if remaining.is_zero() {
        return Ok(None);
    }
    let reap_reserve = REAP_RESERVE.min(remaining / 4);
    let operation_deadline = deadline.checked_sub(reap_reserve).unwrap_or(now);
    let Ok(mut command) = control_command() else {
        return Err(());
    };
    let Ok(mut child) = command.spawn() else {
        return Err(());
    };
    if let Some(mut stdin) = child.stdin.take() {
        let write = timeout_at(operation_deadline, async {
            stdin.write_all(&request).await?;
            stdin.shutdown().await
        })
        .await;
        if !matches!(write, Ok(Ok(()))) {
            return if stop_and_reap(&mut child, deadline).await {
                Ok(None)
            } else {
                Err(())
            };
        }
    } else {
        let _ = stop_and_reap(&mut child, deadline).await;
        return Err(());
    }
    match bounded_wait(&mut child, operation_deadline, deadline).await {
        ChildWait::Exited(status) if status.success() => {
            let Some(stdout) = child.stdout.take() else {
                return Err(());
            };
            let mut output = Vec::new();
            let mut limited = stdout.take(MAX_CONTROL_RESULT);
            let read = timeout_at(deadline, limited.read_to_end(&mut output)).await;
            if !matches!(read, Ok(Ok(_))) || output.len() > 5 {
                return Err(());
            }
            match output.as_slice() {
                b"true\n" => Ok(Some(true)),
                b"false\n" => Ok(Some(false)),
                _ => Err(()),
            }
        }
        ChildWait::Exited(_) | ChildWait::Failed => Err(()),
        ChildWait::TimedOut => Ok(None),
    }
}

fn control_command() -> std::io::Result<Command> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .env_clear()
        .arg(CONTROL_LEAF_FLAG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    Ok(command)
}

fn run_control_parent() -> ExitCode {
    let Ok(request) = read_control_input() else {
        return ExitCode::from(2);
    };
    let Some(timeout_ms) = request
        .get("timeout_ms")
        .and_then(serde_json::Value::as_u64)
    else {
        return ExitCode::from(2);
    };
    if timeout_ms == 0 || timeout_ms > 300_000 {
        return ExitCode::from(2);
    }
    let deadline = StdInstant::now() + Duration::from_millis(timeout_ms);
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .enable_io()
        .build()
    else {
        return ExitCode::from(2);
    };
    match runtime.block_on(run_control_child(
        serde_json::to_vec(&request).unwrap_or_default(),
        Instant::from_std(deadline),
    )) {
        Ok(Some(value)) => write_control_result(value),
        Ok(None) => write_control_result(false),
        Err(()) => ExitCode::from(1),
    }
}

fn run_control_leaf() -> ExitCode {
    let Ok(request) = read_control_input() else {
        return ExitCode::from(2);
    };
    let Some(operation) = request.get("operation").and_then(serde_json::Value::as_str) else {
        return ExitCode::from(2);
    };
    let Some(state) = request
        .get("state")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
    else {
        return ExitCode::from(2);
    };
    let Some(timeout_ms) = request
        .get("timeout_ms")
        .and_then(serde_json::Value::as_u64)
    else {
        return ExitCode::from(2);
    };
    if timeout_ms == 0 || timeout_ms > 300_000 {
        return ExitCode::from(2);
    }
    let timeout = Duration::from_millis(timeout_ms);
    let result = match operation {
        "drain" if exact_control_fields(&request, &["operation", "state", "timeout_ms"]) => {
            wait_for_drain(&state, timeout)
        }
        "connection_drain"
            if exact_control_fields(
                &request,
                &["operation", "state", "timeout_ms", "connection_id"],
            ) =>
        {
            let Some(raw_id) = request
                .get("connection_id")
                .and_then(serde_json::Value::as_str)
            else {
                return ExitCode::from(2);
            };
            let Ok(id) = ConnectionId::parse(raw_id) else {
                return ExitCode::from(2);
            };
            wait_for_connection_drain(&state, &id, timeout)
        }
        "detached" | "reactivate"
            if exact_control_fields(&request, &["operation", "state", "timeout_ms", "binding"]) =>
        {
            let Some(value) = request.get("binding").cloned() else {
                return ExitCode::from(2);
            };
            let Ok(binding) = serde_json::from_value::<RepositoryConnection>(value) else {
                return ExitCode::from(2);
            };
            if operation == "detached" {
                wait_for_connection_detached(&state, &binding, timeout)
            } else {
                reactivate_connection_if_drained(&state, &binding)
            }
        }
        _ => return ExitCode::from(2),
    };
    match result {
        Ok(value) => write_control_result(value),
        Err(_) => ExitCode::from(1),
    }
}

fn read_control_input() -> Result<serde_json::Value, ()> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_CONTROL_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.is_empty() || u64::try_from(bytes.len()).map_or(true, |size| size > MAX_CONTROL_BYTES)
    {
        return Err(());
    }
    serde_json::from_slice(&bytes).map_err(|_| ())
}

fn exact_control_fields(request: &serde_json::Value, allowed: &[&str]) -> bool {
    request.as_object().is_some_and(|object| {
        object.len() == allowed.len() && object.keys().all(|key| allowed.contains(&key.as_str()))
    })
}

fn write_control_result(value: bool) -> ExitCode {
    let mut output = std::io::stdout().lock();
    let result: &[u8] = if value { b"true\n" } else { b"false\n" };
    if output.write_all(result).is_err() || output.flush().is_err() {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
}
