// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ring::signature::{RSA_PKCS1_2048_8192_SHA256, RsaKeyPair, UnparsedPublicKey};
use serde_json::{Value, json};
use velnor_runner::protocol::oauth::build_client_assertion;
use velnor_runner::protocol::{
    DecodedJitConfig, OAuthClient, OAuthJwtCredentials, OAuthRegistrationNotFound,
    decode_jit_config,
};

use super::support::{Scripted, TestServer, scripted};

const TEST_KEY_PEM: &str =
    include_str!("../../../velnor-ci/tests/it/data/app_token/TEST-ONLY-not-a-secret-rsa-pkcs1.pem");
const CLIENT_ID: &str = "00000000-0000-0000-0000-0000000000aa";
const TOKEN_PATH: &str = "/oauth/token";
const GRANT_TYPE: &str = "urn:ietf:params:oauth:client-assertion-type:jwt-bearer";

fn test_key_der() -> Result<Vec<u8>> {
    let body: String = TEST_KEY_PEM
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    STANDARD.decode(body).context("decode test key PEM")
}

fn read_tlv(input: &[u8]) -> Result<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first().context("DER tag")?;
    let (&first, rest) = rest.split_first().context("DER length")?;
    let (length, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let count = usize::from(first & 0x7f);
        let (digits, rest) = rest.split_at_checked(count).context("DER long length")?;
        let length = digits
            .iter()
            .fold(0_usize, |acc, byte| (acc << 8) | usize::from(*byte));
        (length, rest)
    };
    let (content, rest) = rest.split_at_checked(length).context("DER content")?;
    Ok((tag, content, rest))
}

fn rsa_components(der: &[u8]) -> Result<Vec<Vec<u8>>> {
    let (_, mut sequence, _) = read_tlv(der)?;
    let mut components = Vec::new();
    while !sequence.is_empty() {
        let (tag, content, rest) = read_tlv(sequence)?;
        assert_eq!(tag, 0x02, "RSAPrivateKey holds only INTEGERs");
        components.push(content.to_vec());
        sequence = rest;
    }
    Ok(components)
}

fn encode_file(value: &Value) -> String {
    STANDARD.encode(value.to_string())
}

fn encoded_jit_config(authorization_url: &str) -> Result<String> {
    let components = rsa_components(&test_key_der()?)?;
    let [_version, modulus, exponent, d, p, q, dp, dq, inverse_q] = components.as_slice() else {
        bail!("unexpected RSAPrivateKey layout");
    };
    let params = json!({
        "modulus": STANDARD.encode(modulus),
        "exponent": STANDARD.encode(exponent),
        "d": STANDARD.encode(d),
        "p": STANDARD.encode(p),
        "q": STANDARD.encode(q),
        "dp": STANDARD.encode(dp),
        "dq": STANDARD.encode(dq),
        "inverseQ": STANDARD.encode(inverse_q),
    });
    let files: BTreeMap<&str, String> = BTreeMap::from([
        (
            ".runner",
            encode_file(&json!({
                "agentId": 7,
                "agentName": "velnor-test",
                "poolId": 1,
                "serverUrl": "https://pipelines.actions.githubusercontent.com/abc",
                "serverUrlV2": "https://broker.actions.githubusercontent.com/",
                "gitHubUrl": "https://github.com/example/repo",
                "workFolder": "_work",
                "useV2Flow": true,
                "ephemeral": true,
                "disableUpdate": true
            })),
        ),
        (
            ".credentials",
            encode_file(&json!({
                "Scheme": "OAuth",
                "Data": {"clientId": CLIENT_ID, "authorizationUrl": authorization_url}
            })),
        ),
        (".credentials_rsaparams", encode_file(&params)),
    ]);
    Ok(STANDARD.encode(serde_json::to_string(&files)?))
}

fn decoded_config(authorization_url: &str) -> Result<DecodedJitConfig> {
    decode_jit_config(&encoded_jit_config(authorization_url)?)
}

