use velnor_runner_github::CompleteSourceMetadata;

pub(super) struct ReplayContext<'a> {
    pub(super) connection_id: &'a str,
    pub(super) scale_set_id: i64,
    pub(super) request_id: i64,
    pub(super) binding_digest: &'a str,
    pub(super) source: CompleteSourceMetadata<'a>,
}

pub(super) struct RunnerLinkContext<'a> {
    pub(super) runner_id: i64,
    pub(super) runner_name: &'a str,
    pub(super) expected_intent_id: i64,
    pub(super) event_digest: &'a str,
    pub(super) has_completed: bool,
    pub(super) completed: bool,
}

pub(super) struct StartedEventContext<'a> {
    pub(super) message_id: Option<i64>,
    pub(super) digest: Option<&'a str>,
    pub(super) job_id: Option<&'a str>,
    pub(super) runner_id: Option<i64>,
    pub(super) runner_name: Option<&'a str>,
    pub(super) has_completed: bool,
}

pub(super) struct CompletionEventContext<'a> {
    pub(super) message_id: i64,
    pub(super) result: Option<&'a str>,
    pub(super) runner_id: Option<i64>,
    pub(super) runner_name: Option<&'a str>,
    pub(super) job_id: Option<&'a str>,
}
