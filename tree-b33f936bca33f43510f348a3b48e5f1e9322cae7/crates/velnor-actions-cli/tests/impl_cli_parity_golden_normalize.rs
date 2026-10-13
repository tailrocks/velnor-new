//! Parity-golden `plan-v1` response normalization (split from
//! `impl_cli_parity_golden.rs` to satisfy the 400-line gate).
use std::error::Error;
use std::path::Path;

/// `plan-v1` response with volatile generator/head fields normalized.
///
/// The generator target triple and executable SHA are environment facts,
/// not planning behavior; both must be present and well-formed, then are
/// replaced so the remaining bytes (obligations, digests, matrix entries,
/// cache IDs, metadata, run vectors) compare exactly. The per-task
/// `input_digest` embeds that same host triple (the running binary names
/// itself; no request override by design), so its value is normalized too
/// after a shape check — task, closure, platform, toolchain, lane, and
/// workspace digests stay byte-exact.
pub(crate) fn normalized_response(
    repo: &Path,
    head: &str,
    bytes: &[u8],
) -> Result<Vec<u8>, Box<dyn Error>> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let generator = value
        .pointer("/plan/generator")
        .ok_or("response lacks plan.generator")?;
    let target = generator
        .get("target")
        .and_then(serde_json::Value::as_str)
        .ok_or("response lacks generator.target")?;
    let sha = generator
        .get("sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or("response lacks generator.sha256")?;
    if target.is_empty() || sha.len() != 64 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("generator identity malformed".into());
    }
    for digest in input_digests(&value)? {
        if digest.len() != 67
            || !digest.starts_with("b3-")
            || !digest.bytes().skip(3).all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("input_digest malformed".into());
        }
    }
    let text = String::from_utf8(bytes.to_vec())?;
    if !text.contains(head) {
        return Err("response lacks the head SHA".into());
    }
    // Raw Cargo package IDs embed the checkout path (both the temp path
    // as passed and its canonicalization); digests never do. Normalize
    // both spellings so package records compare exactly.
    let canonical = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
    // Scoped to the generator object itself: on hosts whose triple equals a
    // planned platform triple (x86_64 Linux CI), a global replace would also
    // rewrite the plan's legitimate `planned_platform.target` values. The
    // generator object is flat (`version`/`target`/`sha256`, all plain
    // strings), so its first `}` closes it.
    let marker = "\"generator\":{";
    let start = text.find(marker).ok_or("response lacks generator object")? + marker.len();
    let end = text[start..]
        .find('}')
        .ok_or("generator object unterminated")?
        + start;
    let span = &text[start..end];
    if !span.contains(target) || !span.contains(sha) {
        return Err("generator identity missing from generator object".into());
    }
    let mut text = format!(
        "{}{}{}",
        &text[..start],
        span.replace(sha, "<generator-sha>")
            .replace(target, "<target>"),
        &text[end..]
    );
    text = text
        .replace(head, "<head>")
        .replace(&canonical.display().to_string(), "<repo>")
        .replace(&repo.display().to_string(), "<repo>");
    // Scoped to `"input_digest":"<value>"` pairs so no other digest field
    // can match; every distinct value must occur or the response shape
    // drifted from the parser. Obligations and matrix entries share
    // values, and `replace` removes all occurrences, so dedupe first.
    let mut seen = std::collections::BTreeSet::new();
    for digest in input_digests(&value)? {
        if !seen.insert(digest) {
            continue;
        }
        let pair = format!("\"input_digest\":\"{digest}\"");
        if !text.contains(&pair) {
            return Err("input_digest pair missing from response text".into());
        }
        text = text.replace(&pair, "\"input_digest\":\"<input-digest>\"");
    }
    Ok(text.into_bytes())
}

/// Every `input_digest` value in obligations plus matrix entries.
fn input_digests(value: &serde_json::Value) -> Result<Vec<&str>, Box<dyn Error>> {
    let mut digests = Vec::new();
    let obligations = value
        .pointer("/plan/obligations")
        .and_then(serde_json::Value::as_array)
        .ok_or("plan lacks obligations")?;
    for obligation in obligations {
        digests.push(
            obligation
                .get("input_digest")
                .and_then(serde_json::Value::as_str)
                .ok_or("obligation lacks input_digest")?,
        );
    }
    let entries = value
        .pointer("/plan/matrix/include")
        .and_then(serde_json::Value::as_array)
        .ok_or("plan lacks matrix.include")?;
    for entry in entries {
        digests.push(
            entry
                .get("input_digest")
                .and_then(serde_json::Value::as_str)
                .ok_or("entry lacks input_digest")?,
        );
    }
    // Empty is legitimate: ignored-stack plans carry no obligations.
    Ok(digests)
}

#[test]
fn generator_scrub_keeps_planned_platform_triples() {
    // On x86_64 Linux CI the generator triple equals the planned platform
    // triple, so only the generator object itself may be scrubbed.
    let sha = "a".repeat(64);
    let digest = format!("b3-{}", "b".repeat(64));
    let head = "c".repeat(40);
    let version = env!("CARGO_PKG_VERSION");
    let response = format!(
        "{{\"plan\":{{\"head\":\"{head}\",\"generator\":{{\"version\":\"{version}\",\
         \"target\":\"x86_64-unknown-linux-gnu\",\"sha256\":\"{sha}\"}},\
         \"obligations\":[{{\"input_digest\":\"{digest}\"}}],\
         \"matrix\":{{\"include\":[{{\"input_digest\":\"{digest}\",\
         \"planned_platform\":{{\"target\":\"x86_64-unknown-linux-gnu\"}}}}]}}}}}}"
    );
    let out = String::from_utf8(
        normalized_response(Path::new("/nonexistent-repo"), &head, response.as_bytes())
            .expect("normalize"),
    )
    .expect("utf8");
    assert!(out.contains(r#""target":"<target>""#), "{out}");
    assert!(out.contains(&format!(r#""version":"{version}""#)), "{out}");
    assert!(out.contains("<generator-sha>"), "{out}");
    assert!(!out.contains(&sha), "{out}");
    assert!(
        out.contains(r#""planned_platform":{"target":"x86_64-unknown-linux-gnu"}"#),
        "{out}"
    );
}
