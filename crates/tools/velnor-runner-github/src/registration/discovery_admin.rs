//! Read-only Actions-service metadata access after one-shot credential bootstrap.

use std::fmt;

use crate::{AdminConnection, RunnerGroup, ScaleSetByName, ScaleSetFound, SessionError, WireError};

use super::discovery::DiscoveryTransport;
use super::discovery_async::{AsyncDiscoveryTransport, execute_discovery};

/// Actions-service credential restricted to the discovery GET methods below.
///
/// The secret never leaves this wrapper. It is not evidence that the selected
/// group policy applies to a repository-scoped Scale Set, and the returned
/// Scale Set view is not a runner-admission permit. The host must validate the
/// returned service URL against its fixed-origin policy and bind a bounded
/// no-redirect transport before calling either GET method.
#[must_use]
pub struct RepositoryDiscoveryAdmin {
    connection: AdminConnection,
}

impl RepositoryDiscoveryAdmin {
    pub(super) const fn new(connection: AdminConnection) -> Self {
        Self { connection }
    }

    /// Borrow the Actions service URL for host-side strict origin validation.
    /// Do not log or persist the returned URL.
    #[must_use]
    pub fn service_url(&self) -> &str {
        self.connection.expose_url()
    }

    /// Read runner group IDs and names from the current Actions service.
    ///
    /// The result is untrusted discovery metadata. It does not include or
    /// prove visibility, workflow restrictions, or repository eligibility.
    ///
    /// # Errors
    ///
    /// Returns a transport or service error without exposing the response body.
    pub fn list_runner_groups<T>(&self, transport: &mut T) -> Result<Vec<RunnerGroup>, SessionError>
    where
        T: DiscoveryTransport + ?Sized,
    {
        transport.bind_actions_service_origin(self.connection.expose_url())?;
        super::list_runner_groups(transport, self.connection.expose_token())
    }

    /// Read one explicitly named Velnor Scale Set in one explicitly named
    /// runner group. This method performs only GET; `NotFound` is distinct from
    /// transport, authorization, or malformed-response errors.
    ///
    /// A Found value is still untrusted metadata and does not prove the
    /// effective runner-group policy or authorize admission.
    ///
    /// # Errors
    ///
    /// Returns `WireError::RegistrationRejected` for an unsupported product
    /// selector or invalid group ID, or a transport/service error otherwise.
    pub fn get_existing_product_scale_set<T>(
        &self,
        transport: &mut T,
        runner_group_id: i64,
        scale_set_name: &str,
    ) -> Result<ScaleSetFound, SessionError>
    where
        T: DiscoveryTransport + ?Sized,
    {
        if runner_group_id <= 0 || !super::is_supported_product_selector(scale_set_name) {
            return Err(WireError::RegistrationRejected.into());
        }
        transport.bind_actions_service_origin(self.connection.expose_url())?;
        super::get_runner_scale_set(
            transport,
            &ScaleSetByName {
                runner_group_id,
                name: scale_set_name,
                admin_token: self.connection.expose_token(),
            },
        )
    }

    /// Asynchronously read runner-group IDs and names from the Actions service.
    ///
    /// The returned metadata is untrusted and does not prove group visibility
    /// or workflow restrictions. The transport must provide the bounded owned
    /// worker described by [`AsyncDiscoveryTransport`].
    ///
    /// # Errors
    ///
    /// Returns a secret-safe transport, service, or decoding error.
    pub async fn list_runner_groups_async<T>(
        &self,
        transport: &mut T,
    ) -> Result<Vec<RunnerGroup>, SessionError>
    where
        T: AsyncDiscoveryTransport + ?Sized,
    {
        transport.bind_actions_service_origin(self.connection.expose_url())?;
        let request = super::groups::groups_request(self.connection.expose_token())?;
        let exchange = execute_discovery(transport, request).await?;
        if exchange.status != 200 {
            return Err(super::other_status(exchange.status));
        }
        super::groups::decode_groups(&exchange.body)
    }

    /// Asynchronously read one exact existing product Scale Set by name.
    ///
    /// This performs only one GET. `NotFound` remains distinct from errors and
    /// never triggers create. A found set is untrusted discovery metadata, not
    /// an admission permit.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe validation, transport, service, or decoding error.
    pub async fn get_existing_product_scale_set_async<T>(
        &self,
        transport: &mut T,
        runner_group_id: i64,
        scale_set_name: &str,
    ) -> Result<ScaleSetFound, SessionError>
    where
        T: AsyncDiscoveryTransport + ?Sized,
    {
        if runner_group_id <= 0 || !super::is_supported_product_selector(scale_set_name) {
            return Err(WireError::RegistrationRejected.into());
        }
        transport.bind_actions_service_origin(self.connection.expose_url())?;
        let request = super::scale_set::scale_set_name_request(&ScaleSetByName {
            runner_group_id,
            name: scale_set_name,
            admin_token: self.connection.expose_token(),
        })?;
        let exchange = execute_discovery(transport, request).await?;
        if exchange.status != 200 {
            return Err(super::other_status(exchange.status));
        }
        super::scale_set::decode_page(&exchange.body, scale_set_name)
    }
}

impl fmt::Debug for RepositoryDiscoveryAdmin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RepositoryDiscoveryAdmin([redacted])")
    }
}
