use super::{
    BTreeMap, ContractError, HomebrewFormulaAssetLiteral, HomebrewFormulaReleaseLiterals,
    HomebrewFormulaSpec, MAX_FORMULA_BYTES, PackageReleaseChannel, ValidatedHomebrewProjection,
    canonical_channel_version_matches, invalid, is_full_sha, is_sha256, render_homebrew_formula,
    safe_release_tag, safe_version, unquote, validate_projection_spec,
};

pub fn parse_generated_formula(
    spec: &HomebrewFormulaSpec,
    projection: &ValidatedHomebrewProjection,
    bytes: &[u8],
) -> Result<HomebrewFormulaReleaseLiterals, ContractError> {
    if bytes.len() > MAX_FORMULA_BYTES {
        return Err(invalid("formula_too_large"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("formula_not_utf8"))?;
    validate_projection_spec(spec, projection)?;
    let source = text
        .lines()
        .find_map(|line| line.strip_prefix("# source-sha: "));
    let version = text
        .lines()
        .find_map(|line| line.strip_prefix("  version \""))
        .and_then(|line| line.strip_suffix('"'))
        .ok_or_else(|| invalid("formula_version_missing"))?;
    if projection.channel == PackageReleaseChannel::Stable && version == "0.0.0" {
        let literals = HomebrewFormulaReleaseLiterals::stable_sentinel(spec, projection)?;
        if render_homebrew_formula(spec, &literals)?.as_slice() != bytes {
            return Err(invalid("formula_template_mismatch"));
        }
        return Ok(literals);
    }
    let source = source.ok_or_else(|| invalid("formula_source_sha_missing"))?;
    let tag = match projection.channel {
        PackageReleaseChannel::Preview => format!("preview-{source}"),
        PackageReleaseChannel::Stable => format!("v{version}"),
    };
    let digests = recognize_digests(text, projection, &tag)?;
    if !is_full_sha(source)
        || source == "0000000000000000000000000000000000000000"
        || !safe_version(version)
        || !canonical_channel_version_matches(projection.channel, source, version)
        || !safe_release_tag(&tag, projection.channel, source, version)
    {
        return Err(invalid("formula_release_identity_malformed"));
    }
    let assets = projection
        .selected
        .iter()
        .map(|payload| {
            let sha256 = digests
                .get(&payload.asset_name)
                .ok_or_else(|| invalid("formula_release_literal_missing"))?;
            if !is_sha256(sha256) || sha256.bytes().all(|byte| byte == b'0') {
                return Err(invalid("formula_release_digest_malformed"));
            }
            Ok(HomebrewFormulaAssetLiteral {
                name: payload.asset_name.clone(),
                url: format!(
                    "https://github.com/{}/releases/download/{}/{}",
                    projection.repository, tag, payload.asset_name
                ),
                sha256: sha256.clone(),
            })
        })
        .collect::<Result<Vec<_>, ContractError>>()?;
    let literals = HomebrewFormulaReleaseLiterals {
        source_sha: Some(source.to_owned()),
        version: version.to_owned(),
        tag: Some(tag),
        assets,
        channel: projection.channel,
        repository: projection.repository.clone(),
        selected: projection.selected.clone(),
        spec_digest: spec.digest()?,
        disabled_stable_sentinel: false,
    };
    if render_homebrew_formula(spec, &literals)?.as_slice() != bytes {
        return Err(invalid("formula_template_mismatch"));
    }
    Ok(literals)
}

fn recognize_digests(
    text: &str,
    projection: &ValidatedHomebrewProjection,
    tag: &str,
) -> Result<BTreeMap<String, String>, ContractError> {
    let mut digests = BTreeMap::new();
    let lines = text.lines().collect::<Vec<_>>();
    for payload in &projection.selected {
        let hash = lines
            .windows(2)
            .find_map(|pair| {
                let url = pair[0].trim().strip_prefix("url ").and_then(unquote)?;
                if url
                    != format!(
                        "https://github.com/{}/releases/download/{}/{}",
                        projection.repository, tag, payload.asset_name
                    )
                {
                    return None;
                }
                pair[1].trim().strip_prefix("sha256 ").and_then(unquote)
            })
            .ok_or_else(|| invalid("formula_release_literal_missing"))?;
        digests.insert(payload.asset_name.clone(), hash.to_owned());
    }
    Ok(digests)
}
