//! Private queued JSON transport for exercising the real native API adapter.

use super::{GitHubApi, OrchestratorError, internal};
use serde_json::Value;
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

pub(crate) struct JsonFixture {
    responses: RefCell<VecDeque<(String, Value)>>,
    observed: RefCell<Vec<String>>,
}

impl JsonFixture {
    pub(super) fn read(&self, route: &str) -> Result<Value, OrchestratorError> {
        self.observed.borrow_mut().push(route.to_owned());
        let Some((expected, response)) = self.responses.borrow_mut().pop_front() else {
            return Err(internal("unexpected_fixture_api_call"));
        };
        assert_eq!(route, expected, "native API endpoint/order changed");
        Ok(response)
    }

    pub(super) fn mutate(
        &self,
        method: &str,
        route: &str,
        fields: &[(&str, String)],
    ) -> Result<Vec<u8>, OrchestratorError> {
        let expectation = self.read(&format!("{method} {route}"))?;
        let actual = serde_json::Value::Object(
            fields
                .iter()
                .map(|(key, value)| ((*key).to_owned(), Value::String(value.clone())))
                .collect(),
        );
        assert_eq!(
            expectation.get("expected_fields"),
            Some(&actual),
            "native mutation fields changed"
        );
        if expectation.get("transport_error") == Some(&Value::Bool(true)) {
            return Err(internal("fixture_ambiguous_transport_error"));
        }
        let response = expectation
            .get("response")
            .expect("mutation fixture response");
        serde_json::to_vec(response).map_err(|_| internal("fixture_response_json"))
    }

    pub(crate) fn assert_drained(&self) {
        assert!(
            self.responses.borrow().is_empty(),
            "native API skipped expected calls: {:?}",
            self.responses
                .borrow()
                .iter()
                .map(|(route, _)| route)
                .collect::<Vec<_>>()
        );
    }

    pub(crate) fn observed(&self) -> Vec<String> {
        self.observed.borrow().clone()
    }
}

impl GitHubApi {
    pub(crate) fn with_fixture(
        repository: &str,
        responses: Vec<(String, Value)>,
    ) -> (Self, Rc<JsonFixture>) {
        let fixture = Rc::new(JsonFixture {
            responses: RefCell::new(responses.into()),
            observed: RefCell::new(Vec::new()),
        });
        let mut api = Self::new(repository.to_owned());
        api.fixture = Some(Rc::clone(&fixture));
        (api, fixture)
    }
}
