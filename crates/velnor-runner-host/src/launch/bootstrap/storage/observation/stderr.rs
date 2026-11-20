// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{pin::Pin, time::Instant};

use bollard::container::LogOutput;
use futures_util::{Stream, StreamExt};

use super::within;
use crate::error::ErrorDetail;

const STDERR_LIMIT: usize = 4096;

pub(super) async fn helper_failure<S>(
    first: &[u8],
    output: &mut Pin<Box<S>>,
    deadline: Instant,
) -> ErrorDetail
where
    S: Stream<Item = Result<LogOutput, bollard::errors::Error>> + Send + ?Sized,
{
    let mut bytes = first.to_vec();
    while !bytes.contains(&b'\n') && bytes.len() < STDERR_LIMIT {
        let Ok(Some(Ok(LogOutput::StdErr { message }))) = within(deadline, output.next()).await
        else {
            break;
        };
        bytes.extend_from_slice(&message);
    }
    ErrorDetail::from_bytes(&bytes)
}
