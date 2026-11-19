// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::Path;

use serde::Deserialize;

use crate::error::HostError;

use super::storage::Sizes;

pub const MARKER: &str = "velnor-local";
const MAX_PLAN_BYTES: u64 = 64 * 1024;
const MAX_PAIRS_PER_SIDE: u32 = 64;
const FORBIDDEN_STATE_FILES: [&str; 4] =
    ["host.toml", "daemon.lock", "admission.lock", "launch.db"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Trusted,
    Untrusted,
}

impl Side {
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        [Self::Trusted, Self::Untrusted]
            .into_iter()
            .find(|side| side.name() == name)
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Trusted => "trusted",
            Self::Untrusted => "untrusted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopbackAuthority(SocketAddr);

impl LoopbackAuthority {
    pub fn parse(text: &str) -> Result<Self, HostError> {
        let host = text.strip_prefix("http://").ok_or(HostError::Config)?;
        let address: SocketAddr = host.parse().map_err(|_| HostError::Config)?;
        let loopback = match address.ip() {
            IpAddr::V4(ip) => ip == Ipv4Addr::LOCALHOST,
            IpAddr::V6(ip) => ip == Ipv6Addr::LOCALHOST,
        };
        if !loopback || address.port() == 0 {
            return Err(HostError::Config);
        }
        Ok(Self(address))
    }

    #[must_use]
    pub const fn address(self) -> SocketAddr {
        self.0
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub runner_cpu_millicores: u64,
    pub runner_memory_bytes: u64,
    pub dind_cpu_millicores: u64,
    pub dind_memory_bytes: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidePlan {
    pub side: Side,
    pub network: String,
    pub pairs: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    environment: String,
    authority: String,
    token: String,
    docker: String,
    platform: String,
    runner_image: String,
    dind_image: String,
    limits: Limits,
    #[serde(default)]
    storage: Option<Sizes>,
    sides: Vec<SidePlan>,
    #[serde(default)]
    policy: Option<String>,
}

pub struct Plan {
    pub environment: String,
    pub authority: LoopbackAuthority,
    pub token: String,
    pub docker: String,
    pub platform: String,
    pub runner_image: String,
    pub dind_image: String,
    pub limits: Limits,
    pub storage: Sizes,
    pub sides: Vec<SidePlan>,
    pub policy: Option<String>,
}

impl std::fmt::Debug for Plan {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Plan")
            .field("environment", &self.environment)
            .field("authority", &self.authority)
            .field("sides", &self.sides)
            .finish_non_exhaustive()
    }
}

impl Plan {
    pub fn read(input: impl Read) -> Result<Self, HostError> {
        let mut bytes = Vec::new();
        input
            .take(MAX_PLAN_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| HostError::Config)?;
        if bytes.len() as u64 > MAX_PLAN_BYTES {
            return Err(HostError::Config);
        }
        Self::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, HostError> {
        let raw: Raw = serde_json::from_slice(bytes).map_err(|_| HostError::Config)?;
        let authority = LoopbackAuthority::parse(&raw.authority)?;
        let token_plain = raw.token.bytes().all(|byte| byte.is_ascii_alphanumeric());
        let sides_valid = !raw.sides.is_empty()
            && raw.sides.iter().enumerate().all(|(index, side)| {
                side.pairs <= MAX_PAIRS_PER_SIDE
                    && raw.sides[..index]
                        .iter()
                        .all(|seen| seen.side != side.side && seen.network != side.network)
            })
            && raw.sides.iter().any(|side| side.pairs > 0);
        if !(32..=128).contains(&raw.token.len()) || !token_plain || !sides_valid {
            return Err(HostError::Config);
        }
        Ok(Self {
            environment: raw.environment,
            authority,
            token: raw.token,
            docker: raw.docker,
            platform: raw.platform,
            runner_image: raw.runner_image,
            dind_image: raw.dind_image,
            limits: raw.limits,
            storage: raw.storage.unwrap_or_default().checked()?,
            sides: raw.sides,
            policy: raw.policy,
        })
    }
}

pub fn check_state(state: &Path) -> Result<(), HostError> {
    let marker = fs::symlink_metadata(state.join(MARKER)).map_err(|_| HostError::Config)?;
    if !marker.is_file() {
        return Err(HostError::Config);
    }
    for entry in fs::read_dir(state).map_err(|_| HostError::Config)? {
        let name = entry.map_err(|_| HostError::Config)?.file_name();
        let name = name.to_string_lossy();
        let journal = name.starts_with("launch.db") || name.ends_with(".db");
        if journal || FORBIDDEN_STATE_FILES.contains(&name.as_ref()) {
            return Err(HostError::Config);
        }
    }
    Ok(())
}
