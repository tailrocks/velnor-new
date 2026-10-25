use velnor_runner_github::{Exchange, SessionRequest, Transport, TransportFail};

const PULL_REQUEST_RESPONSE: &[u8] = br#"{
    "number": 2110,
    "state": "open",
    "merged": false,
    "head": {
        "ref": "fix/runner",
        "sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "repo": {
            "id": 77,
            "name": "java-monorepo",
            "full_name": "chainargos/java-monorepo",
            "owner": { "login": "chainargos" },
            "private": true
        }
    },
    "base": {
        "ref": "sea/integration",
        "sha": "cccccccccccccccccccccccccccccccccccccccc",
        "repo": {
            "id": 77,
            "name": "java-monorepo",
            "full_name": "chainargos/java-monorepo",
            "owner": { "login": "chainargos" },
            "private": true
        }
    },
    "merge_commit_sha": "dddddddddddddddddddddddddddddddddddddddd"
}"#;

const PULL_REQUEST_RUN_RESPONSE: &[u8] = br#"{
    "id": 900,
    "event": "pull_request",
    "path": ".github/workflows/ci.yml@refs/pull/2110/merge",
    "head_branch": "fix/runner",
    "head_sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    "workflow_id": 77,
    "run_attempt": 1,
    "repository": {
        "id": 77,
        "name": "java-monorepo",
        "full_name": "chainargos/java-monorepo",
        "owner": { "login": "chainargos" },
        "private": true
    },
    "head_repository": {
        "id": 77,
        "name": "java-monorepo",
        "full_name": "chainargos/java-monorepo",
        "owner": { "login": "chainargos" },
        "private": true
    },
    "pull_requests": [{
        "number": 2110,
        "head": {
            "ref": "fix/runner",
            "sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "repo": { "id": 77, "name": "java-monorepo" }
        },
        "base": {
            "ref": "sea/integration",
            "sha": "cccccccccccccccccccccccccccccccccccccccc",
            "repo": { "id": 77, "name": "java-monorepo" }
        }
    }]
}"#;

const PUSH_RUN_RESPONSE: &[u8] = br#"{
    "id": 900,
    "event": "push",
    "path": ".github/workflows/ci.yml@refs/heads/sea/integration",
    "head_branch": "sea/integration",
    "head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "workflow_id": 77,
    "run_attempt": 1,
    "repository": {
        "id": 77,
        "name": "java-monorepo",
        "full_name": "chainargos/java-monorepo",
        "owner": { "login": "chainargos" },
        "private": true
    }
}"#;

pub(super) struct SourceApiFixture {
    pub(super) authorization: Option<String>,
    pub(super) authorizations: Vec<String>,
    pub(super) pull_request_responses: bool,
    pub(super) paths: Vec<String>,
}

impl Transport for SourceApiFixture {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.authorization = request
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .map(|(_, value)| value.clone());
        if let Some(authorization) = &self.authorization {
            self.authorizations.push(authorization.clone());
        }
        self.paths.push(request.path.clone());
        let body = if self.pull_request_responses && request.path.ends_with("/pulls/2110") {
            PULL_REQUEST_RESPONSE
        } else if self.pull_request_responses {
            PULL_REQUEST_RUN_RESPONSE
        } else {
            PUSH_RUN_RESPONSE
        };
        Ok(Exchange {
            status: 200,
            body: body.to_vec(),
        })
    }
}
