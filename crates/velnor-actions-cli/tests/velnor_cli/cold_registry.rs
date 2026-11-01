// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use super::fixture;

mod cargo_1_99_1 {
    pub(super) const COLD_NO_MATCHING_PACKAGE: &str = "error: no matching package named `itoa` found\nlocation searched: crates.io index\nrequired by package `probe v0.1.0 (/work/probe)`\nnote: offline mode (via `--offline`) can sometimes cause surprising resolution failures\nhelp: if this error is too confusing you may wish to retry without `--offline`\n";

    pub(super) const COLD_HTTP_REQUEST: &str = "error: failed to download `itoa v1.0.15`\n\nCaused by:\n  attempting to make an HTTP request, but --offline was specified\n";

    pub(super) const TRANSIENT_CONNECT: &str = "    Updating crates.io index\nerror: failed to get `itoa` as a dependency of package `probe v0.1.0 (/work/probe)`\n\nCaused by:\n  failed to load source for dependency `itoa`\n\nCaused by:\n  unable to update registry `crates-io`\n\nCaused by:\n  download of config.json failed\n\nCaused by:\n  [7] Couldn't connect to server (Failed to connect to 127.0.0.1 port 9 after 0 ms: Couldn't connect to server)\n";

    pub(super) const MALFORMED_TOML: &str =
        "error: unclosed table, expected `]`\n --> Cargo.toml:5:5\n  |\n5 | [lib\n  |     ^\n";

    pub(super) const MALFORMED_MANIFEST: &str = "error: failed to parse manifest at `/work/probe/Cargo.toml`\n\nCaused by:\n  feature `x` includes `y` which is neither a dependency nor another feature\n\n  help: a feature with a similar name exists: `x`\n";

    pub(super) const MALFORMED_MANIFEST_NETWORK_PATH: &str = "error: failed to parse manifest at `/work/network/Cargo.toml`\n\nCaused by:\n  feature `timeout` includes `connection` which is neither a dependency nor another feature\n";

    pub(super) const MALFORMED_AFTER_WARNING: &str = "warning: unused manifest key: package.foo\nerror: unclosed table, expected `]`\n --> Cargo.toml:5:5\n";
}

const TRANSIENT_TIMEOUT: &str = "error: operation timed out\n";
const TRANSIENT_UNREACHABLE: &str = "error: network unreachable\n";
const TRANSIENT_AFTER_WARNING: &str =
    "warning: unused manifest key: package.foo\nerror: operation timed out\n";

#[test]
fn cold_registry_generate_names_the_warm_command() -> Result<(), Box<dyn Error>> {
    for stderr in [
        cargo_1_99_1::COLD_NO_MATCHING_PACKAGE,
        cargo_1_99_1::COLD_HTTP_REQUEST,
    ] {
        let fixture = fixture::Fixture::new()?;
        fixture.fail_qualification(stderr)?;
        let output = fixture.root.join("generated");
        let generated = fixture.generate_unguarded(&output)?;
        let text = fixture::stderr(&generated);
        assert_eq!(generated.status.code(), Some(1), "{text}");
        assert!(
            text.contains(
                "velnor-actions: cold_registry: Cargo.toml: dependency not in the local registry; run `cargo fetch --locked --manifest-path Cargo.toml` (or the dependency does not exist)"
            ),
            "{text}"
        );
        assert!(!text.contains("preparation_incomplete"), "{text}");
        assert!(!output.exists());
    }
    Ok(())
}

#[test]
fn transient_metadata_failure_is_not_reported_as_cold_registry() -> Result<(), Box<dyn Error>> {
    for (stderr, cause) in [
        (TRANSIENT_TIMEOUT, "operation timed out"),
        (TRANSIENT_UNREACHABLE, "network unreachable"),
        (
            cargo_1_99_1::TRANSIENT_CONNECT,
            "failed to get `itoa` as a dependency of package",
        ),
        (TRANSIENT_AFTER_WARNING, "operation timed out"),
    ] {
        let fixture = fixture::Fixture::new()?;
        fixture.fail_qualification(stderr)?;
        let output = fixture.root.join("generated");
        let generated = fixture.generate_unguarded(&output)?;
        let text = fixture::stderr(&generated);
        assert_eq!(generated.status.code(), Some(1), "{text}");
        assert!(
            text.contains(&format!(
                "preparation_incomplete: Cargo.toml: metadata_transient:error: {cause}"
            )),
            "{text}"
        );
        assert!(!text.contains("Updating crates.io index"), "{text}");
        assert!(!text.contains("unused manifest key"), "{text}");
        assert!(!text.contains("cold_registry"), "{text}");
        assert!(!text.contains("cargo fetch"), "{text}");
        assert!(!output.exists());
    }
    Ok(())
}

#[test]
fn malformed_manifest_failure_is_not_reported_as_cold_registry() -> Result<(), Box<dyn Error>> {
    for (stderr, diagnostic) in [
        (cargo_1_99_1::MALFORMED_TOML, "unclosed table, expected `]`"),
        (
            cargo_1_99_1::MALFORMED_MANIFEST,
            "failed to parse manifest at",
        ),
        (
            cargo_1_99_1::MALFORMED_MANIFEST_NETWORK_PATH,
            "failed to parse manifest at `/work/network/Cargo.toml`",
        ),
        (
            cargo_1_99_1::MALFORMED_AFTER_WARNING,
            "metadata_failed:error: unclosed table, expected `]`",
        ),
    ] {
        let fixture = fixture::Fixture::new()?;
        fixture.fail_qualification(stderr)?;
        let output = fixture.root.join("generated");
        let generated = fixture.generate_unguarded(&output)?;
        let text = fixture::stderr(&generated);
        assert_eq!(generated.status.code(), Some(1), "{text}");
        assert!(text.contains(diagnostic), "{text}");
        assert!(!text.contains("cold_registry"), "{text}");
        assert!(!text.contains("metadata_offline"), "{text}");
        assert!(!text.contains("metadata_transient"), "{text}");
        assert!(!output.exists());
    }
    Ok(())
}
