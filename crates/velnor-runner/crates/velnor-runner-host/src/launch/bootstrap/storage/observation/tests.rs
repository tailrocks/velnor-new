use std::time::{Duration, Instant};

use bollard::container::LogOutput;
use bytes::Bytes;
use futures_util::stream;

use crate::error::HostError;

use super::{decode_line, read_stdout_line};

#[tokio::test]
async fn reads_one_bounded_stdout_line() -> Result<(), HostError> {
    let frame = LogOutput::StdOut {
        message: Bytes::from_static(b"{\"value\":1}\n"),
    };
    let mut output = Box::pin(stream::iter([Ok::<_, bollard::errors::Error>(frame)]));
    let bytes = read_stdout_line(&mut output, Instant::now() + Duration::from_secs(1)).await?;
    let value: serde_json::Value = decode_line(&bytes, 64)?;
    assert_eq!(value, serde_json::json!({"value": 1}));
    Ok(())
}

#[tokio::test]
async fn rejects_non_stdout_and_trailing_output() {
    let stderr = LogOutput::StdErr {
        message: Bytes::from_static(b"private diagnostic\n"),
    };
    let mut output = Box::pin(stream::iter([Ok::<_, bollard::errors::Error>(stderr)]));
    assert!(matches!(
        read_stdout_line(&mut output, Instant::now() + Duration::from_secs(1)).await,
        Err(HostError::Frame)
    ));

    let trailing = LogOutput::StdOut {
        message: Bytes::from_static(b"{}\nextra"),
    };
    let mut output = Box::pin(stream::iter([Ok::<_, bollard::errors::Error>(trailing)]));
    assert!(matches!(
        read_stdout_line(&mut output, Instant::now() + Duration::from_secs(1)).await,
        Err(HostError::Frame)
    ));
}

#[tokio::test]
async fn rejects_an_oversize_worker_record() {
    let frame = LogOutput::StdOut {
        message: Bytes::from(vec![b'a'; 128]),
    };
    let mut output = Box::pin(stream::iter([Ok::<_, bollard::errors::Error>(frame)]));
    assert!(matches!(
        read_stdout_line(&mut output, Instant::now() + Duration::from_secs(1)).await,
        Err(HostError::Frame)
    ));
}

#[test]
fn rejects_bytes_after_the_json_line() {
    assert!(matches!(
        decode_line::<serde_json::Value>(b"{}\nextra\n", 64),
        Err(HostError::Frame)
    ));
    assert!(matches!(
        decode_line::<serde_json::Value>(b"{ }\n", 64),
        Err(HostError::Frame)
    ));
}
