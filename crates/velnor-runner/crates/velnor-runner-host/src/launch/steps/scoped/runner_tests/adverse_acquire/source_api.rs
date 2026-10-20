use velnor_runner_github::{
    Exchange, Method, SessionRequest, SourceMetadata, Transport, TransportFail,
};

use super::super::support::exchange;

#[derive(Default)]
pub(super) struct SourceApi {
    pub(super) calls: usize,
}

impl Transport for SourceApi {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        assert_eq!(request.method, Method::Get);
        self.calls += 1;
        let body = serde_json::json!({
            "id": 900,
            "event": "push",
            "path": ".github/workflows/build.yml@refs/heads/main",
            "head_branch": "main",
            "head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "workflow_id": 201,
            "run_attempt": 1,
            "repository": {
                "id": 101,
                "name": "repository",
                "full_name": "owner/repository",
                "owner": { "login": "owner" },
                "private": true
            }
        });
        Ok(exchange(200, &body.to_string()))
    }
}

pub(super) fn trusted_metadata() -> SourceMetadata {
    SourceMetadata {
        owner_name: Some("owner".to_owned()),
        repository_name: Some("repository".to_owned()),
        job_workflow_ref: Some(
            "owner/repository/.github/workflows/build.yml@refs/heads/main".to_owned(),
        ),
        workflow_run_id: Some(900),
        event_name: Some("push".to_owned()),
    }
}
