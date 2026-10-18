use std::env;

/// Require canonical positive GitHub run coordinates.
pub(super) fn valid_run_coordinates() -> bool {
    let run_id = env::var("GITHUB_RUN_ID").ok();
    let run_attempt = env::var("GITHUB_RUN_ATTEMPT").ok();
    matches!(
        (run_id, run_attempt),
        (Some(run_id), Some(run_attempt))
            if run_id.parse::<u64>().is_ok_and(|value| value > 0 && value.to_string() == run_id)
                && run_attempt.parse::<u32>().is_ok_and(|value| value > 0 && value.to_string() == run_attempt)
    )
}
