// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use ring::signature::RsaKeyPair;
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub struct DecodedJitConfig {
    pub settings: DecodedJitRunnerSettings,
    pub credentials: DecodedJitCredentials,
    pub private_key_pem: String,
}

impl fmt::Debug for DecodedJitConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DecodedJitConfig")
            .field("settings", &self.settings)
            .field("credentials", &"<redacted>")
            .field("private_key_pem", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodedJitRunnerSettings {
    #[serde(default, alias = "AgentId", deserialize_with = "lenient_i64")]
    pub agent_id: Option<i64>,
    #[serde(default, alias = "AgentName")]
    pub agent_name: Option<String>,
    #[serde(default, alias = "PoolId", deserialize_with = "lenient_i64")]
    pub pool_id: Option<i64>,
    #[serde(default, alias = "PoolName")]
    pub pool_name: Option<String>,
    #[serde(default, alias = "ServerUrl")]
    pub server_url: Option<String>,
    #[serde(default, alias = "ServerUrlV2")]
    pub server_url_v2: Option<String>,
    #[serde(default, rename = "gitHubUrl", alias = "GitHubUrl")]
    pub github_url: Option<String>,
    #[serde(default, alias = "WorkFolder")]
    pub work_folder: Option<String>,
    #[serde(default, alias = "UseV2Flow", deserialize_with = "lenient_bool")]
    pub use_v2_flow: bool,
    #[serde(default, alias = "Ephemeral", deserialize_with = "lenient_bool")]
    pub ephemeral: bool,
    #[serde(default, alias = "DisableUpdate", deserialize_with = "lenient_bool")]
    pub disable_update: bool,
    #[serde(
        default,
        rename = "skipSessionRecover",
        alias = "SkipSessionRecover",
        deserialize_with = "session_recovery"
    )]
    pub session_recovery: SessionRecovery,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SessionRecovery {
    #[default]
    Recreate,
    Skip,
}

fn session_recovery<'de, D>(deserializer: D) -> Result<SessionRecovery, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(if lenient_bool(deserializer)? {
        SessionRecovery::Skip
    } else {
        SessionRecovery::Recreate
    })
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Lenient<T> {
    Typed(T),
    Text(String),
}

fn lenient_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<Lenient<i64>>::deserialize(deserializer)?
        .map(|value| match value {
            Lenient::Typed(number) => Ok(number),
            Lenient::Text(text) => text.trim().parse().map_err(serde::de::Error::custom),
        })
        .transpose()
}

fn lenient_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(match Option::<Lenient<bool>>::deserialize(deserializer)? {
        Some(Lenient::Typed(flag)) => flag,
        Some(Lenient::Text(text)) => text.trim().eq_ignore_ascii_case("true"),
        None => false,
    })
}

#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct DecodedJitCredentials {
    #[serde(rename = "Scheme", alias = "scheme")]
    pub scheme: String,
    #[serde(rename = "Data", alias = "data")]
    pub data: BTreeMap<String, String>,
}

impl fmt::Debug for DecodedJitCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DecodedJitCredentials")
            .field("scheme", &self.scheme)
            .field("data", &"<redacted>")
            .finish()
    }
}

pub fn decode_jit_config(encoded_jit_config: &str) -> Result<DecodedJitConfig> {
    let decoded = STANDARD
        .decode(encoded_jit_config)
        .context("decode encoded_jit_config")?;
    let file_map: BTreeMap<String, String> =
        serde_json::from_slice(&decoded).context("parse encoded_jit_config file map")?;
    let settings = decode_jit_file(&file_map, ".runner")?;
    let credentials = decode_jit_file(&file_map, ".credentials")?;
    let rsa_params = decode_jit_file_bytes(&file_map, ".credentials_rsaparams")?;
    let private_key_pem = rsa_parameters_json_to_pem(&rsa_params)?;
    Ok(DecodedJitConfig {
        settings,
        credentials,
        private_key_pem,
    })
}

fn decode_jit_file<T>(file_map: &BTreeMap<String, String>, name: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = decode_jit_file_bytes(file_map, name)?;
    serde_json::from_slice(&bytes).with_context(|| format!("parse JIT config file {name}"))
}

fn decode_jit_file_bytes(file_map: &BTreeMap<String, String>, name: &str) -> Result<Vec<u8>> {
    let encoded = file_map
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("encoded_jit_config missing {name}"))?;
    STANDARD
        .decode(encoded)
        .with_context(|| format!("decode JIT config file {name}"))
}

const PEM_BEGIN: &str = "-----BEGIN RSA PRIVATE KEY-----";
const PEM_END: &str = "-----END RSA PRIVATE KEY-----";
const RSA_PARAMETER_ORDER: [&str; 8] =
    ["modulus", "exponent", "d", "p", "q", "dp", "dq", "inverseQ"];

pub fn rsa_parameters_json_to_pem(json_bytes: &[u8]) -> Result<String> {
    let fields: BTreeMap<String, String> =
        serde_json::from_slice(json_bytes).context("parse JIT RSA parameters")?;
    let mut content = der_integer(&[0]);
    for name in RSA_PARAMETER_ORDER {
        let encoded = fields
            .get(name)
            .with_context(|| format!("JIT RSA parameters missing {name}"))?;
        let magnitude = STANDARD
            .decode(encoded)
            .with_context(|| format!("decode RSA parameter {name}"))?;
        content.extend(der_integer(&magnitude));
    }
    let der = der_tlv(0x30, &content);
    RsaKeyPair::from_der(&der).map_err(|error| anyhow::anyhow!("invalid JIT RSA key: {error}"))?;
    let body = STANDARD.encode(&der);
    let mut pem = String::with_capacity(body.len() + body.len() / 64 + 64);
    pem.push_str(PEM_BEGIN);
    pem.push('\n');
    for line in body.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(line).context("encode PEM line")?);
        pem.push('\n');
    }
    pem.push_str(PEM_END);
    pem.push('\n');
    Ok(pem)
}

pub(crate) fn private_key_der_from_pem(pem: &str) -> Result<Vec<u8>> {
    let body: String = pem
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("-----"))
        .collect();
    STANDARD.decode(body).context("decode RSA private key PEM")
}

fn der_integer(magnitude: &[u8]) -> Vec<u8> {
    let start = magnitude
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(magnitude.len().saturating_sub(1));
    let trimmed = &magnitude[start..];
    let mut content = Vec::with_capacity(trimmed.len() + 1);
    if trimmed.first().is_none_or(|byte| byte & 0x80 != 0) {
        content.push(0);
    }
    content.extend_from_slice(trimmed);
    der_tlv(0x02, &content)
}

fn der_tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut encoded = vec![tag];
    let length = content.len();
    if length < 0x80 {
        encoded.push(u8::try_from(length).unwrap_or(0));
    } else {
        let bytes = length.to_be_bytes();
        let significant: Vec<u8> = bytes
            .iter()
            .copied()
            .skip_while(|byte| *byte == 0)
            .collect();
        encoded.push(0x80 | u8::try_from(significant.len()).unwrap_or(0));
        encoded.extend(significant);
    }
    encoded.extend_from_slice(content);
    encoded
}
