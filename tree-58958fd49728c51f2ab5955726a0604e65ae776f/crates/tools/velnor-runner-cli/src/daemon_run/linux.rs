//! CLI entrypoint for State's Linux daemon coordinator.

use std::process::ExitCode;

use velnor_runner_host::{
    DaemonLock, ProtectedStateDirectory, ValidatedHostConfigSnapshot, load_configured_secret,
};
use velnor_runner_launch::linux::{
    LinuxDaemonOutcome, LinuxLaunchContext, LinuxLaunchCredentials, run_linux_daemon,
};

pub(super) struct PreparedDaemon {
    context: LinuxLaunchContext,
    profile_available: bool,
    credential_reference: String,
}

pub(super) fn prepare(
    state_directory: &std::path::Path,
    snapshot: ValidatedHostConfigSnapshot,
) -> Result<PreparedDaemon, velnor_runner_launch::linux::LinuxDaemonError> {
    let profile_available = snapshot.runner_image_profile().is_some();
    let credential_reference = snapshot.config().github.credential_ref.clone();
    let context = LinuxLaunchContext::from_snapshot(snapshot, state_directory)?;
    Ok(PreparedDaemon {
        context,
        profile_available,
        credential_reference,
    })
}

pub(super) fn run(
    lock: &DaemonLock,
    prepared: PreparedDaemon,
    protected_state: ProtectedStateDirectory,
) -> ExitCode {
    if !lock.is_held() {
        eprintln!("daemon lock lost");
        return ExitCode::from(1);
    }

    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    else {
        eprintln!("Linux daemon runtime unavailable");
        return ExitCode::from(1);
    };

    runtime.block_on(run_with_coordinator(prepared, protected_state))
}

async fn run_with_coordinator(
    prepared: PreparedDaemon,
    protected_state: ProtectedStateDirectory,
) -> ExitCode {
    let PreparedDaemon {
        context,
        profile_available,
        credential_reference,
    } = prepared;

    let Ok(signal_adapter) = super::shutdown_signal::install(context.drain_timeout()) else {
        eprintln!("Linux shutdown signal handlers unavailable");
        return ExitCode::from(1);
    };

    // No source-qualified runner profile means cleanup-only operation. Avoid
    // loading the host PAT when the immutable snapshot cannot authorize a
    // runner profile; State independently enforces this gate before preflight.
    let credentials = if profile_available {
        load_credentials(&credential_reference)
    } else {
        None
    };
    if profile_available && credentials.is_none() {
        eprintln!("host credential unavailable; Linux daemon remains cleanup-only");
    }

    let outcome = Box::pin(run_linux_daemon(
        context,
        credentials,
        protected_state,
        signal_adapter.receiver(),
    ))
    .await;
    drop(signal_adapter);

    match outcome {
        Ok(LinuxDaemonOutcome::Quiescent {
            admission,
            cleaned_generations,
        }) => {
            eprintln!(
                "Linux daemon exited with verified quiescence: admission={admission:?}, cleaned_generations={cleaned_generations}"
            );
            ExitCode::SUCCESS
        }
        Ok(LinuxDaemonOutcome::Deadline {
            admission,
            occupied_launches,
            unresolved_intents,
            gap,
            owned_resources,
        }) => {
            eprintln!(
                "Linux daemon shutdown deadline elapsed without quiescence proof: admission={admission:?}, gap={gap:?}, occupied_launches={occupied_launches:?}, unresolved_intents={unresolved_intents:?}, owned_resources={owned_resources:?}"
            );
            ExitCode::from(1)
        }
        Ok(LinuxDaemonOutcome::Unresolved {
            admission,
            gap,
            occupied_launches,
            unresolved_intents,
            owned_resources,
            cleaned_generations,
        }) => {
            eprintln!(
                "Linux daemon exited without quiescence proof: admission={admission:?}, gap={gap:?}, occupied_launches={occupied_launches:?}, unresolved_intents={unresolved_intents:?}, owned_resources={owned_resources:?}, cleaned_generations={cleaned_generations}"
            );
            ExitCode::from(1)
        }
        Err(error) => {
            eprintln!("Linux daemon failed: {error}");
            ExitCode::from(1)
        }
    }
}

fn load_credentials(reference: &str) -> Option<LinuxLaunchCredentials> {
    let secret = load_configured_secret(reference).ok()?;
    let pat = super::credential_text(secret.as_slice())?;
    // These role-separated values may be the same host-only PAT. Its actual
    // required union permissions are enforced by the bounded preflight; this
    // credential is never passed to worker/JIT environment construction.
    LinuxLaunchCredentials::new(pat.to_owned(), pat.to_owned()).ok()
}
