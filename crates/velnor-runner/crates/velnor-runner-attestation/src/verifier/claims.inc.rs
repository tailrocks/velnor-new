fn verification_io_error(error: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
}

fn is_supported_ci_oid(oid: &str) -> Result<bool, Box<dyn Error>> {
    if !oid.starts_with(CI_OID_PREFIX) {
        return Ok(false);
    }
    if !CI_OIDS.contains(&oid) {
        return Err(format!("unsupported Fulcio CI extension {oid}").into());
    }
    Ok(true)
}

fn extract_claims(bundle: &Bundle) -> Result<BTreeMap<String, Vec<String>>, Box<dyn Error>> {
    let der = bundle
        .signing_certificate()
        .ok_or("bundle has no signing certificate")?;
    let certificate = Certificate::from_der(der.as_bytes())?;
    let mut claims: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for ext in certificate
        .tbs_certificate
        .extensions
        .as_deref()
        .unwrap_or_default()
    {
        let oid = ext.extn_id.to_string();
        if !is_supported_ci_oid(&oid)? {
            continue;
        }
        if ext.critical {
            return Err(format!("critical CI extension {oid}").into());
        }
        let bytes = ext.extn_value.as_bytes();
        let value = if matches!(
            oid.as_str(),
            "1.3.6.1.4.1.57264.1.1"
                | "1.3.6.1.4.1.57264.1.2"
                | "1.3.6.1.4.1.57264.1.3"
                | "1.3.6.1.4.1.57264.1.4"
                | "1.3.6.1.4.1.57264.1.5"
                | "1.3.6.1.4.1.57264.1.6"
        ) {
            std::str::from_utf8(bytes)?.to_owned()
        } else {
            Utf8StringRef::from_der(bytes)?.as_str().to_owned()
        };
        claims.entry(oid).or_default().push(value);
    }
    Ok(claims)
}


fn single_claim<'a>(
    claims: &'a BTreeMap<String, Vec<String>>,
    oid: &str,
) -> Result<&'a str, Box<dyn Error>> {
    let values = claims
        .get(oid)
        .ok_or_else(|| format!("missing CI claim {oid}"))?;
    if values.len() != 1 {
        return Err(format!("CI claim {oid} is duplicated").into());
    }
    Ok(values[0].as_str())
}

fn claim_oid(short: u8) -> String {
    format!("1.3.6.1.4.1.57264.1.{short}")
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedClaims {
    signer: String,
    signer_digest: String,
    source: String,
    source_digest: String,
    source_ref: String,
    build_config: String,
    build_config_digest: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedSubject {
    name: String,
    digest: String,
}

fn validate_claims(
    bundle: &Bundle,
    expected: &ExpectedClaims,
    expected_subject: &ExpectedSubject,
) -> Result<(), Box<dyn Error>> {
    let der = bundle
        .signing_certificate()
        .ok_or("bundle has no signing certificate")?;
    let cert_info = sigstore_verify::crypto::parse_certificate_info(der.as_bytes())?;
    if cert_info.identity.as_deref() != Some(expected.signer.as_str()) {
        return Err("verified certificate identity mismatch".into());
    }
    if cert_info.issuer.as_deref() != Some("https://token.actions.githubusercontent.com") {
        return Err("verified certificate issuer mismatch".into());
    }

    let claims = extract_claims(bundle)?;
    for (short, expected, label) in [
        (8, "https://token.actions.githubusercontent.com", "issuer"),
        (9, expected.signer.as_str(), "signer URI"),
        (10, expected.signer_digest.as_str(), "signer digest"),
        (11, "github-hosted", "runner environment"),
        (12, expected.source.as_str(), "source repository"),
        (13, expected.source_digest.as_str(), "source digest"),
        (14, expected.source_ref.as_str(), "source ref"),
        (18, expected.build_config.as_str(), "build config URI"),
        (
            19,
            expected.build_config_digest.as_str(),
            "build config digest",
        ),
    ] {
        let oid = claim_oid(short);
        if single_claim(&claims, &oid)? != expected {
            return Err(format!("verified CI claim mismatch: {label}").into());
        }
    }

    let SignatureContent::DsseEnvelope(envelope) = &bundle.content else {
        return Err("unsupported signed content type".into());
    };
    let statement: serde_json::Value = serde_json::from_slice(envelope.payload.as_bytes())?;
    if statement
        .get("predicateType")
        .and_then(serde_json::Value::as_str)
        != Some("https://slsa.dev/provenance/v1")
    {
        return Err("unsupported provenance predicate".into());
    }
    let subjects = statement
        .get("subject")
        .and_then(serde_json::Value::as_array)
        .ok_or("provenance subject missing")?;
    let matching: Vec<&serde_json::Value> = subjects
        .iter()
        .filter(|subject| {
            subject.get("name").and_then(serde_json::Value::as_str)
                == Some(expected_subject.name.as_str())
        })
        .collect();
    if matching.len() != 1 {
        return Err("expected provenance subject missing or duplicated".into());
    }
    if matching[0]
        .get("digest")
        .and_then(|digest| digest.get("sha256"))
        .and_then(serde_json::Value::as_str)
        != Some(expected_subject.digest.as_str())
    {
        return Err("provenance subject digest mismatch".into());
    }
    Ok(())
}
