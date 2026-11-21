// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::signature::{RSA_PKCS1_2048_8192_SHA256, RsaKeyPair, UnparsedPublicKey};
use rustls_pki_types::PrivateKeyDer;
use rustls_pki_types::pem::PemObject;
use serde_json::{Value, json};

use crate::io::Repo;

type TestResult = Result<(), Box<dyn Error>>;

const CLIENT_ID: &str = "Iv23liTestClient";
const MINTED: &str = "ghs_mintedInstallationToken0123456789";
const STATELESS: &str = "ghs_eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJ4LXkifQ.c2ln-bmF0dXJl_x";
const INSTALLATION: &str = "/repos/acme/shared-lib/installation";
const ACCESS_TOKENS: &str = "/app/installations/4242/access_tokens";

struct Request {
    line: String,
    authorization: String,
    body: String,
}

struct Api {
    url: String,
    requests: Arc<Mutex<Vec<Request>>>,
}

fn serve(installation: (u16, &str), minted: (u16, &str)) -> Result<Api, Box<dyn Error>> {
    let answers = [installation, minted].map(|(status, body)| (status, body.to_owned()));
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}", listener.local_addr()?);
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(&stream);
            let mut line = String::new();
            reader.read_line(&mut line).ok();
            let (mut authorization, mut length) = (String::new(), 0);
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).ok();
                let header = header.trim_end();
                if header.is_empty() {
                    break;
                }
                let (name, value) = header.split_once(':').unwrap_or_default();
                match name.to_ascii_lowercase().as_str() {
                    "authorization" => value.trim().clone_into(&mut authorization),
                    "content-length" => length = value.trim().parse().unwrap_or(0),
                    _ => {}
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).ok();
            let (status, answer) = if line.starts_with("DELETE") {
                &(204, String::new())
            } else {
                &answers[usize::from(line.starts_with("POST"))]
            };
            if let Ok(mut seen) = seen.lock() {
                seen.push(Request {
                    line: line.trim_end().to_owned(),
                    authorization,
                    body: String::from_utf8_lossy(&body).into_owned(),
                });
            }
            drop(stream.write_all(
                format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                )
                .as_bytes(),
            ));
        }
    });
    Ok(Api { url, requests })
}

const PKCS1: &str = include_str!("data/app_token/TEST-ONLY-not-a-secret-rsa-pkcs1.pem");
const PKCS8: &str = include_str!("data/app_token/TEST-ONLY-not-a-secret-rsa-pkcs8.pem");

struct Key {
    dir: Repo,
    pem: String,
    public: Vec<u8>,
}

impl Key {
    fn load(name: &str, pem: &str) -> Result<Self, Box<dyn Error>> {
        let der = PrivateKeyDer::from_pem_slice(pem.as_bytes())?;
        let pair = match &der {
            PrivateKeyDer::Pkcs1(key) => RsaKeyPair::from_der(key.secret_pkcs1_der()),
            PrivateKeyDer::Pkcs8(key) => RsaKeyPair::from_pkcs8(key.secret_pkcs8_der()),
            _ => return Err("not an RSA key".into()),
        }
        .map_err(|e| e.to_string())?;
        Ok(Self {
            dir: Repo::empty(name),
            pem: pem.to_owned(),
            public: pair.public().as_ref().to_vec(),
        })
    }

    fn pkcs1(name: &str) -> Result<Self, Box<dyn Error>> {
        Self::load(name, PKCS1)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.root().join(name)
    }

    fn verifies(&self, signed: &str, signature: &[u8]) -> bool {
        UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, &self.public)
            .verify(signed.as_bytes(), signature)
            .is_ok()
    }
}

