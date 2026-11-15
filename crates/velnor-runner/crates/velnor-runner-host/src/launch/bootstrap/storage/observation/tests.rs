// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use bollard::container::LogOutput;
use futures_util::stream;

use super::read_stdout_line;
use crate::error::HostError;

type Frames = Vec<Result<LogOutput, bollard::errors::Error>>;

fn stderr(text: &[u8]) -> LogOutput {
    LogOutput::StdErr {
        message: text.to_vec().into(),
    }
}

async fn read(frames: Frames) -> Result<Vec<u8>, HostError> {
    let mut output = Box::pin(stream::iter(frames));
    read_stdout_line(&mut output, Instant::now() + Duration::from_secs(5)).await
}

fn failure(result: Result<Vec<u8>, HostError>) -> Result<String, Box<dyn std::error::Error>> {
    match result {
        Err(error) => Ok(error.to_string()),
        Ok(_) => Err("the helper failure was not reported".into()),
    }
}

#[tokio::test]
async fn a_helper_that_fails_reports_its_own_message() -> Result<(), Box<dyn std::error::Error>> {
    let text = failure(
        read(vec![Ok(stderr(
            b"resource probe: helper_source_revision_missing_invalid\n",
        ))])
        .await,
    )?;
    assert!(
        text.contains("helper_source_revision_missing_invalid"),
        "{text}"
    );
    Ok(())
}

#[tokio::test]
async fn a_helper_message_is_printable_and_bounded() -> Result<(), Box<dyn std::error::Error>> {
    let mut hostile = b"\x1b[31mred\x1b[0m\r\n".to_vec();
    hostile.extend(std::iter::repeat_n(b'x', 4096));
    let text = failure(read(vec![Ok(stderr(&hostile))]).await)?;
    assert!(text.chars().all(|c| !c.is_control()), "{text:?}");
    assert!(text.len() < 200, "{}", text.len());
    assert!(text.contains("red"), "{text}");
    Ok(())
}

#[tokio::test]
async fn a_record_line_on_stdout_is_returned() -> Result<(), HostError> {
    let line = read(vec![Ok(LogOutput::StdOut {
        message: b"{}\n".to_vec().into(),
    })])
    .await?;
    assert_eq!(line, b"{}\n");
    Ok(())
}
