// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;

use super::lifecycle_support::{fixture_job, run};
use super::support::mock_github::{Behavior, JOB_LOG_BLOB, MockGithub, STEP_LOG_BLOB, step_id};

const STAMP_LEN: usize = "2026-11-27T00:00:00.0000000Z ".len();

fn assert_stamped(line: &str, text: &str) {
    let (stamp, rest) = line.split_at_checked(STAMP_LEN).unwrap_or((line, ""));
    assert_eq!(rest, text, "line {line:?}");
    for (index, byte) in stamp.bytes().enumerate() {
        let expected = match index {
            4 | 7 => Some(b'-'),
            10 => Some(b'T'),
            13 | 16 => Some(b':'),
            19 => Some(b'.'),
            27 => Some(b'Z'),
            28 => Some(b' '),
            _ => None,
        };
        match expected {
            Some(expected) => assert_eq!(byte, expected, "stamp {stamp:?} at {index}"),
            None => assert!(byte.is_ascii_digit(), "stamp {stamp:?} at {index}"),
        }
    }
}

fn texts(log: &str) -> Vec<&str> {
    log.lines()
        .map(|line| line.get(STAMP_LEN..).unwrap_or_default())
        .collect()
}

fn uploaded(mock: &MockGithub, blob: &str) -> Result<String> {
    Ok(mock
        .requests_to("PUT", blob)?
        .first()
        .with_context(|| format!("{blob} was uploaded"))?
        .text())
}

fn frame_lines(frames: &[Value]) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    for frame in frames {
        assert_eq!(frame["stepId"], step_id(1));
        let value = frame["value"].as_array().context("frame lines")?;
        assert_eq!(frame["count"], value.len());
        assert_eq!(
            frame["startLine"],
            lines.len() + 1,
            "startLine continues the step"
        );
        for line in value {
            lines.push(line.as_str().context("frame line")?.to_owned());
        }
    }
    Ok(lines)
}

#[tokio::test]
async fn lifecycle_step_and_job_logs_use_seven_digit_timestamps() -> Result<()> {
    let mock = MockGithub::start(
        fixture_job("hello", Some("echo alpha; for i in $(seq 1 40); do echo \"tick $i\"; sleep 0.03; done; echo omega"))?,
        Behavior::default(),
    )
    .await?;

    run(&mock, Duration::from_millis(200)).await?;

    let expected: Vec<String> = std::iter::once("alpha".to_owned())
        .chain((1..=40).map(|number| format!("tick {number}")))
        .chain(std::iter::once("omega".to_owned()))
        .collect();
    for blob in [STEP_LOG_BLOB, JOB_LOG_BLOB] {
        let log = uploaded(&mock, blob)?;
        let lines: Vec<&str> = log.lines().collect();
        let first = lines
            .iter()
            .position(|line| line.ends_with(" alpha"))
            .with_context(|| format!("{blob} has alpha"))?;
        let ours = &lines[first..first + expected.len()];
        for (line, text) in ours.iter().zip(&expected) {
            assert_stamped(line, text);
        }
        assert!(log.ends_with('\n'));
    }
    Ok(())
}

#[tokio::test]
async fn lifecycle_streams_step_lines_to_the_live_feed() -> Result<()> {
    let behavior = Behavior {
        feed: true,
        ..Behavior::default()
    };
    let mock = MockGithub::start(
        fixture_job("hello", Some("echo one; sleep 1; echo two"))?,
        behavior,
    )
    .await?;

    run(&mock, Duration::from_millis(200)).await?;

    let frames = mock.feed.as_ref().context("feed")?.frames().await?;
    assert!(frames.len() >= 2, "{frames:?}");
    let lines = frame_lines(&frames)?;
    let one = lines.iter().position(|line| line == "one").context("one")?;
    assert_eq!(lines[one + 1], "two");
    let step_log = uploaded(&mock, STEP_LOG_BLOB)?;
    assert_eq!(lines, texts(&step_log), "the feed shows the logged lines");
    Ok(())
}

#[tokio::test]
async fn lifecycle_keeps_every_line_of_a_step_that_prints_far_more_than_the_buffer() -> Result<()> {
    const LINES: usize = 200_000;
    let behavior = Behavior {
        feed: true,
        ..Behavior::default()
    };
    let script = format!("seq 1 {LINES}");
    let mock = MockGithub::start(fixture_job("hello", Some(&script))?, behavior).await?;

    run(&mock, Duration::from_millis(200)).await?;

    let step_log = uploaded(&mock, STEP_LOG_BLOB)?;
    let job_log = uploaded(&mock, JOB_LOG_BLOB)?;
    let numbers = |log: &str| -> Vec<usize> {
        texts(log)
            .into_iter()
            .filter_map(|text| text.parse().ok())
            .collect()
    };
    let expected: Vec<usize> = (1..=LINES).collect();
    assert_eq!(numbers(&step_log), expected);
    assert_eq!(numbers(&job_log), expected);
    let frames = mock.feed.as_ref().context("feed")?.frames().await?;
    assert!(frames.len() > 100, "{} frames", frames.len());
    let lines = frame_lines(&frames)?;
    assert_eq!(lines, texts(&step_log), "the feed lost no line");
    Ok(())
}
