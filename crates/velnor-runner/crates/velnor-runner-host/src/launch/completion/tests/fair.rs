use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::launch::install_job_capacity;

use super::*;

#[derive(Clone)]
struct FairRunnerApi {
    failures: Arc<HashSet<String>>,
    ids: Arc<HashMap<String, i64>>,
    registered: Arc<Mutex<HashSet<String>>>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl FairRunnerApi {
    fn new(failed_names: &[String], names_and_ids: &[(String, i64)]) -> Self {
        Self {
            failures: Arc::new(failed_names.iter().cloned().collect()),
            ids: Arc::new(names_and_ids.iter().cloned().collect()),
            registered: Arc::new(Mutex::new(
                names_and_ids.iter().map(|(name, _)| name.clone()).collect(),
            )),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn calls(&self) -> Result<Vec<String>, String> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .map_err(|_| "fair API call log was poisoned".to_owned())
    }
}

impl Transport for FairRunnerApi {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        match request.method {
            Method::Get => {
                let name = request
                    .query
                    .as_deref()
                    .and_then(|query| {
                        query
                            .split('&')
                            .find_map(|part| part.strip_prefix("agentName="))
                    })
                    .ok_or(TransportFail::Reset)?
                    .to_owned();
                self.calls
                    .lock()
                    .map_err(|_| TransportFail::Reset)?
                    .push(name.clone());
                if self.failures.contains(&name) {
                    return Ok(Exchange {
                        status: 503,
                        body: Vec::new(),
                    });
                }
                if self
                    .registered
                    .lock()
                    .map_err(|_| TransportFail::Reset)?
                    .contains(&name)
                {
                    let id = self.ids.get(&name).copied().ok_or(TransportFail::Reset)?;
                    let body = serde_json::json!({
                        "count": 1,
                        "value": [{"id": id, "name": name, "runnerScaleSetId": 7}]
                    })
                    .to_string()
                    .into_bytes();
                    Ok(Exchange { status: 200, body })
                } else {
                    Ok(Exchange {
                        status: 200,
                        body: br#"{"count":0,"value":[]}"#.to_vec(),
                    })
                }
            }
            Method::Delete => {
                let id = request
                    .path
                    .rsplit('/')
                    .next()
                    .and_then(|value| value.parse::<i64>().ok())
                    .ok_or(TransportFail::Reset)?;
                let name = self
                    .ids
                    .iter()
                    .find_map(|(name, runner_id)| (*runner_id == id).then_some(name.clone()))
                    .ok_or(TransportFail::Reset)?;
                self.registered
                    .lock()
                    .map_err(|_| TransportFail::Reset)?
                    .remove(&name);
                Ok(Exchange {
                    status: 204,
                    body: Vec::new(),
                })
            }
            Method::Post | Method::Patch => Err(TransportFail::Http(500)),
        }
    }
}

#[tokio::test]
async fn old_failed_rows_do_not_starve_later_cleanup() -> Result<(), String> {
    let _capacity = install_job_capacity(8);
    let (_scratch, journal) = open("completion-fair-retry").await?;
    let mut identities = Vec::new();
    for request_id in 100..105 {
        let (_, identity, _, _) = launch(&journal, 7, request_id).await?;
        let name = format!("v{}", identity.launch_id());
        let runner_id = request_id + 10;
        completion::record_completion_events(
            &journal,
            7,
            &completion_poll(request_id, runner_id, &name),
        )
        .await
        .map_err(|error| error.to_string())?;
        identities.push((name, runner_id));
    }
    let failed_names: Vec<String> = identities[..4]
        .iter()
        .map(|(name, _)| name.clone())
        .collect();
    let healthy_name = identities[4].0.clone();
    let healthy_launch_id = healthy_name
        .strip_prefix('v')
        .ok_or_else(|| "healthy runner name has no launch prefix".to_owned())?;
    let api = FairRunnerApi::new(&failed_names, &identities);
    let engine = CompletionEngine::empty();
    let mut sweeps = 0;
    while sweeps < 5 {
        sweeps += 1;
        let tasks = completion::schedule_completed_isolated(
            api.clone(),
            7,
            "admin-token",
            journal.clone(),
            engine.clone(),
        )
        .await
        .map_err(|error| error.to_string())?;
        for task in tasks {
            task.await.map_err(|error| error.to_string())?;
        }
        let rows = journal
            .completed_launches()
            .await
            .map_err(|error| error.to_string())?;
        if rows
            .iter()
            .all(|row| row.launch_id.as_deref() != Some(healthy_launch_id))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let healthy_id = identities[4].1;
    let healthy_row = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.github_runner_id.as_deref() == Some(&healthy_id.to_string()))
        .ok_or_else(|| "healthy row was not recorded".to_owned())?;
    assert!(
        healthy_row.cleanup_proven,
        "later cleanup did not run in {sweeps} sweeps"
    );
    let calls = api.calls()?;
    assert!(calls.contains(&healthy_name));
    assert_eq!(journal.occupied_launches().await, Ok(4));
    Ok(())
}
