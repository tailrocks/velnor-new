//! Stderr lines for `VELNOR_HTTPS_TRACE`. No tokens and no JIT.

use velnor_runner_github::{InnerKind, Poll, QueueSession};

pub(super) fn session(session: &QueueSession) {
    if std::env::var_os("VELNOR_HTTPS_TRACE").is_none() {
        return;
    }
    let stats = session.statistics();
    let assigned = stats.map_or(-1, velnor_runner_github::Statistics::assigned_population);
    let available = stats.map_or(-1, |item| item.total_available_jobs);
    let running = stats.map_or(-1, |item| item.total_running_jobs);
    let registered = stats.map_or(-1, |item| item.total_registered_runners);
    let busy = stats.map_or(-1, |item| item.total_busy_runners);
    let idle = stats.map_or(-1, |item| item.total_idle_runners);
    eprintln!(
        "session assigned={assigned} available={available} running={running} registered={registered} busy={busy} idle={idle}"
    );
}

pub(super) fn batch(polled: &Poll) {
    if std::env::var_os("VELNOR_HTTPS_TRACE").is_none() {
        return;
    }
    let Poll::Batch(batch) = polled else {
        eprintln!("batch empty");
        return;
    };
    let available = batch
        .statistics
        .as_ref()
        .map_or(-1, |stats| stats.total_available_jobs);
    let assigned = batch
        .statistics
        .as_ref()
        .map_or(-1, |stats| stats.total_assigned_jobs);
    eprintln!(
        "batch id={} jobs={} stats_available={available} stats_assigned={assigned}",
        batch.message_id,
        batch.jobs.len()
    );
    for job in &batch.jobs {
        let labels = job.labels.join(",");
        let job_id = job.job_id.as_deref().unwrap_or("-");
        let fields = job.fields.join(",");
        eprintln!(
            "batch job kind={} request_id={} job_id={job_id} labels={labels} fields={fields}",
            kind_name(&job.kind),
            job.request_id.unwrap_or(-1)
        );
    }
}

fn kind_name(kind: &InnerKind) -> String {
    match kind {
        InnerKind::Available => "JobAvailable".to_owned(),
        InnerKind::Assigned => "JobAssigned".to_owned(),
        InnerKind::Started => "JobStarted".to_owned(),
        InnerKind::Completed => "JobCompleted".to_owned(),
        InnerKind::Unsupported(name) => format!("other:{name}"),
    }
}
