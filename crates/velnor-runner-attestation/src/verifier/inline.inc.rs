// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

async fn qualify_subject_bytes(
    bundle_bytes: &[u8],
    expected: &ExpectedClaims,
    subject: &ExpectedSubject,
    peer: &ExpectedSubject,
    state_directory: &std::path::Path,
    deadline: Instant,
) -> Result<(), Box<dyn Error>> {
    if bundle_bytes.len() > MAX_BUNDLE_BYTES {
        return Err("verification bundle exceeds bound".into());
    }
    let bundle: Bundle = serde_json::from_slice(bundle_bytes)?;
    let digest = Sha256Hash::from_hex(&subject.digest)?;
    let trusted_root = load_public_good_trusted_root(state_directory).await?;
    let expected = expected.clone();
    let subject = subject.clone();
    let peer = peer.clone();
    tokio::task::spawn_blocking(move || -> Result<(), std::io::Error> {
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "verification deadline exceeded before signature verification",
            ));
        }
        let verifier = Verifier::new(&trusted_root).map_err(verification_io_error)?;
        let policy = VerificationPolicy::new(
            IdentityMatcher::Uri(expected.signer.clone()),
            GITHUB_ACTIONS_ISSUER,
        )
        .skip_sct();
        verifier
            .verify(digest, &bundle, &policy)
            .map_err(verification_io_error)?;
        validate_claims(&bundle, &expected, &subject, &peer).map_err(verification_io_error)?;
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "verification deadline exceeded during signature verification",
            ));
        }
        Ok(())
    })
    .await??;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InlineVerifyRequest {
    schema: u8,
    state_directory: String,
    bundle_base64: String,
    expected: ExpectedClaims,
    subject: ExpectedSubject,
    peer_subject: ExpectedSubject,
}

#[derive(Debug, Serialize)]
pub(crate) struct InlineVerifyResponse {
    schema: u8,
    result: &'static str,
}

pub(crate) async fn verify_inline_subject(
    request: InlineVerifyRequest,
    deadline: Instant,
) -> Result<InlineVerifyResponse, Box<dyn Error>> {
    if request.schema != 3 {
        return Err("unsupported request schema".into());
    }
    let state_directory = validate_state_directory(&request.state_directory)?;
    let bundle_bytes = base64::engine::general_purpose::STANDARD.decode(&request.bundle_base64)?;
    if bundle_bytes.len() > MAX_BUNDLE_BYTES {
        return Err("verification bundle exceeds bound".into());
    }
    validate_expected_inputs(&request.expected, &request.subject, &request.peer_subject)?;
    qualify_subject_bytes(
        &bundle_bytes,
        &request.expected,
        &request.subject,
        &request.peer_subject,
        &state_directory,
        deadline,
    )
    .await?;
    Ok(InlineVerifyResponse {
        schema: 3,
        result: "verified",
    })
}

fn validate_state_directory(value: &str) -> Result<std::path::PathBuf, Box<dyn Error>> {
    use std::path::Path;

    let components = value.strip_prefix('/');
    if value.is_empty()
        || value.len() > 4096
        || value.chars().any(char::is_control)
        || !components.is_some_and(|path| {
            !path.is_empty()
                && path
                    .split('/')
                    .all(|component| !component.is_empty() && component != "." && component != "..")
        })
    {
        return Err("state directory is malformed or exceeds byte limit".into());
    }
    crate::tuf_state::validate_private_directory_path(Path::new(value))?;
    let canonical = std::fs::canonicalize(value)?;
    if canonical != Path::new(value) {
        return Err("state directory is not canonical".into());
    }
    Ok(canonical)
}

fn validate_expected_inputs(
    expected: &ExpectedClaims,
    subject: &ExpectedSubject,
    peer: &ExpectedSubject,
) -> Result<(), Box<dyn Error>> {
    for value in [
        &expected.signer,
        &expected.source,
        &expected.source_ref,
        &expected.build_config,
        &subject.name,
        &peer.name,
    ] {
        if value.is_empty() || value.len() > 4096 {
            return Err("expected claim value is empty or exceeds byte limit".into());
        }
    }
    for commit_id in [
        &expected.signer_digest,
        &expected.source_digest,
        &expected.build_config_digest,
    ] {
        if !is_lower_hex(commit_id, 40) {
            return Err("expected Git commit ID is malformed".into());
        }
    }
    for digest in [&subject.digest, &peer.digest] {
        if !is_lower_hex(digest, 64) {
            return Err("expected SHA-256 digest is malformed".into());
        }
    }
    Ok(())
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
