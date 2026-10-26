// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;

pub const POSTGRES_USER: &str = "chainargos";

pub const POSTGRES_HOST_PORT: u16 = 40000;

pub const POSTGRES_PORT: u16 = 5432;

const POSTGRES_IMAGES: [(u8, &str); 1] = [(
    18,
    "postgres@sha256:74935e72241653ca55e0414067e6d8763aceb8a810eb51b452253ec3dcfc4336",
)];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceContainer {
    pub id: String,

    pub image: String,

    pub env: BTreeMap<String, String>,

    pub ports: Vec<String>,

    pub options: String,
}

impl ServiceContainer {
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

    #[must_use]
    pub fn postgres_majors() -> Vec<u8> {
        POSTGRES_IMAGES.iter().map(|(major, _)| *major).collect()
    }

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
