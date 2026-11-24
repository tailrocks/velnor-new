// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use zeroize::Zeroizing;

use super::{OUTPUT_VARIABLE, append_to};
use crate::app_token::{AppKey, jwt, mint};
use crate::cli::AppTokenArgs;
use crate::error::CiError;
use crate::github::GitHubWrite;

const CLIENT_VARIABLE: &str = "VELNOR_APP_CLIENT_ID";
const KEY_VARIABLE: &str = "VELNOR_APP_PRIVATE_KEY";
const TOKEN_VARIABLE: &str = "VELNOR_APP_TOKEN";
const MINT_TIMEOUT: Duration = Duration::from_secs(60);

fn valid_client_id(id: &str) -> bool {
    id.starts_with(|c: char| c.is_ascii_alphanumeric())
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
}

fn signed_jwt() -> Result<Zeroizing<String>, CiError> {
    let client_id = std::env::var(CLIENT_VARIABLE)
        .ok()
        .filter(|id| valid_client_id(id))
        .ok_or_else(|| {
            CiError::new(format!(
                "set {CLIENT_VARIABLE} to the App's client ID (letters, digits and dots)"
            ))
        })?;
    let pem = Zeroizing::new(
        std::env::var(KEY_VARIABLE)
            .ok()
            .filter(|key| !key.is_empty())
            .ok_or_else(|| CiError::new(format!("set {KEY_VARIABLE} to the App's private key")))?,
    );
    let key = AppKey::from_pem(&pem)?;
    drop(pem);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CiError::new("the system clock is before 1970"))?
        .as_secs();
    jwt(&key, &client_id, now)
}

pub(super) fn run(args: &AppTokenArgs) -> Result<String, CiError> {
    if std::env::var_os(OUTPUT_VARIABLE).is_none_or(|file| file.is_empty()) {
        return Err(CiError::new(format!(
            "{OUTPUT_VARIABLE} is not set, so the token would have nowhere to go"
        )));
    }
    let app = super::fetch::api_client(signed_jwt()?.to_string(), MINT_TIMEOUT)?;
    let token = mint(
        &app,
        &args.owner,
        &args.repositories,
        &args.permissions,
        &std::thread::sleep,
    )
    .map_err(|e| e.context("no installation token was created"))?;
    drop(app);
    let mut stdout = std::io::stdout().lock();
    let masked = writeln!(stdout, "::add-mask::{}", token.as_str()).and_then(|()| stdout.flush());
    drop(stdout);
    let written = masked
        .map_err(|e| CiError::new(format!("cannot mask the token: {e}")))
        .and_then(|()| {
            append_to(
                OUTPUT_VARIABLE,
                Zeroizing::new(format!("token={}\n", token.as_str())).as_str(),
            )
        });
    if let Err(error) = written {
        let revoked = super::fetch::api_client(token.to_string(), MINT_TIMEOUT)
            .and_then(|github| github.delete(super::fetch::REVOKE_PATH));
        return Err(match revoked {
            Ok(()) => error.context("the token was revoked"),
            Err(_) => error.context("the token could not be revoked; it expires within the hour"),
        });
    }
    let granted: Vec<String> = args.permissions.iter().map(ToString::to_string).collect();
    Ok(format!(
        "Created a token with {} on {} repositories of {}\n",
        granted.join(", "),
        args.repositories.len(),
        args.owner
    ))
}

pub(super) fn revoke() -> Result<String, CiError> {
    let token = std::env::var(TOKEN_VARIABLE)
        .ok()
        .filter(|token| !token.is_empty())
        .ok_or_else(|| CiError::new(format!("set {TOKEN_VARIABLE} to the token to revoke")))?;
    super::fetch::api_client(token, MINT_TIMEOUT)
        .and_then(|github| github.delete(super::fetch::REVOKE_PATH))
        .map_err(|e| e.context("the token could not be revoked; it expires within the hour"))?;
    Ok("Revoked the App token\n".to_owned())
}
