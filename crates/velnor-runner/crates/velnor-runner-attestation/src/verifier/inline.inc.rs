async fn qualify_checksum_target_bytes(
    bundle_bytes: &[u8],
    checksum_bytes: &[u8],
    expected: &ExpectedClaims,
    checksum_subject: &ExpectedSubject,
    target_subject: &ExpectedSubject,
    deadline: Instant,
) -> Result<(), Box<dyn Error>> {
    if bundle_bytes.len() > MAX_BUNDLE_BYTES {
        return Err("verification bundle exceeds bound".into());
    }
    if checksum_bytes.len() > MAX_CHECKSUM_BYTES {
        return Err("checksum asset exceeds bound".into());
    }
    let bundle: Bundle = serde_json::from_slice(bundle_bytes)?;
    let checksum_text = std::str::from_utf8(checksum_bytes)?;
    let mut target_digests = Vec::new();
    for line in checksum_text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() != 2 {
            return Err("checksum list has unsupported row format".into());
        }
        if fields[1] == target_subject.name {
            target_digests.push(fields[0]);
        }
    }
    if target_digests.len() != 1 || target_digests[0] != target_subject.digest {
        return Err("signed checksum list target digest missing, duplicated, or mismatched".into());
    }
    let digest = Sha256Hash::from_hex(target_digests[0])?;
    let trusted_root = load_public_good_trusted_root().await?;
    let expected = expected.clone();
    let checksum_subject = checksum_subject.clone();
    let target_subject = target_subject.clone();
    let checksum_bytes = checksum_bytes.to_vec();
    tokio::task::spawn_blocking(move || -> Result<(), std::io::Error> {
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "verification deadline exceeded before signature verification",
            ));
        }
        let verifier = Verifier::new(&trusted_root);
        let policy = VerificationPolicy::default()
            .require_identity(expected.signer.clone())
            .require_issuer("https://token.actions.githubusercontent.com")
            .skip_sct();
        verifier
            .verify(checksum_bytes.as_slice(), &bundle, &policy)
            .map_err(verification_io_error)?;
        validate_claims(&bundle, &expected, &checksum_subject).map_err(verification_io_error)?;
        verifier
            .verify(digest, &bundle, &policy)
            .map_err(verification_io_error)?;
        validate_claims(&bundle, &expected, &target_subject).map_err(verification_io_error)?;
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
    bundle_base64: String,
    checksum_base64: String,
    expected: ExpectedClaims,
    checksum_subject: ExpectedSubject,
    target_subject: ExpectedSubject,
}

#[derive(Debug, Serialize)]
pub(crate) struct InlineVerifyResponse {
    schema: u8,
    result: &'static str,
}

pub(crate) async fn verify_inline_checksum_target(
    request: InlineVerifyRequest,
    deadline: Instant,
) -> Result<InlineVerifyResponse, Box<dyn Error>> {
    if request.schema != 1 {
        return Err("unsupported request schema".into());
    }
    let bundle_bytes = base64::engine::general_purpose::STANDARD.decode(&request.bundle_base64)?;
    let checksum_bytes =
        base64::engine::general_purpose::STANDARD.decode(&request.checksum_base64)?;
    if bundle_bytes.len() > MAX_BUNDLE_BYTES {
        return Err("verification bundle exceeds bound".into());
    }
    if checksum_bytes.len() > MAX_CHECKSUM_BYTES {
        return Err("checksum asset exceeds bound".into());
    }
    validate_expected_inputs(&request.expected, &request.checksum_subject, &request.target_subject)?;
    qualify_checksum_target_bytes(
        &bundle_bytes,
        &checksum_bytes,
        &request.expected,
        &request.checksum_subject,
        &request.target_subject,
        deadline,
    )
    .await?;
    Ok(InlineVerifyResponse {
        schema: 1,
        result: "verified",
    })
}

fn validate_expected_inputs(
    expected: &ExpectedClaims,
    checksum: &ExpectedSubject,
    target: &ExpectedSubject,
) -> Result<(), Box<dyn Error>> {
    for value in [
        &expected.signer,
        &expected.source,
        &expected.source_ref,
        &expected.build_config,
        &checksum.name,
        &target.name,
    ] {
        if value.is_empty() || value.len() > 4096 {
            return Err("expected claim value is empty or exceeds byte limit".into());
        }
    }
    for digest in [
        &expected.signer_digest,
        &expected.source_digest,
        &expected.build_config_digest,
        &checksum.digest,
        &target.digest,
    ] {
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("expected SHA-256 digest is malformed".into());
        }
    }
    Ok(())
}