fn oauth_credentials(config: &DecodedJitConfig) -> Result<OAuthJwtCredentials> {
    let data = &config.credentials.data;
    Ok(OAuthJwtCredentials {
        client_id: data.get("clientId").context("clientId")?.clone(),
        authorization_url: data
            .get("authorizationUrl")
            .context("authorizationUrl")?
            .clone(),
        private_key_pem: config.private_key_pem.clone(),
    })
}

fn verify_rs256(assertion: &str) -> Result<Value> {
    let (signing_input, signature) = assertion.rsplit_once('.').context("JWT has a signature")?;
    let public_key = RsaKeyPair::from_der(&test_key_der()?)
        .map_err(|error| anyhow::anyhow!("load test key: {error}"))?
        .public()
        .as_ref()
        .to_vec();
    UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, public_key)
        .verify(
            signing_input.as_bytes(),
            &URL_SAFE_NO_PAD.decode(signature).context("signature")?,
        )
        .map_err(|error| anyhow::anyhow!("RS256 verification failed: {error}"))?;
    let (header, claims) = signing_input.split_once('.').context("JWT segments")?;
    let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header)?)?;
    assert_eq!(header, json!({"alg": "RS256", "typ": "JWT"}));
    Ok(serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims)?)?)
}

#[test]
fn decoded_jit_config_signs_a_verifiable_rs256_client_assertion() -> Result<()> {
    let authorization_url = "https://token.actions.githubusercontent.com/oauth/token";
    let config = decoded_config(authorization_url)?;

    assert_eq!(config.settings.agent_id, Some(7));
    assert_eq!(config.settings.agent_name.as_deref(), Some("velnor-test"));
    assert_eq!(config.settings.pool_id, Some(1));
    assert_eq!(
        config.settings.server_url_v2.as_deref(),
        Some("https://broker.actions.githubusercontent.com/")
    );
    assert_eq!(
        config.settings.github_url.as_deref(),
        Some("https://github.com/example/repo")
    );
    assert!(config.settings.use_v2_flow && config.settings.ephemeral);
    assert_eq!(config.credentials.scheme, "OAuth");
    assert!(
        config
            .private_key_pem
            .starts_with("-----BEGIN RSA PRIVATE KEY-----")
    );

    let assertion = build_client_assertion(&oauth_credentials(&config)?)?;
    let claims = verify_rs256(&assertion)?;

    assert_eq!(claims["iss"], CLIENT_ID);
    assert_eq!(claims["sub"], CLIENT_ID);
    assert_eq!(claims["aud"], authorization_url);
    assert_eq!(
        claims["exp"].as_u64().context("exp")? - claims["nbf"].as_u64().context("nbf")?,
        300
    );
    let again = verify_rs256(&build_client_assertion(&oauth_credentials(&config)?)?)?;
    assert_ne!(claims["jti"], again["jti"]);
    Ok(())
}

#[test]
fn jit_config_missing_a_file_is_rejected() -> Result<()> {
    let encoded = encoded_jit_config("https://token.actions.githubusercontent.com/oauth/token")?;
    let mut files: BTreeMap<String, String> = serde_json::from_slice(&STANDARD.decode(encoded)?)?;
    files.remove(".credentials_rsaparams");

    let error = decode_jit_config(&STANDARD.encode(serde_json::to_string(&files)?))
        .err()
        .context("a missing RSA parameter file must fail")?;

    assert!(
        format!("{error:#}").contains(".credentials_rsaparams"),
        "{error:#}"
    );
    Ok(())
}

