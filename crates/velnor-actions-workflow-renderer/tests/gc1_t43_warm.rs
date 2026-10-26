// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) struct BridgeOutcome {
    pub(crate) code: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

pub(crate) fn write_bridge_source(
    dir: &std::path::Path,
) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let path = dir.join("cache_bootstrap.py");
    std::fs::write(&path, CACHE_BOOTSTRAP_SCRIPT)?;
    Ok(path)
}

pub(crate) fn run_bridge_source(
    source: &std::path::Path,
    env: &[(String, String)],
) -> Result<BridgeOutcome, Box<dyn Error>> {
    let output = Command::new("python3")
        .arg(source)
        .envs(env.iter().map(|pair| (pair.0.as_str(), pair.1.as_str())))
        .output()?;
    Ok(BridgeOutcome {
        code: output
            .status
            .code()
            .ok_or_else(|| fail("bridge left no exit code"))?,
        stdout: String::from_utf8(output.stdout)?,
        stderr: String::from_utf8(output.stderr)?,
    })
}

pub(crate) fn member_env(
    payload: &str,
    member: &str,
    sha: &str,
    staged: &str,
    extra: &[(&str, &str)],
) -> Vec<(String, String)> {
    let mut env = vec![
        (
            "VELNOR_BOOTSTRAP_PAYLOAD_DIR".to_owned(),
            payload.to_owned(),
        ),
        ("VELNOR_BOOTSTRAP_MEMBER".to_owned(), member.to_owned()),
        (
            "VELNOR_BOOTSTRAP_EXPECTED_SHA256".to_owned(),
            sha.to_owned(),
        ),
        ("VELNOR_BOOTSTRAP_STAGED_PATH".to_owned(), staged.to_owned()),
        (
            "VELNOR_BOOTSTRAP_MAX_BYTES".to_owned(),
            "67108864".to_owned(),
        ),
    ];
    env.extend(
        extra
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
    );
    env
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

pub(crate) struct WarmFixture {
    pub(crate) root: tempfile::TempDir,
    pub(crate) source: std::path::PathBuf,
    pub(crate) payload: String,
    pub(crate) member: String,
    pub(crate) sha: String,
}

pub(crate) fn warm_fixture() -> Result<WarmFixture, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let source = write_bridge_source(root.path())?;
    let payload = root.path().join("payload");
    std::fs::create_dir_all(payload.join("controller"))?;
    let member = format!("controller/velnor-actions-{}", version());
    let bytes = b"pinned-controller-bytes";
    std::fs::write(payload.join(&member), bytes)?;
    Ok(WarmFixture {
        source,
        payload: payload
            .to_str()
            .ok_or_else(|| fail("payload path"))?
            .to_owned(),
        member,
        sha: sha256_hex(bytes),
        root,
    })
}

pub(crate) fn staged_under(fixture: &WarmFixture, name: &str) -> Result<String, Box<dyn Error>> {
    Ok(fixture
        .root
        .path()
        .join(name)
        .to_str()
        .ok_or_else(|| fail("staged path"))?
        .to_owned())
}

pub(crate) fn check_bridge_warm_path(fixture: &WarmFixture) -> Result<(), Box<dyn Error>> {
    let stage = fixture.root.path().join("stage");
    std::fs::create_dir_all(&stage)?;
    let staged = stage
        .join(format!("velnor-actions-{}", version()))
        .to_str()
        .ok_or_else(|| fail("staged path"))?
        .to_owned();
    let warm = member_env(
        &fixture.payload,
        &fixture.member,
        &fixture.sha,
        &staged,
        &[],
    );
    let outcome = run_bridge_source(&fixture.source, &warm)?;
    if outcome.code != 0 {
        return Err(fail(&format!("warm bridge refused:{}", outcome.stderr)));
    }
    require(&outcome.stdout, "member_bytes=23", "warm observations")?;
    require(&outcome.stdout, "payload_files=1", "warm observations")?;
    if std::fs::read(&staged)? != b"pinned-controller-bytes" {
        return Err(fail("warm bridge staged unverified bytes"));
    }
    if std::fs::metadata(&staged)?.permissions().mode() & 0o111 == 0 {
        return Err(fail("warm bridge left its staged binary unexecutable"));
    }
    Ok(())
}

pub(crate) fn check_bridge_cold_paths(fixture: &WarmFixture) -> Result<(), Box<dyn Error>> {
    let missing = member_env(
        &fixture.payload,
        "controller/absent-binary",
        &fixture.sha,
        &staged_under(fixture, "miss")?,
        &[],
    );
    let outcome = run_bridge_source(&fixture.source, &missing)?;
    if outcome.code != 10 {
        return Err(fail("absent member left its cold miss"));
    }
    let wrong = member_env(
        &fixture.payload,
        &fixture.member,
        &"0".repeat(64),
        &staged_under(fixture, "wrong")?,
        &[],
    );
    let outcome = run_bridge_source(&fixture.source, &wrong)?;
    if outcome.code != 1 || !outcome.stderr.contains("digest_mismatch") {
        return Err(fail("digest mismatch left its rejection"));
    }
    let shadowed = member_env(
        &fixture.payload,
        &fixture.member,
        &fixture.sha,
        &staged_under(fixture, "shadow")?,
        &[("VELNOR_BOOTSTRAP_SHADOW_STATE", "conflicting-visible-ref")],
    );
    let outcome = run_bridge_source(&fixture.source, &shadowed)?;
    if outcome.code != 10 {
        return Err(fail("shadowed entry left its cold miss"));
    }
    Ok(())
}

pub(crate) fn check_bridge_entry_guard(fixture: &WarmFixture) -> Result<(), Box<dyn Error>> {
    let admitted = member_env(
        &fixture.payload,
        &fixture.member,
        &fixture.sha,
        &staged_under(fixture, "admit")?,
        &[
            ("VELNOR_BOOTSTRAP_EXPECTED_KEY", "mise-tools-v2-exact"),
            ("VELNOR_BOOTSTRAP_MATCHED_KEY", "mise-tools-v2-exact"),
            ("VELNOR_BOOTSTRAP_SHADOW_STATE", BOOTSTRAP_SHADOW_CLEAR),
        ],
    );
    let outcome = run_bridge_source(&fixture.source, &admitted)?;
    if outcome.code != 0 {
        return Err(fail(&format!(
            "exact admitted entry refused:{}",
            outcome.stderr
        )));
    }
    let prefixed = member_env(
        &fixture.payload,
        &fixture.member,
        &fixture.sha,
        &staged_under(fixture, "prefix")?,
        &[
            ("VELNOR_BOOTSTRAP_EXPECTED_KEY", "mise-tools-v2-exact"),
            (
                "VELNOR_BOOTSTRAP_MATCHED_KEY",
                "mise-tools-v2-longer-prefix-shadow",
            ),
        ],
    );
    let outcome = run_bridge_source(&fixture.source, &prefixed)?;
    if outcome.code != 10 {
        return Err(fail("prefix-only match left its exact-entry guard"));
    }
    Ok(())
}

pub(crate) fn check_bridge_executes_contract() -> Result<(), Box<dyn Error>> {
    let fixture = warm_fixture()?;
    check_bridge_warm_path(&fixture)?;
    check_bridge_cold_paths(&fixture)?;
    check_bridge_entry_guard(&fixture)?;
    Ok(())
}
