//! Durable binding between one Linux launch and its trusted logical Docker Engine.

use std::fmt::{self, Formatter};

use crate::error::HostError;
use crate::journal::Journal;

/// Validated endpoint and opaque `/info.ID` value observed from the trusted Docker endpoint.
///
/// This records the logical Engine used for a launch. It is not a cryptographic
/// attestation or proof against a cloned or misconfigured daemon endpoint.
#[derive(Clone, PartialEq, Eq)]
pub struct JournalDockerDaemonBinding {
    endpoint: String,
    engine_id: String,
}

impl fmt::Debug for JournalDockerDaemonBinding {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JournalDockerDaemonBinding")
            .field("endpoint", &"<redacted>")
            .field("engine_id", &"<redacted>")
            .finish()
    }
}

impl JournalDockerDaemonBinding {
    /// Validate one Host-observed absolute Unix socket and opaque Docker Engine ID.
    ///
    /// The Linux caller must supply the fields from the same Host observation
    /// used by its bound Docker transport; this type is not an authority proof.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for an invalid endpoint or Engine ID.
    pub fn new(endpoint: &str, engine_id: &str) -> Result<Self, HostError> {
        if !endpoint.starts_with('/')
            || endpoint.len() <= 1
            || endpoint.len() > 4096
            || endpoint.chars().any(char::is_control)
            || engine_id.trim().is_empty()
            || engine_id.len() > 1024
            || engine_id.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(HostError::Journal);
        }
        Ok(Self {
            endpoint: endpoint.to_owned(),
            engine_id: engine_id.to_owned(),
        })
    }

    /// The exact validated absolute Unix socket path.
    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// The exact opaque Engine ID returned by Docker `/info`.
    #[must_use]
    pub fn engine_id(&self) -> &str {
        &self.engine_id
    }
}

impl Journal {
    /// Read the durable logical-Engine binding for one launch row.
    ///
    /// A missing value denotes a legacy or otherwise unbound row and must stay
    /// unresolved; it does not authorize probing an arbitrary current daemon.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the row is not a launch or storage is invalid.
    pub async fn launch_daemon_binding(
        &self,
        launch_id: i64,
    ) -> Result<Option<JournalDockerDaemonBinding>, HostError> {
        if launch_id <= 0 {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT b.endpoint, b.engine_id FROM intents AS i LEFT JOIN linux_launch_daemon_bindings AS b ON b.launch_id = i.id WHERE i.id = ?1 AND i.kind = 'launch'",
                [launch_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
            return Err(HostError::Journal);
        };
        let endpoint = row
            .get::<Option<String>>(0)
            .map_err(|_| HostError::Journal)?;
        let engine_id = row
            .get::<Option<String>>(1)
            .map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
            return Err(HostError::Journal);
        }
        match (endpoint, engine_id) {
            (None, None) => Ok(None),
            (Some(endpoint), Some(engine_id)) => {
                JournalDockerDaemonBinding::new(&endpoint, &engine_id).map(Some)
            }
            _ => Err(HostError::Journal),
        }
    }
}
