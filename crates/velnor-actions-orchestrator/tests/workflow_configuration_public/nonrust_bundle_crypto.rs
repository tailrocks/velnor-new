// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

use super::{nonrust_tools_fixture::Fixture, tools_cache_verify_hook::Sandbox};

#[test]
fn authentic_inline_bundle_replay_still_requires_valid_signatures() -> Result<(), Box<dyn Error>> {
    let (fixture, _replay) = Fixture::load()?;
    let runner = Sandbox::new()?;
    fixture.provision(&runner)?;
    let cache = runner
        .home
        .path()
        .join(fixture.cache.strip_prefix(&fixture.home)?);
    fs::remove_dir_all(cache.join("github"))?;
    fixture.corrupt_bundles()?;
    let output = fixture.install_result(&runner, "gh", "2.103.3")?;
    let error = String::from_utf8(output.stderr)?;
    assert!(!output.status.success(), "corrupt signature accepted");
    assert!(
        error.contains("attestations verification failed"),
        "{error}"
    );
    assert!(
        error.contains("DSSE signature verification failed"),
        "{error}"
    );
    assert!(!runner.mise_root().join("installs/gh/2.103.3").exists());
    assert_eq!(fixture.metrics()?.misses, 0);
    eprintln!("actual_mise_corrupted_inline_signature_rejected={error:?}");
    Ok(())
}
