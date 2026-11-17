// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::TestResult;
use crate::{
    config::RepositoryConnection,
    https::HttpsTransport,
    listen::{Link, link_from_admin},
};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};
use velnor_runner_github::workflow_run::{RepositoryTarget, get_repository_scope};
use velnor_runner_github::{
    AdminConnectionCall, Exchange, Method, SessionRequest, Transport, TransportFail,
    admin_connection,
};

pub(super) struct Github {
    pub(super) runners: BTreeMap<i64, (String, i64)>,
    pub(super) lookups: Vec<String>,
    pub(super) lookup_clock: Option<(Arc<AtomicI64>, i64)>,
}

impl Github {
    pub(super) fn new(name: &str, exists: bool) -> Self {
        let mut runners = BTreeMap::from([(99, ("foreign-runner".to_owned(), 9))]);
        if exists {
            runners.insert(21, (name.to_owned(), 7));
        }
        Self {
            runners,
            lookups: Vec::new(),
            lookup_clock: None,
        }
    }
    pub(super) fn link(&mut self, binding: &RepositoryConnection) -> TestResult<Link> {
        let admin = admin_connection(
            self,
            &AdminConnectionCall {
                config_url: "https://github.com/owner/repo",
                registration_token: "registration",
            },
        )?;
        let scope = get_repository_scope(
            self,
            RepositoryTarget {
                id: 42,
                owner: "owner",
                name: "repo",
            },
            "pat",
        )?;
        Ok(link_from_admin(
            HttpsTransport::new("https://api.github.com")?,
            admin,
            binding,
            &scope,
        )?)
    }
}

impl Transport for Github {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        let (status, body) = match (&request.method, request.path.as_str()) {
            (Method::Post, "/actions/runner-registration") => (
                200,
                json!({"url":"https://admin.example.com","token":"admin"}),
            ),
            (Method::Get, "/repos/owner/repo") => (
                200,
                json!({"id":42,"name":"repo","full_name":"owner/repo","private":true,"owner":{"login":"owner"}}),
            ),
            (Method::Get, path) if path.ends_with("/agents") => {
                if let Some((clock, after_lookup)) = &self.lookup_clock {
                    clock.store(*after_lookup, Ordering::SeqCst);
                }
                let query = request.query.as_deref().unwrap_or_default();
                let name = query
                    .split('&')
                    .find_map(|p| p.strip_prefix("agentName="))
                    .unwrap_or_default();
                self.lookups.push(name.to_owned());
                let value = self
                    .runners
                    .iter()
                    .filter(|(_, (runner, _))| runner == name)
                    .map(|(id, (name, set))| json!({"id":id,"name":name,"runnerScaleSetId":set}))
                    .collect::<Vec<_>>();
                (200, json!({"count":value.len(),"value":value}))
            }
            (Method::Delete, path) => {
                let id = path
                    .rsplit('/')
                    .next()
                    .and_then(|value| value.parse::<i64>().ok())
                    .expect("runner id");
                self.runners.remove(&id);
                (204, serde_json::Value::Null)
            }
            _ => panic!("unexpected external GitHub request {request:?}"),
        };
        Ok(Exchange {
            status,
            body: if body.is_null() {
                Vec::new()
            } else {
                serde_json::to_vec(&body).expect("GitHub response")
            },
        })
    }
}
