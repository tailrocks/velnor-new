// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use velnor_runner_github::{
    AdminConnectionCall, Exchange, Method, SessionRequest, Transport, TransportFail,
    admin_connection,
    workflow_run::{RepositoryTarget, get_repository_scope},
};

use super::{TestResult, fixture::Fixture, restore_timeout::refreshes};
use crate::{
    host_controller::loop_driver::connection_poll::credentials::{
        RestoreMode, restore_credentials_from_authority,
    },
    https::HttpsTransport,
    listen::{Link, Secret, link_from_admin},
};

struct Github;

impl Transport for Github {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        let body = match (&request.method, request.path.as_str()) {
            (Method::Post, "/actions/runner-registration") => {
                serde_json::json!({"url":"https://admin.example.com","token":"admin"})
            }
            (Method::Get, "/repos/owner/repo") => {
                serde_json::json!({"id":42,"name":"repo","full_name":"owner/repo","private":true,"owner":{"login":"owner"}})
            }
            _ => return Err(TransportFail::Reset),
        };
        Ok(Exchange {
            status: 200,
            body: serde_json::to_vec(&body).map_err(|_| TransportFail::Reset)?,
        })
    }
}

fn link(fixture: &Fixture) -> TestResult<Link> {
    let admin = admin_connection(
        &mut Github,
        &AdminConnectionCall {
            config_url: "https://github.com/owner/repo",
            registration_token: "registration",
        },
    )?;
    let scope = get_repository_scope(
        &mut Github,
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
        &fixture.prepared.binding,
        &scope,
    )?)
}

#[tokio::test]
async fn restoring_credentials_for_a_deleted_set_sends_nothing_to_it() -> TestResult<()> {
    let mut fixture = Fixture::new().await?;
    fixture.bind_session().await?;
    fixture
        .journal
        .connection()
        .await?
        .execute(
            "INSERT INTO connection_scale_set_deletes (connection_id, scale_set_id, runner_group_id, state) VALUES ('conn-test-1', 7, 1, 'deleted')",
            (),
        )
        .await?;
    let link = link(&fixture)?;
    let restored = restore_credentials_from_authority(
        &mut fixture.prepared,
        RestoreMode::Polling,
        tokio::time::Instant::now() + Duration::from_secs(10),
        Ok((link, Secret::new("pat-test"), 7)),
    )
    .await;
    assert!(!restored);
    assert!(fixture.prepared.credentials.is_none());
    let sent = refreshes(&fixture).await?;
    assert!(sent.is_empty(), "{sent:?}");
    Ok(())
}