fn mint(key: &str, api: &Api, output: &Path, repositories: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .env_clear()
        .args(["app-token", "--owner", "acme"])
        .env("VELNOR_APP_CLIENT_ID", CLIENT_ID)
        .env("VELNOR_APP_PRIVATE_KEY", key)
        .env("VELNOR_API_URL", &api.url)
        .env("GITHUB_OUTPUT", output);
    for repository in repositories {
        command.args(["--repository", repository]);
    }
    command
        .output()
        .unwrap_or_else(|e| unreachable!("velnor-ci: {e}"))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn minted_answer() -> String {
    json!({"token": MINTED, "expires_at": "2026-11-21T12:00:00Z"}).to_string()
}

#[test]
fn the_jwt_is_rs256_signed_by_a_pkcs1_app_key_with_claims_that_github_accepts() -> TestResult {
    assert_signed_jwt(&Key::load("app-token-jwt-pkcs1", PKCS1)?)
}

#[test]
fn the_jwt_is_rs256_signed_by_a_pkcs8_app_key_with_claims_that_github_accepts() -> TestResult {
    assert_signed_jwt(&Key::load("app-token-jwt-pkcs8", PKCS8)?)
}

fn assert_signed_jwt(key: &Key) -> TestResult {
    let api = serve((200, r#"{"id":4242}"#), (201, &minted_answer()))?;
    let output = key.path("github-output");
    let before = now();
    let run = mint(&key.pem, &api, &output, &["shared-lib", "other-lib"]);
    let after = now();
    assert!(run.status.success(), "{run:?}");
    let requests = api.requests.lock().map_err(|e| e.to_string())?;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].line, format!("GET {INSTALLATION} HTTP/1.1"));
    let jwt = requests[0]
        .authorization
        .strip_prefix("Bearer ")
        .ok_or("no bearer")?;
    assert_eq!(requests[1].authorization, requests[0].authorization);
    let parts: Vec<&str> = jwt.split('.').collect();
    assert_eq!(parts.len(), 3);
    let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0])?)?;
    assert_eq!(header, json!({"alg": "RS256", "typ": "JWT"}));
    let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1])?)?;
    assert_eq!(claims["iss"], CLIENT_ID);
    let iat = claims["iat"].as_u64().ok_or("iat")?;
    let exp = claims["exp"].as_u64().ok_or("exp")?;
    assert!(
        iat + 30 <= before && iat + 120 >= after,
        "iat {iat} vs {before}..{after}"
    );
    assert!(
        exp > after && exp <= before + 600,
        "exp {exp} vs {before}..{after}"
    );
    assert!(exp - iat <= 600, "a JWT of more than ten minutes: {claims}");
    assert_eq!(claims.as_object().map(serde_json::Map::len), Some(3));
    let signature = URL_SAFE_NO_PAD.decode(parts[2])?;
    assert!(key.verifies(&format!("{}.{}", parts[0], parts[1]), &signature));
    assert!(!key.verifies(&format!("{}.{}", parts[0], parts[0]), &signature));
    Ok(())
}

