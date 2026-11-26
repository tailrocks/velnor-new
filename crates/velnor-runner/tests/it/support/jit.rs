// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::{Generate, PrivateKey};
use serde_json::{Value, json};

fn der_read(input: &[u8]) -> Result<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first().context("DER tag")?;
    let (&first, rest) = rest.split_first().context("DER length")?;
    let (length, rest) = if first & 0x80 == 0 {
        (usize::from(first), rest)
    } else {
        let count = usize::from(first & 0x7f);
        if rest.len() < count {
            bail!("DER length is cut short");
        }
        let (digits, rest) = rest.split_at(count);
        (
            digits
                .iter()
                .fold(0_usize, |length, byte| (length << 8) | usize::from(*byte)),
            rest,
        )
    };
    if rest.len() < length {
        bail!("DER content is cut short");
    }
    let (content, rest) = rest.split_at(length);
    Ok((tag, content, rest))
}

fn rsa_parameters() -> Result<Value> {
    let key = PrivateKey::generate();
    let document = key.to_pkcs1_der().context("encode the test key")?;
    let (_, mut rest, _) = der_read(document.as_bytes())?;
    let mut integers = Vec::new();
    while !rest.is_empty() {
        let (_, content, remainder) = der_read(rest)?;
        integers.push(STANDARD.encode(content));
        rest = remainder;
    }
    let [_version, modulus, exponent, d, p, q, dp, dq, inverse_q] = integers
        .try_into()
        .map_err(|_| anyhow::anyhow!("the test key is not a PKCS#1 key"))?;
    Ok(json!({
        "modulus": modulus,
        "exponent": exponent,
        "d": d,
        "p": p,
        "q": q,
        "dp": dp,
        "dq": dq,
        "inverseQ": inverse_q,
    }))
}

pub(super) fn jit_config(base: &str) -> Result<String> {
    let files = json!({
        ".runner": json!({
            "AgentId": "42",
            "AgentName": "velnor-test",
            "PoolId": "1",
            "Ephemeral": "True",
            "DisableUpdate": "True",
            "ServerUrlV2": format!("{base}/broker"),
            "UseV2Flow": "true",
        }),
        ".credentials": json!({
            "Scheme": "OAuth",
            "Data": {
                "ClientId": "client-1",
                "AuthorizationUrl": format!("{base}/oauth/token"),
            },
        }),
        ".credentials_rsaparams": rsa_parameters()?,
    });
    let encoded: serde_json::Map<String, Value> = files
        .as_object()
        .context("JIT files")?
        .iter()
        .map(|(name, content)| {
            (
                name.clone(),
                Value::String(STANDARD.encode(content.to_string())),
            )
        })
        .collect();
    Ok(STANDARD.encode(Value::Object(encoded).to_string()))
}
