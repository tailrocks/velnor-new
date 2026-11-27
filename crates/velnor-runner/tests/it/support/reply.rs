// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

pub(crate) struct Reply {
    pub(super) status: u16,
    pub(super) content_type: &'static str,
    pub(super) body: Vec<u8>,
    pub(super) header: Option<(&'static str, String)>,
    pub(super) stall: bool,
    pub(super) hangup: bool,
    pub(super) location: Option<String>,
    pub(super) half_body: bool,
}

impl Reply {
    pub(crate) fn status(status: u16) -> Self {
        Self::text(status, "")
    }

    pub(crate) fn text(status: u16, body: &str) -> Self {
        Self::bytes(status, "text/plain", body.as_bytes().to_vec())
    }

    pub(crate) fn bytes(status: u16, content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type,
            body,
            header: None,
            stall: false,
            hangup: false,
            location: None,
            half_body: false,
        }
    }

    pub(crate) fn stall() -> Self {
        Self {
            stall: true,
            ..Self::status(200)
        }
    }

    pub(crate) fn hangup() -> Self {
        Self {
            hangup: true,
            ..Self::status(200)
        }
    }

    pub(crate) fn redirect(location: &str) -> Self {
        Self {
            location: Some(location.to_owned()),
            ..Self::status(302)
        }
    }

    pub(crate) fn half_body(mut self) -> Self {
        self.half_body = true;
        self
    }

    pub(crate) fn with_header(mut self, name: &'static str, value: &str) -> Self {
        self.header = Some((name, value.to_owned()));
        self
    }

    pub(crate) fn json(status: u16, body: &Value) -> Self {
        Self::bytes(status, "application/json", body.to_string().into_bytes())
    }

    pub(crate) fn camel_cased(mut self) -> Self {
        fn rename(value: Value) -> Value {
            match value {
                Value::Object(map) => Value::Object(
                    map.into_iter()
                        .map(|(key, value)| {
                            let mut parts = key.split('_');
                            let first = parts.next().unwrap_or_default().to_owned();
                            let camel = parts.fold(first, |mut text, part| {
                                let mut chars = part.chars();
                                if let Some(head) = chars.next() {
                                    text.extend(head.to_uppercase());
                                    text.push_str(chars.as_str());
                                }
                                text
                            });
                            (camel, rename(value))
                        })
                        .collect(),
                ),
                Value::Array(items) => Value::Array(items.into_iter().map(rename).collect()),
                other => other,
            }
        }
        if let Ok(value) = serde_json::from_slice::<Value>(&self.body) {
            self.body = rename(value).to_string().into_bytes();
        }
        self
    }
}