enum TokenOutcome {
    Token(&'static str, Option<Duration>),
    RegistrationGone(&'static str),
    Failed(&'static str),
}

fn token_cases() -> Vec<(&'static str, Scripted, TokenOutcome)> {
    vec![
        (
            "token",
            Scripted::Json(
                200,
                json!({"access_token": "sentinel-access-token", "expires_in": 3600}),
            ),
            TokenOutcome::Token("sentinel-access-token", Some(Duration::from_secs(3600))),
        ),
        (
            "token without an expiry",
            Scripted::Json(200, json!({"access_token": "sentinel-access-token"})),
            TokenOutcome::Token("sentinel-access-token", None),
        ),
        (
            "invalid_client means the registration is gone",
            Scripted::Json(
                400,
                json!({"error": "invalid_client", "error_description": "runner was removed"}),
            ),
            TokenOutcome::RegistrationGone("runner was removed"),
        ),
        (
            "invalid_client is matched case-insensitively",
            Scripted::Json(
                400,
                json!({"error": "Invalid_Client", "error_description": "removed"}),
            ),
            TokenOutcome::RegistrationGone("removed"),
        ),
        (
            "another OAuth error",
            Scripted::Json(
                400,
                json!({"error": "invalid_grant", "error_description": "expired assertion"}),
            ),
            TokenOutcome::Failed("invalid_grant"),
        ),
        (
            "an HTTP 400 never establishes credentials, even with an access token",
            Scripted::Json(400, json!({"access_token": "sentinel-access-token"})),
            TokenOutcome::Failed("HTTP status 400"),
        ),
        (
            "server error",
            Scripted::Text(500, "temporary"),
            TokenOutcome::Failed("HTTP status 500"),
        ),
        (
            "empty access token",
            Scripted::Json(200, json!({"access_token": ""})),
            TokenOutcome::Failed("missing access_token"),
        ),
    ]
}

#[tokio::test]
async fn oauth_exchange_signs_the_assertion_and_classifies_the_response() -> Result<()> {
    let server = TestServer::start().await?;
    let authorization_url = format!("{}{TOKEN_PATH}", server.uri());
    let credentials = oauth_credentials(&decoded_config(&authorization_url)?)?;

    for (name, script, expected) in token_cases() {
        server.reset()?;
        server.respond("POST", TOKEN_PATH, scripted(vec![script]))?;

        let outcome = OAuthClient::new()?
            .exchange_client_credentials(&credentials)
            .await;

        match (expected, outcome) {
            (TokenOutcome::Token(token, expires_in), Ok(actual)) => {
                assert_eq!(actual.token, token, "{name}");
                assert_eq!(actual.expires_in, expires_in, "{name}");
            }
            (TokenOutcome::RegistrationGone(description), Err(error)) => {
                let gone = error
                    .downcast_ref::<OAuthRegistrationNotFound>()
                    .with_context(|| format!("{name}: {error:#}"))?;
                assert_eq!(gone.0, description, "{name}");
            }
            (TokenOutcome::Failed(fragment), Err(error)) => {
                assert!(
                    error.downcast_ref::<OAuthRegistrationNotFound>().is_none(),
                    "{name}"
                );
                assert!(format!("{error:#}").contains(fragment), "{name}: {error:#}");
            }
            (_, outcome) => bail!("{name}: unexpected outcome {outcome:?}"),
        }

        let request = server.only_request("POST", TOKEN_PATH)?;
        assert_eq!(request.header("authorization"), None, "{name}");
        assert_eq!(
            request.header("content-type"),
            Some("application/x-www-form-urlencoded"),
            "{name}"
        );
        let form = request.form();
        assert_eq!(
            form.get("grant_type").map(String::as_str),
            Some("client_credentials")
        );
        assert_eq!(
            form.get("client_assertion_type").map(String::as_str),
            Some(GRANT_TYPE)
        );
        let claims = verify_rs256(form.get("client_assertion").context("assertion")?)?;
        assert_eq!(claims["aud"], authorization_url, "{name}");
    }
    Ok(())
}

#[tokio::test]
async fn oauth_exchange_refuses_a_plain_http_token_endpoint_off_loopback() -> Result<()> {
    let config = decoded_config("http://token.example.test/oauth/token")?;

    let error = OAuthClient::new()?
        .exchange_client_credentials(&oauth_credentials(&config)?)
        .await
        .err()
        .context("a non-loopback http token endpoint must be refused")?;

    assert!(format!("{error:#}").contains("must use HTTPS"), "{error:#}");
    Ok(())
}
