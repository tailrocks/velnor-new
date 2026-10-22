//! Stderr lines for `VELNOR_HTTPS_TRACE`. No tokens and no JIT.

use velnor_runner_github::{InnerKind, Poll, QueueSession};

const MAX_KIND_NAME_INPUT_BYTES: usize = 128;
const MAX_KIND_NAME_OUTPUT_BYTES: usize = 192;

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
    let batch = match polled {
        Poll::Batch(batch) => batch,
        Poll::Quarantined(batch) => {
            eprintln!(
                "batch quarantined id={} bytes={}",
                batch.message_id,
                batch.raw_body.len()
            );
            return;
        }
        Poll::Empty => {
            eprintln!("batch empty");
            return;
        }
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
        InnerKind::Unsupported(name) => format!("other:{}", safe_kind_name(name)),
    }
}

fn safe_kind_name(name: &str) -> String {
    let mut safe = String::with_capacity(name.len().min(MAX_KIND_NAME_OUTPUT_BYTES));
    let mut input_bytes = 0;
    let mut truncated = false;
    let output_limit = MAX_KIND_NAME_OUTPUT_BYTES - 3;

    for character in name.chars() {
        let character_bytes = character.len_utf8();
        if input_bytes + character_bytes > MAX_KIND_NAME_INPUT_BYTES {
            truncated = true;
            break;
        }
        input_bytes += character_bytes;

        let escaped_bytes = if trace_control(character) {
            let value = u32::from(character);
            let hex_digits = value.bit_width().div_ceil(4).max(1);
            4 + hex_digits as usize
        } else {
            character_bytes
        };
        if safe.len() + escaped_bytes > output_limit {
            truncated = true;
            break;
        }

        if trace_control(character) {
            safe.push_str(&format!("\\u{{{:x}}}", u32::from(character)));
        } else {
            safe.push(character);
        }
    }

    if input_bytes < name.len() {
        truncated = true;
    }
    if truncated {
        safe.push_str("...");
    }
    safe
}

fn trace_control(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{206f}'
        )
}

#[cfg(test)]
mod tests {
    use super::{MAX_KIND_NAME_OUTPUT_BYTES, kind_name, safe_kind_name, trace_control};
    use velnor_runner_github::InnerKind;

    #[test]
    fn known_and_ordinary_kinds_keep_their_diagnostic_names() {
        assert_eq!(kind_name(&InnerKind::Available), "JobAvailable");
        assert_eq!(kind_name(&InnerKind::Assigned), "JobAssigned");
        assert_eq!(kind_name(&InnerKind::Started), "JobStarted");
        assert_eq!(kind_name(&InnerKind::Completed), "JobCompleted");
        assert_eq!(
            kind_name(&InnerKind::Unsupported("future-kind".to_owned())),
            "other:future-kind"
        );
    }

    #[test]
    fn hostile_unsupported_names_are_escaped_and_bounded() {
        let hostile = format!("new\n\u{1b}[31m\u{009b}31m\u{202e}{}", "x".repeat(1024));
        let formatted = kind_name(&InnerKind::Unsupported(hostile));

        assert!(formatted.starts_with("other:new\\u{a}\\u{1b}[31m\\u{9b}31m\\u{202e}"));
        assert!(formatted.ends_with("..."));
        assert!(formatted.len() <= "other:".len() + MAX_KIND_NAME_OUTPUT_BYTES);
        assert!(formatted.chars().all(|character| !trace_control(character)));
    }

    #[test]
    fn truncation_preserves_utf8_boundaries() {
        let safe = safe_kind_name(&"界".repeat(100));

        assert!(safe.is_char_boundary(safe.len()));
        assert!(safe.len() <= MAX_KIND_NAME_OUTPUT_BYTES);
        assert!(safe.ends_with("..."));
        assert!(safe.starts_with("界"));
    }
}
