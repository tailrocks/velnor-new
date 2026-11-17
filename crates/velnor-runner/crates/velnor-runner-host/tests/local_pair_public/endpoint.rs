// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env::VarError;
use std::path::Path;

use bollard::{API_DEFAULT_VERSION, Docker};

use super::TestResult;

#[derive(Clone)]
pub(crate) enum Endpoint {
    Unix(String),
    Tcp(String),
}

impl Endpoint {
    pub(crate) fn selected() -> TestResult<Option<Self>> {
        match std::env::var("VELNOR_TEST_DOCKER_HOST") {
            Ok(value) => Self::parse(&value).map(Some),
            Err(VarError::NotPresent) => {
                eprintln!(
                    "SKIP Docker integration test: set VELNOR_TEST_DOCKER_HOST to an isolated test daemon"
                );
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }

    pub(crate) fn parse(value: &str) -> TestResult<Self> {
        if let Some(path) = value.strip_prefix("unix://")
            && Path::new(path).is_absolute()
            && path.len() > 1
            && !path.chars().any(char::is_control)
        {
            return Ok(Self::Unix(path.to_owned()));
        }
        if let Some(address) = value.strip_prefix("tcp://") {
            let url = reqwest::Url::parse(value)?;
            if url.host_str().is_some()
                && url.port().is_some_and(|port| port > 0)
                && url.username().is_empty()
                && url.password().is_none()
                && url.path().is_empty()
                && url.query().is_none()
                && url.fragment().is_none()
            {
                return Ok(Self::Tcp(address.to_owned()));
            }
        }
        Err("VELNOR_TEST_DOCKER_HOST must be an explicit unix:///socket or tcp://host:port endpoint".into())
    }

    pub(crate) fn docker(&self) -> TestResult<Docker> {
        Ok(match self {
            Self::Unix(path) => Docker::connect_with_unix(path, 120, API_DEFAULT_VERSION)?,
            Self::Tcp(address) => {
                Docker::connect_with_http(&format!("http://{address}"), 120, API_DEFAULT_VERSION)?
            }
        })
    }
}
