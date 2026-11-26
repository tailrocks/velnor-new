// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Method;
use url::Url;
use velnor_runner::protocol::{BrokerClient, TwirpResultsClient, github_json_http_response};

use super::support::{Reply, TOKEN, TestServer, immediate_run_service};

const PASSWORD: &str = "sentinel-userinfo-password";

#[tokio::test]
async fn authenticated_requests_refuse_unsafe_urls_before_sending() -> Result<()> {
    let server = TestServer::start().await?;
    let with_userinfo = |base: &str| -> Result<String> {
        let mut url = Url::parse(base)?;
        url.set_username("user")
            .map_err(|()| anyhow::anyhow!("set username"))?;
        url.set_password(Some(PASSWORD))
            .map_err(|()| anyhow::anyhow!("set password"))?;
        Ok(url.to_string())
    };

    for (name, url, expected) in [
        (
            "plain http off loopback",
            "http://broker.example.test/echo".to_owned(),
            "must use HTTPS",
        ),
        (
            "unsupported scheme",
            "ftp://broker.example.test/echo".to_owned(),
            "must use HTTPS",
        ),
        (
            "userinfo on https",
            with_userinfo("https://broker.example.test/echo")?,
            "userinfo",
        ),
        (
            "userinfo on loopback",
            with_userinfo(&format!("{}/echo", server.uri()))?,
            "userinfo",
        ),
    ] {
        let error =
            github_json_http_response(Method::GET, &url, TOKEN, None, Duration::from_secs(5))
                .await
                .err()
                .with_context(|| format!("{name}: must be refused"))?;

        let rendered = format!("{error:#}");
        assert!(rendered.contains(expected), "{name}: {rendered}");
        assert!(!rendered.contains(PASSWORD), "{name}: {rendered}");
    }
    assert!(server.requests()?.is_empty());
    Ok(())
}

#[tokio::test]
async fn service_endpoints_accept_only_approved_hosts_on_the_default_port() -> Result<()> {
    let run_service = immediate_run_service()?;

    for (name, url, expected) in [
        (
            "explicit non-default port",
            "https://run.actions.githubusercontent.com:8443/jobs/1",
            "not an approved GitHub Actions service",
        ),
        (
            "unapproved host",
            "https://evil.example.test/jobs/1",
            "not an approved GitHub Actions service",
        ),
        (
            "query data",
            "https://run.actions.githubusercontent.com/jobs/1?next=elsewhere",
            "must not contain query or fragment data",
        ),
        (
            "plain http",
            "http://run.actions.githubusercontent.com/jobs/1",
            "must use HTTPS",
        ),
    ] {
        let error = run_service
            .acquire_job(url, "broker-message", "Linux", None)
            .await
            .err()
            .with_context(|| format!("{name}: must be refused"))?;

        let rendered = format!("{error:#}");
        assert!(rendered.contains(expected), "{name}: {rendered}");
    }

    let results_error = TwirpResultsClient::new(
        "https://results-receiver.actions.githubusercontent.com:8443/",
        TOKEN,
    )
    .err()
    .context("results endpoint on a non-default port must be refused")?;
    assert!(format!("{results_error:#}").contains("not an approved"));

    let broker_error = BrokerClient::new("http://broker.example.test/", TOKEN)
        .err()
        .context("plain http broker endpoint must be refused")?;
    assert!(format!("{broker_error:#}").contains("must use HTTPS"));
    Ok(())
}

#[tokio::test]
async fn authenticated_requests_do_not_follow_redirects() -> Result<()> {
    let server = TestServer::start().await?;
    let leak = format!("{}/leak", server.uri());
    server.respond("GET", "/moved", move |_| Reply::redirect(&leak))?;
    server.respond("GET", "/leak", |_| Reply::status(200))?;

    let response = github_json_http_response(
        Method::GET,
        &format!("{}/moved", server.uri()),
        TOKEN,
        None,
        Duration::from_secs(5),
    )
    .await?;

    assert_eq!(response.status, 302);
    assert_eq!(server.requests_to("GET", "/moved")?.len(), 1);
    assert!(server.requests_to("GET", "/leak")?.is_empty());
    Ok(())
}
