//! Read-only Actions-service metadata access after one-shot credential bootstrap.

use std::fmt;

use crate::{AdminConnection, RunnerGroup, ScaleSetByName, ScaleSetFound, SessionError, WireError};

use super::actions_service_route::ActionsServiceScaleSetRoute;
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
    repository_id: i64,
    repository_full_name: String,
}

impl RepositoryDiscoveryAdmin {
    pub(super) fn new(
        connection: AdminConnection,
        repository_id: i64,
        repository_full_name: String,
    ) -> Result<Self, SessionError> {
        repository_full_name
            .split_once('/')
            .filter(|(owner, repository)| {
                !owner.is_empty() && !repository.is_empty() && !repository.contains('/')
            })
            .ok_or(WireError::Malformed)?;
        Ok(Self {
            connection,
            repository_id,
            repository_full_name,
        })
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
        formatter
            .debug_struct("RepositoryDiscoveryAdmin")
            .field("repository_id", &self.repository_id)
            .field("repository_full_name", &self.repository_full_name)
            .field("connection", &"[redacted]")
            .finish_non_exhaustive()
    }
}

mod session_capability;
mod session_cleanup;
pub use session_capability::{
    AcquireUnresolvedReason, PoolSessionCapabilityError, PopulationObservationSource,
    SessionCloseOutcome, SessionPopulationObservation, VerifiedAcquireOutcome, VerifiedAcquiredJob,
    VerifiedAssignedDemand, VerifiedPoolSessionAdmin, VerifiedQueueSession,
};
pub use session_cleanup::{
    RepositorySessionCleanupBinding, RepositorySessionCleanupExpectation,
    RepositorySessionCleanupOutcome, RepositorySessionCleanupRoute, RepositorySessionCloseClaim,
};

/// Lookup result for an exact group and Scale Set in an organization-scoped
/// Actions Service connection. Missing objects are distinct, read-only
/// outcomes and never trigger a create fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionsServiceRouteLookup {
    /// No exact group name exists in this organization's internal group list.
    GroupNotFound,
    /// The exact internal group exists, but its filtered Set lookup is empty.
    ScaleSetNotFound,
    /// The exact group and Set were observed in this organization scope.
    Found(ActionsServiceScaleSetRoute),
}

/// Organization-scoped Actions Service credential restricted to metadata GETs.
///
/// The trusted repository is retained separately from the registration scope.
/// The secret remains private and cannot be used to create a group or Scale Set.
#[must_use]
pub struct OrganizationDiscoveryAdmin {
    connection: AdminConnection,
    organization: String,
    target_repository_id: i64,
    target_repository_full_name: String,
}

impl OrganizationDiscoveryAdmin {
    pub(in crate::registration) const fn new(
        connection: AdminConnection,
        organization: String,
        target_repository_id: i64,
        target_repository_full_name: String,
    ) -> Self {
        Self {
            connection,
            organization,
            target_repository_id,
            target_repository_full_name,
        }
    }

    /// Organization login used to establish this Actions Service scope.
    #[must_use]
    pub fn organization(&self) -> &str {
        &self.organization
    }

    /// Immutable target repository ID associated with the bootstrap.
    #[must_use]
    pub const fn target_repository_id(&self) -> i64 {
        self.target_repository_id
    }

    /// Exact target repository name associated with the bootstrap.
    #[must_use]
    pub fn target_repository_full_name(&self) -> &str {
        &self.target_repository_full_name
    }

    /// Borrow the returned Actions-service URL for host-side origin binding.
    /// Do not log or persist it.
    #[must_use]
    pub fn service_url(&self) -> &str {
        self.connection.expose_url()
    }

    /// Resolve the exact internal group name and read the exact existing Set
    /// through the returned internal group ID. This is GET-only, requires the
    /// same organization-scoped service credential for both reads, and rejects
    /// duplicate/malformed group inventories. REST numeric group IDs are never
    /// part of this route object.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe transport, service, or decoding error. An
    /// unsupported selector or malformed group name is rejected before I/O.
    pub async fn read_scale_set_route_async<T>(
        &self,
        transport: &mut T,
        exact_group_name: &str,
        exact_scale_set_name: &str,
    ) -> Result<ActionsServiceRouteLookup, SessionError>
    where
        T: AsyncDiscoveryTransport + ?Sized,
    {
        if exact_group_name.is_empty()
            || exact_group_name.len() > 256
            || exact_group_name.chars().any(char::is_control)
            || !super::is_supported_product_selector(exact_scale_set_name)
        {
            return Err(WireError::RegistrationRejected.into());
        }
        transport.bind_actions_service_origin(self.connection.expose_url())?;
        let request = super::groups::groups_request(self.connection.expose_token())?;
        let exchange = execute_discovery(transport, request).await?;
        if exchange.status != 200 {
            return Err(super::other_status(exchange.status));
        }
        let groups = super::groups::decode_groups(&exchange.body)?;
        let inventory_group_count = groups.len();
        let mut matching = groups
            .into_iter()
            .filter(|group| group.name == exact_group_name);
        let Some(group) = matching.next() else {
            return Ok(ActionsServiceRouteLookup::GroupNotFound);
        };
        if matching.next().is_some() {
            return Err(WireError::Malformed.into());
        }

        transport.bind_actions_service_origin(self.connection.expose_url())?;
        let request = super::scale_set::scale_set_name_request(&ScaleSetByName {
            runner_group_id: group.id,
            name: exact_scale_set_name,
            admin_token: self.connection.expose_token(),
        })?;
        let exchange = execute_discovery(transport, request).await?;
        if exchange.status != 200 {
            return Err(super::other_status(exchange.status));
        }
        match super::scale_set::decode_page(&exchange.body, exact_scale_set_name)? {
            ScaleSetFound::NotFound => Ok(ActionsServiceRouteLookup::ScaleSetNotFound),
            ScaleSetFound::Found(scale_set) => Ok(ActionsServiceRouteLookup::Found(
                ActionsServiceScaleSetRoute::new(
                    self.organization.clone(),
                    inventory_group_count,
                    group,
                    scale_set,
                ),
            )),
        }
    }
}

impl fmt::Debug for OrganizationDiscoveryAdmin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OrganizationDiscoveryAdmin")
            .field("organization", &self.organization)
            .field("target_repository_id", &self.target_repository_id)
            .field(
                "target_repository_full_name",
                &self.target_repository_full_name,
            )
            .field("connection", &"[redacted]")
            .finish()
    }
}
