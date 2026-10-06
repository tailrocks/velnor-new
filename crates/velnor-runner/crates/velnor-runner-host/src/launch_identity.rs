//! Immutable identity for one durable worker launch.

use crate::error::HostError;

/// Docker and seed ownership for one launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchIdentity {
    instance_id: String,
    intent_id: i64,
    launch_id: String,
    engine_id: String,
    private_volume: String,
}

impl LaunchIdentity {
    /// Build a validated identity for journal-backed or focused staged work.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when an identity field is invalid.
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

    /// Stable journal instance identity.
    #[must_use]
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    /// Stable unique identity for this launch.
    #[must_use]
    pub fn launch_id(&self) -> &str {
        &self.launch_id
    }

    /// Journal row that owns this launch.
    #[must_use]
    pub fn intent_id(&self) -> i64 {
        self.intent_id
    }

    /// Docker engine identity bound by the journal.
    #[must_use]
    pub fn engine_id(&self) -> &str {
        &self.engine_id
    }

    /// Private volume name derived from this launch identity.
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
