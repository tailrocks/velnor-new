// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_image_manifest::strict_json;

const MAX_MESSAGE: usize = 64 * 1024;
const MAX_MESSAGES: usize = 16_384;

#[derive(Debug)]
pub(super) struct DecodeError;

pub(super) struct PullDecoder<'a> {
    digest: &'a str,
    reference: &'a str,
    pending: Vec<u8>,
    messages: usize,
    digest_seen: bool,
    terminal_seen: bool,
}

impl<'a> PullDecoder<'a> {
    pub(super) fn new(digest: &'a str, reference: &'a str) -> Self {
        Self {
            digest,
            reference,
            pending: Vec::new(),
            messages: 0,
            digest_seen: false,
            terminal_seen: false,
        }
    }

    pub(super) fn push(&mut self, chunk: &[u8]) -> Result<(), DecodeError> {
        for part in chunk.split_inclusive(|byte| *byte == b'\n') {
            let complete = part.last() == Some(&b'\n');
            let content = if complete {
                &part[..part.len() - 1]
            } else {
                part
            };
            if self
                .pending
                .len()
                .checked_add(content.len())
                .is_none_or(|size| size > MAX_MESSAGE)
            {
                return Err(DecodeError);
            }
            self.pending.extend_from_slice(content);
            if complete {
                let line = std::mem::take(&mut self.pending);
                self.line(&line)?;
            }
        }
        Ok(())
    }

    pub(super) fn finish(mut self) -> Result<(), DecodeError> {
        if !self.pending.is_empty() {
            let line = std::mem::take(&mut self.pending);
            self.line(&line)?;
        }
        if !self.terminal_seen {
            return Err(DecodeError);
        }
        Ok(())
    }

    fn line(&mut self, bytes: &[u8]) -> Result<(), DecodeError> {
        if bytes.iter().all(u8::is_ascii_whitespace) {
            self.messages = self
                .messages
                .checked_add(1)
                .filter(|count| *count <= MAX_MESSAGES)
                .ok_or(DecodeError)?;
            return Ok(());
        }
        validate_message(
            bytes,
            &mut self.messages,
            self.digest,
            self.reference,
            &mut self.digest_seen,
            &mut self.terminal_seen,
        )
    }
}

fn validate_message(
    bytes: &[u8],
    messages: &mut usize,
    digest: &str,
    reference: &str,
    digest_seen: &mut bool,
    terminal_seen: &mut bool,
) -> Result<(), DecodeError> {
    *messages = messages
        .checked_add(1)
        .filter(|count| *count <= MAX_MESSAGES)
        .ok_or(DecodeError)?;
    let value =
        strict_json::parse_strict_json_bytes(bytes, MAX_MESSAGE + 1).map_err(|_| DecodeError)?;
    let object = value.as_object().ok_or(DecodeError)?;
    if object.get("error").is_some_and(|value| !value.is_null())
        || object
            .get("errorDetail")
            .is_some_and(|value| !value.is_null())
    {
        return Err(DecodeError);
    }
    if object
        .get("status")
        .is_some_and(|value| !value.is_null() && !value.is_string())
        || object
            .get("id")
            .is_some_and(|value| !value.is_null() && !value.is_string())
    {
        return Err(DecodeError);
    }
    if !["status", "id"].iter().any(|key| {
        object
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    }) {
        return Err(DecodeError);
    }
    if *terminal_seen {
        return Err(DecodeError);
    }
    if let Some(status) = object.get("status").and_then(serde_json::Value::as_str) {
        if let Some(observed) = status.strip_prefix("Digest: ") {
            if *digest_seen || observed != digest {
                return Err(DecodeError);
            }
            *digest_seen = true;
        } else if let Some(observed) = status
            .strip_prefix("Status: Downloaded newer image for ")
            .or_else(|| status.strip_prefix("Status: Image is up to date for "))
        {
            if !*digest_seen || observed != reference {
                return Err(DecodeError);
            }
            *terminal_seen = true;
        } else if *digest_seen {
            return Err(DecodeError);
        }
    } else if *digest_seen {
        return Err(DecodeError);
    }
    Ok(())
}
