//! Service containers started beside a job (closed catalog).
//!
//! A job may declare only catalog services. The image digest, credentials,
//! port mapping and health check are fixed here, never read from a repository,
//! so a service cannot become an arbitrary image or carry a secret. Today the
//! catalog holds `postgres`, the database that jOOQ code generation and tests
//! need; the catalog digest is refreshed by the freshness pipeline.
//!
//! The credentials, database name and host port are the ChainArgos build
//! convention (`backend/compose.yaml` of every Java repository: user,
//! password and database `chainargos`, host port 40000), so the same build
//! runs unchanged against a local compose database and the CI service.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;

/// Fixed, non-secret PostgreSQL user, password and database of the catalog service.
pub const POSTGRES_USER: &str = "chainargos";
/// Host port of the catalog PostgreSQL service (the compose default).
pub const POSTGRES_HOST_PORT: u16 = 40000;
/// Container port of PostgreSQL.
pub const POSTGRES_PORT: u16 = 5432;

/// `(major, image reference)` rows: tag-less, digest-pinned.
///
/// Digest of the `postgres:18` multi-architecture index, read from the
/// Docker Hub registry on 2026-10-20.
const POSTGRES_IMAGES: [(u8, &str); 1] = [(
    18,
    "postgres@sha256:74935e72241653ca55e0414067e6d8763aceb8a810eb51b452253ec3dcfc4336",
)];

/// A catalog service container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceContainer {
    /// Service id; also the container's network alias.
    pub id: String,
    /// Digest-pinned image reference.
    pub image: String,
    /// Fixed environment.
    pub env: BTreeMap<String, String>,
    /// `host:container` port mappings.
    pub ports: Vec<String>,
    /// Container `options` (the health check).
    pub options: String,
}

impl ServiceContainer {
    /// The catalog PostgreSQL service for a supported major version.
    ///
    /// # Errors
    /// `unsupported_postgres_major` outside the catalog.
    pub fn postgres(major: u8) -> Result<Self, ContractError> {
        let image = POSTGRES_IMAGES
            .iter()
            .find_map(|(known, image)| (*known == major).then_some(*image))
            .ok_or_else(|| {
                ContractError::identity("service.postgres", "unsupported_postgres_major")
            })?;
        Ok(Self {
            id: "postgres".to_owned(),
            image: image.to_owned(),
            env: BTreeMap::from([
                ("POSTGRES_USER".to_owned(), POSTGRES_USER.to_owned()),
                ("POSTGRES_PASSWORD".to_owned(), POSTGRES_USER.to_owned()),
                ("POSTGRES_DB".to_owned(), POSTGRES_USER.to_owned()),
            ]),
            ports: vec![format!("{POSTGRES_HOST_PORT}:{POSTGRES_PORT}")],
            options: format!(
                "--health-cmd \"pg_isready -h 127.0.0.1 -U {POSTGRES_USER} -d {POSTGRES_USER}\" --health-interval 2s --health-timeout 5s --health-retries 30"
            ),
        })
    }

    /// Catalog majors, ascending.
    #[must_use]
    pub fn postgres_majors() -> Vec<u8> {
        POSTGRES_IMAGES.iter().map(|(major, _)| *major).collect()
    }

    /// Require this service to equal its catalog entry exactly.
    ///
    /// # Errors
    /// `service_not_in_catalog` for any other id, image, env, port or option.
    pub fn validate(&self) -> Result<(), ContractError> {
        let known = POSTGRES_IMAGES
            .iter()
            .filter_map(|(major, _)| Self::postgres(*major).ok())
            .any(|catalog| &catalog == self);
        if known {
            Ok(())
        } else {
            Err(ContractError::identity(
                "job.services",
                "service_not_in_catalog",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_is_digest_pinned_credentialed_with_fixed_values_only() {
        let service = ServiceContainer::postgres(18).expect("catalog");
        assert!(service.image.starts_with("postgres@sha256:"));
        assert_eq!(service.image.len(), "postgres@sha256:".len() + 64);
        assert!(!service.image.contains(':') || service.image.contains("@sha256:"));
        assert_eq!(
            service.env.get("POSTGRES_PASSWORD").map(String::as_str),
            Some("chainargos")
        );
        assert!(service.options.contains("--health-cmd"));
        assert_eq!(service.validate(), Ok(()));
        assert_eq!(ServiceContainer::postgres_majors(), [18]);
    }

    #[test]
    fn anything_outside_the_catalog_is_rejected() {
        assert!(ServiceContainer::postgres(17).is_err());
        let mut tampered = ServiceContainer::postgres(18).expect("catalog");
        tampered.image = "evil/postgres:latest".to_owned();
        assert!(tampered.validate().is_err());
        let mut env = ServiceContainer::postgres(18).expect("catalog");
        env.env.insert("EXTRA".to_owned(), "x".to_owned());
        assert!(env.validate().is_err());
        let mut renamed = ServiceContainer::postgres(18).expect("catalog");
        renamed.id = "redis".to_owned();
        assert!(renamed.validate().is_err());
    }
}