#[test]
fn the_token_request_names_only_the_listed_repositories_and_contents_read() -> TestResult {
    let key = Key::pkcs1("app-token-request")?;
    let api = serve((200, r#"{"id":4242}"#), (201, &minted_answer()))?;
    let output = key.path("github-output");
    let run = mint(&key.pem, &api, &output, &["shared-lib", "other-lib"]);
    assert!(run.status.success(), "{run:?}");
    let requests = api.requests.lock().map_err(|e| e.to_string())?;
    assert_eq!(requests[1].line, format!("POST {ACCESS_TOKENS} HTTP/1.1"));
    let body: Value = serde_json::from_str(&requests[1].body)?;
    assert_eq!(
        body,
        json!({"repositories": ["shared-lib", "other-lib"], "permissions": {"contents": "read"}})
    );
    assert_eq!(fs::read_to_string(&output)?, format!("token={MINTED}\n"));
    Ok(())
}

#[test]
fn the_minted_token_reaches_the_log_only_inside_the_add_mask_command() -> TestResult {
    let key = Key::pkcs1("app-token-mask")?;
    let api = serve((200, r#"{"id":4242}"#), (201, &minted_answer()))?;
    let output = key.path("github-output");
    let run = mint(&key.pem, &api, &output, &["shared-lib"]);
    assert!(run.status.success(), "{run:?}");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    let mut lines = stdout.lines();
    assert_eq!(lines.next(), Some(format!("::add-mask::{MINTED}").as_str()));
    assert!(lines.all(|line| !line.contains(MINTED)), "{stdout}");
    assert!(!stderr.contains(MINTED), "{stderr}");
    let requests = api.requests.lock().map_err(|e| e.to_string())?;
    let jwt = requests[0].authorization.trim_start_matches("Bearer ");
    let body = key.pem.lines().nth(1).ok_or("pem body")?;
    for log in [&stdout, &stderr] {
        assert!(!log.contains(jwt) && !log.contains(body), "{log}");
    }
    Ok(())
}

#[test]
fn a_stateless_token_with_dots_and_dashes_is_masked_and_written() -> TestResult {
    let key = Key::pkcs1("app-token-stateless")?;
    let answer = json!({"token": STATELESS, "expires_at": "2026-11-21T12:00:00Z"}).to_string();
    let api = serve((200, r#"{"id":4242}"#), (201, &answer))?;
    let output = key.path("github-output");
    let run = mint(&key.pem, &api, &output, &["shared-lib"]);
    assert!(run.status.success(), "{run:?}");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout.lines().next(),
        Some(format!("::add-mask::{STATELESS}").as_str())
    );
    assert_eq!(fs::read_to_string(&output)?, format!("token={STATELESS}\n"));
    Ok(())
}

#[test]
fn a_refused_mint_writes_no_output_and_never_echoes_the_key_or_the_jwt() -> TestResult {
    let key = Key::pkcs1("app-token-refused")?;
    let answers = [
        (
            (
                401,
                r#"{"message":"A JSON web token could not be decoded"}"#,
            ),
            (201, minted_answer()),
        ),
        (
            (200, r#"{"id":4242}"#),
            (422, r#"{"message":"no"}"#.to_owned()),
        ),
        (
            (200, r#"{"id":4242}"#),
            (201, r#"{"token":"ghs_a\nevil=1"}"#.to_owned()),
        ),
        (
            (200, r#"{"id":4242}"#),
            (201, r#"{"token":"ghs_a evil"}"#.to_owned()),
        ),
        (
            (200, r#"{"id":4242}"#),
            (201, r#"{"token":"ghs_a%0Aevil"}"#.to_owned()),
        ),
        (
            (200, r#"{"id":4242}"#),
            (201, r#"{"token":"ghs_a=evil"}"#.to_owned()),
        ),
        ((200, r#"{"id":"x"}"#), (201, minted_answer())),
    ];
    for (installation, minted) in answers {
        let api = serve(installation, (minted.0, &minted.1))?;
        let output = key.path("github-output");
        let run = mint(&key.pem, &api, &output, &["shared-lib"]);
        assert!(!run.status.success(), "{run:?}");
        assert!(!output.exists(), "{}", fs::read_to_string(&output)?);
        let stderr = String::from_utf8_lossy(&run.stderr);
        let requests = api.requests.lock().map_err(|e| e.to_string())?;
        let jwt = requests[0].authorization.trim_start_matches("Bearer ");
        let body = key.pem.lines().nth(1).ok_or("pem body")?;
        assert!(
            !stderr.contains(jwt) && !stderr.contains(body) && !stderr.contains("evil"),
            "{stderr}"
        );
        assert!(run.stdout.is_empty(), "{run:?}");
    }
    Ok(())
}

#[test]
fn an_unreadable_key_or_a_path_like_name_makes_no_request_and_is_not_echoed() -> TestResult {
    let key = Key::pkcs1("app-token-bad-input")?;
    let garbled = key.pem.replace("MII", "MIX");
    let cases: [(&str, &[&str]); 3] = [
        (&garbled, &["shared-lib"]),
        ("not a key but a secret value", &["shared-lib"]),
        (&key.pem, &["../../app"]),
    ];
    for (pem, repositories) in cases {
        let api = serve((200, r#"{"id":4242}"#), (201, &minted_answer()))?;
        let output = key.path("github-output");
        let run = mint(pem, &api, &output, repositories);
        assert!(!run.status.success(), "{run:?}");
        let stderr = String::from_utf8_lossy(&run.stderr);
        assert!(
            !stderr.contains("secret value") && !stderr.contains("MIX"),
            "{stderr}"
        );
        let requests = api.requests.lock().map_err(|e| e.to_string())?;
        assert!(requests.is_empty(), "{}", requests.len());
        assert!(!output.exists());
    }
    Ok(())
}

#[test]
fn a_token_that_cannot_be_written_is_revoked_at_once() -> TestResult {
    let key = Key::pkcs1("app-token-unwritable")?;
    let api = serve((200, r#"{"id":4242}"#), (201, &minted_answer()))?;
    let output = key.path("missing-directory").join("github-output");
    let run = mint(&key.pem, &api, &output, &["shared-lib"]);
    assert!(!run.status.success(), "{run:?}");
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains("the token was revoked"), "{stderr}");
    assert!(!stderr.contains(MINTED), "{stderr}");
    let requests = api.requests.lock().map_err(|e| e.to_string())?;
    let last = requests.last().ok_or("no request")?;
    assert_eq!(last.line, "DELETE /installation/token HTTP/1.1");
    assert_eq!(last.authorization, format!("Bearer {MINTED}"));
    assert_eq!(requests.len(), 3);
    Ok(())
}
