// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_github::Method;

use super::{TestResult, attempt, intent_states, methods, run, scale_set_json};

async fn refusal_releases_the_row(status: u16, expected: &str) -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let (outcome, transport, journal) = run(&dir, vec![(status, String::new())]).await?;
    assert_eq!(
        intent_states(&journal).await?,
        [expected],
        "status {status}: {outcome:?}"
    );
    assert_eq!(methods(&transport), [Method::Post], "status {status}");
    let (again, transport) = attempt(&journal, vec![(200, scale_set_json(true))]).await?;
    assert_eq!(
        transport.seen.is_empty(),
        expected == "uncertain",
        "status {status}: {again:?}"
    );
    Ok(())
}

#[tokio::test]
async fn status_400_releases_the_row() -> TestResult<()> {
    refusal_releases_the_row(400, "prepared").await
}

#[tokio::test]
async fn status_401_releases_the_row() -> TestResult<()> {
    refusal_releases_the_row(401, "prepared").await
}

#[tokio::test]
async fn status_403_releases_the_row() -> TestResult<()> {
    refusal_releases_the_row(403, "prepared").await
}

#[tokio::test]
async fn status_404_releases_the_row() -> TestResult<()> {
    refusal_releases_the_row(404, "prepared").await
}

#[tokio::test]
async fn status_422_releases_the_row() -> TestResult<()> {
    refusal_releases_the_row(422, "prepared").await
}

#[tokio::test]
async fn status_201_keeps_the_row_uncertain() -> TestResult<()> {
    refusal_releases_the_row(201, "uncertain").await
}

#[tokio::test]
async fn status_202_keeps_the_row_uncertain() -> TestResult<()> {
    refusal_releases_the_row(202, "uncertain").await
}

#[tokio::test]
async fn status_204_keeps_the_row_uncertain() -> TestResult<()> {
    refusal_releases_the_row(204, "uncertain").await
}

#[tokio::test]
async fn status_299_keeps_the_row_uncertain() -> TestResult<()> {
    refusal_releases_the_row(299, "uncertain").await
}

#[tokio::test]
async fn status_301_keeps_the_row_uncertain() -> TestResult<()> {
    refusal_releases_the_row(301, "uncertain").await
}

#[tokio::test]
async fn status_302_keeps_the_row_uncertain() -> TestResult<()> {
    refusal_releases_the_row(302, "uncertain").await
}

#[tokio::test]
async fn status_308_keeps_the_row_uncertain() -> TestResult<()> {
    refusal_releases_the_row(308, "uncertain").await
}

#[tokio::test]
async fn status_408_keeps_the_row_uncertain() -> TestResult<()> {
    refusal_releases_the_row(408, "uncertain").await
}

#[tokio::test]
async fn status_429_keeps_the_row_uncertain() -> TestResult<()> {
    refusal_releases_the_row(429, "uncertain").await
}

#[tokio::test]
async fn status_500_keeps_the_row_uncertain() -> TestResult<()> {
    refusal_releases_the_row(500, "uncertain").await
}

#[tokio::test]
async fn status_502_keeps_the_row_uncertain() -> TestResult<()> {
    refusal_releases_the_row(502, "uncertain").await
}

#[tokio::test]
async fn status_503_keeps_the_row_uncertain() -> TestResult<()> {
    refusal_releases_the_row(503, "uncertain").await
}

#[tokio::test]
async fn status_504_keeps_the_row_uncertain() -> TestResult<()> {
    refusal_releases_the_row(504, "uncertain").await
}
