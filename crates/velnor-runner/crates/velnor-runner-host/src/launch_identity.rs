// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchIdentity {
    instance_id: String,
    intent_id: i64,
    launch_id: String,
    engine_id: String,
    private_volume: String,
}

impl LaunchIdentity {
    pub fn new(
        instance_id: &str,
        intent_id: i64,
        launch_id: &str,
        engine_id: &str,
    ) -> Result<Self, HostError> {
        if !lower_hex_32(instance_id)
            || intent_id <= 0
            || !lower_hex_32(launch_id)
            || !engine_id_ok(engine_id)
        {
            return Err(HostError::Journal);
        }
        Ok(Self {
            instance_id: instance_id.to_owned(),
            intent_id,
            launch_id: launch_id.to_owned(),
            engine_id: engine_id.to_owned(),
            private_volume: format!("v{launch_id}"),
        })
    }

    #[must_use]
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    #[must_use]
    pub fn launch_id(&self) -> &str {
        &self.launch_id
    }

    #[must_use]
    pub fn intent_id(&self) -> i64 {
        self.intent_id
    }

    #[must_use]
    pub fn engine_id(&self) -> &str {
        &self.engine_id
    }

    #[must_use]
    pub fn private_volume(&self) -> &str {
        &self.private_volume
    }
}

fn lower_hex_32(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn engine_id_ok(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
}
